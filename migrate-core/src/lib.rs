//! TODO: crate-level docs

#![warn(missing_docs, unreachable_pub, rust_2018_idioms)]
// Makes rustc abort compilation if there are any unsafe blocks in the crate.
// Presence of this annotation is picked up by tools such as cargo-geiger
// and lets them ensure that there is indeed no unsafe code as opposed to
// something they couldn't detect (e.g. unsafe added via macro expansion, etc).
#![forbid(unsafe_code)]

mod diff;
mod dyn_migration;
mod error;
mod state;

pub use dyn_migration::{MigrationCtxProvider, MigrationRunMode};
pub use error::*;

use async_trait::async_trait;
use dyn_migration::{CtxRegistry, DynMigration, DynMigrationScriptCtx, MigrationDirection};
use itertools::Itertools;
use migrate_state::{StateGuard, StateLock};
use state::State;
use std::fmt;
use tracing::{info, info_span, instrument};
use tracing_futures::Instrument;

/// Contains the behavior of a single migration that may be applied or reversed
/// using [`Migration::up()`] and [`Migration::down()`] methods respectively.
#[async_trait]
pub trait Migration: Send + 'static {
    /// Defines that type of the context that will be injected for the migration
    /// to use during its execution.
    ///
    /// The context will be created by [`MigrationCtxProvider`] and looked up by
    /// its [`type_id()`][std::any::Any::type_id].
    type Ctx: Send + 'static;

    /// Run the forward migration logic. The given [`Migration::Ctx`] should
    /// be used to perform the execution. The context should take care to
    /// to commit the changes to the target migration object (e.g. a database)
    /// or just collect the diagnostic info about the planned operations
    /// according to the [`MigrationRunMode`].
    ///
    /// The migration is safe to assume that migrations that precede it were
    /// already applied and it may observe the changes made by them.
    async fn up(&mut self, ctx: &mut Self::Ctx) -> Result<(), DynError>;

    /// Similar to [`Migration::up()`], but applies the migration logic in reverse
    /// direction. It may safely assume that this same [`Migration::up()`]
    /// method was run and it may observe the changes made by the forward
    /// migration logic.
    ///
    /// This method should cancel the changes made by the forward migration logic
    /// and basically rollback the state of the migration object to the state
    /// it was before [`Migration::up()`] was called.
    async fn down(&mut self, ctx: &mut Self::Ctx) -> Result<(), DynError>;
}

/// Bbuilder for [`Plan`] to allow its convenient configuration
pub struct PlanBuilder {
    ctx_registry: CtxRegistry,
    migrations: Vec<DynMigration>,
    state_lock: Box<dyn StateLock>,
    force_lock: bool,
}

impl PlanBuilder {
    /// Register the [`MigrationCtxProvider`] that will be used to provide
    /// context for the migrations in the built [`Plan`]
    pub fn ctx_provider(&mut self, provider: impl MigrationCtxProvider) -> &mut Self {
        self.ctx_registry.insert(provider);
        self
    }

    /// Append the [`Migration`] to the list of migrations configured for the plan.
    /// Keep in mind that it is important to keep the migrations in order
    /// and add new migrations strictly to the end of the list so that new
    /// migrations obvserve the changes from previous migrations.
    pub fn migration(
        &mut self,
        name: impl Into<String>,
        migration: impl Migration + 'static,
    ) -> &mut Self {
        self.migrations
            .push(DynMigration::new(name.into(), migration));
        self
    }

    /// Use forced stack lock.
    /// Beware that setting it to `true` is dangerous and may lead to migration
    /// state corruptions!
    /// See more detailed info at [`migrate_state::StateLock::lock()`].
    pub fn force_lock(&mut self, val: bool) -> &mut Self {
        self.force_lock = val;
        self
    }

    /// Create the builder for rendering the current migration configuration
    /// in this [`PlanBuilder`].
    pub fn display(&self) -> MigrationsDisplayBuilder<'_> {
        MigrationsDisplayBuilder(self)
    }

    /// Finish building the migration plan.
    ///
    /// This method reads the migration state and figures out which migrations
    /// to run [`up()`][Migration::up] or [`down`][Migration::down].
    /// This information is stored in the returned [`Plan`] struct.
    ///
    /// There are various reasons for this method to fail, see [`PlanBuildError`]
    /// for more details on possible error outcomes.
    #[instrument(skip(self), err)]
    pub async fn build(self, kind: &MigrationsSelection<'_>) -> Result<Plan, PlanBuildError> {
        info!("Aсquiring the state lock (this may take a moment)...");

        let mut state_guard = self
            .state_lock
            .lock(self.force_lock)
            .await
            .map_err(PlanBuildErrorKind::StateLock)?;
        let state_client = state_guard.client();

        let mut state = State::decode(
            &state_client
                .fetch()
                .await
                .map_err(PlanBuildErrorKind::StateFetch)?,
        )?;

        let mut diff = diff::diff(self.migrations, &mut state.applied_migrations)?;

        let (left_completed, left_pending, kind) = match kind {
            MigrationsSelection::Up { inclusive_bound } => {
                let left_pending = match inclusive_bound {
                    Some(bound) => {
                        let idx = Self::find_migration(&diff.pending, bound)?;
                        diff.pending.split_off(idx + 1)
                    }
                    None => vec![],
                };
                (diff.completed, left_pending, PlanKind::Up(diff.pending))
            }
            MigrationsSelection::Down { inclusive_bound } => {
                let idx = Self::find_migration(&diff.completed, inclusive_bound)?;
                let kind = PlanKind::Down(diff.completed.split_off(idx));
                (diff.completed, diff.pending, kind)
            }
        };

        Ok(Plan {
            ctx_registry: self.ctx_registry,
            state: StateCtx {
                guard: Some(state_guard),
                pruned: diff.pruned,
                state,
            },
            left_completed,
            left_pending,
            kind,
        })
    }

    fn find_migration(migs: &[DynMigration], bound: &str) -> Result<usize, PlanBuildError> {
        migs.iter().position(|it| it.name == bound).ok_or_else(|| {
            // TODO: better error handling here (invalid input)
            PlanBuildErrorKind::UnknownMigration {
                name: bound.to_owned(),
                available: migs.iter().map(|it| it.name.clone()).collect(),
            }
            .into()
        })
    }
}

/// Selects the direction of the migration as well as the bounding migration.
#[derive(Debug)]
pub enum MigrationsSelection<'a> {
    /// Run the forward migration logic
    Up {
        /// Defines the upper inclusive bound for the migrations that should be executed
        inclusive_bound: Option<&'a str>,
    },

    /// Run the reverse migration logic that cancels the actions done in
    /// [`MigrationsSelection::Up`] for migrations that are recorded in
    /// [migration state][`migrate_state`].
    Down {
        /// Defines the lower inclusive bound for the migrations that should be executed.
        /// This is non-[`Option`] on purpose to prevent accidental highly destructive
        /// changes that reverse migrations may incur
        inclusive_bound: &'a str,
    },
}

/// Contains a fixed snapshot of the migration state and the list of migrations
/// that will be either skipped as already completed (according to the migration
/// state) or not selected (as per [`MigrationsSelection`]) and the list of
/// migrations that will be run as a result of exucting this migration [`Plan`].
pub struct Plan {
    ctx_registry: CtxRegistry,
    state: StateCtx,
    // FIXME: use these for displaying the diff in display()
    #[allow(unused)]
    left_completed: Vec<DynMigration>,
    #[allow(unused)]
    left_pending: Vec<DynMigration>,

    kind: PlanKind,
}

impl Plan {
    /// Returns the builder for this [`Plan`] to allow its convenient configuration
    pub fn builder(state_lock: impl StateLock + 'static) -> PlanBuilder {
        PlanBuilder {
            ctx_registry: CtxRegistry::new(),
            migrations: Vec::new(),
            state_lock: Box::new(state_lock),
            force_lock: false,
        }
    }

    /// Returns a builder that will allow for configuring how the migration [`Plan`]
    /// will be rendered via [`std::fmt::Display`] impl.
    pub fn display(&self) -> PlanDisplayBuilder<'_> {
        PlanDisplayBuilder { plan: self }
    }

    /// Execute the migration plan by running the migration scripts.
    #[instrument(skip(self))]
    pub async fn exec(mut self, run_mode: MigrationRunMode) -> Result<(), PlanExecError> {
        let mut errors = vec![];
        let mut guard = self.state.guard.take().unwrap();

        info!("Executing migrations...");
        if let Err(err) = self.try_exec(run_mode).await {
            errors.push(err);
        }

        info!("Saving new migration state data...");
        if let Err(err) = guard.client().update(self.state.state.encode()).await {
            errors.push(PlanExecErrorKind::UpdateState(err));
        }

        info!("Releasing the state lock (this may take a moment)...");
        if let Err(err) = guard.unlock().await {
            errors.push(PlanExecErrorKind::UnlockState(err));
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(PlanExecError { errors })
        }
    }

    async fn try_exec(&mut self, run_mode: MigrationRunMode) -> Result<(), PlanExecErrorKind> {
        // FIXME: add a step for manual approval...

        // FIXME: record the migration as `tainted` (this is concept taken from `terraform`) if it fails,
        // or handle it somehow else?

        let mut ctx = DynMigrationScriptCtx {
            ctx_registry: &mut self.ctx_registry,
            run_mode,
            direction: self.kind.to_migration_direction(),
        };
        match &mut self.kind {
            PlanKind::Up(migrations) => {
                for migration in migrations {
                    let state_entry = state::MigrationMeta {
                        name: migration.name.clone(),
                    };
                    self.state.state.applied_migrations.push(state_entry);

                    let span = info_span!("migrate-up");
                    Self::exec_migration(&mut ctx, migration)
                        .instrument(span)
                        .await?;
                }
            }
            PlanKind::Down(migrations) => {
                for migration in migrations.iter_mut().rev() {
                    let removed = self.state.state.applied_migrations.pop();
                    assert_eq!(removed.unwrap().name, migration.name);

                    let span = info_span!("migrate-down");
                    Self::exec_migration(&mut ctx, migration)
                        .instrument(span)
                        .await?;
                }
            }
        }
        Ok(())
    }

    async fn exec_migration(
        ctx: &mut DynMigrationScriptCtx<'_>,
        migration: &mut DynMigration,
    ) -> Result<(), PlanExecErrorKind> {
        info!(
            migration = migration.name.as_str(),
            direction = %ctx.direction,
            "Executing migration",
        );
        match migration.script.exec(ctx).await {
            Err(PlanExecErrorKind::CtxLacksNoCommitMode) => {
                info!("Migration lacks support for no-commit mode, skipping it...");
                Ok(())
            }
            result => result,
        }
    }
}

/// Contains the configuration information to render the [`PlanBuilder`]
pub struct MigrationsDisplayBuilder<'a>(&'a PlanBuilder);

impl MigrationsDisplayBuilder<'_> {
    /// Finish configuring how the [`PlanBuilder`] should be rendered
    pub fn build(&self) -> impl '_ + fmt::Display {
        MigrationsDisplay(self)
    }
}

struct MigrationsDisplay<'a>(&'a MigrationsDisplayBuilder<'a>);

impl fmt::Display for MigrationsDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let format = &(self.0)
            .0
            .migrations
            .iter()
            .enumerate()
            .format_with("\n", |(i, mig), f| {
                f(&format_args!("{}. {}", i + 1, mig.name))
            });

        write!(f, "{}", format)
    }
}

/// Contains the configuration information to render the migration [`Plan`]
pub struct PlanDisplayBuilder<'p> {
    plan: &'p Plan,
    // FIXME: add colors support
    // colored: bool,
}

impl PlanDisplayBuilder<'_> {
    /// Finish configuring how the [`Plan`] should be rendered
    pub fn build(&self) -> impl '_ + fmt::Display {
        PlanDisplay(self)
    }
}

struct PlanDisplay<'p>(&'p PlanDisplayBuilder<'p>);

impl fmt::Display for PlanDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // FIXME: make the output obey diff format like this:
        // * left-completed
        // - rolled-back (down)
        // + applied (up)
        // * left-pending

        let plan = self.0.plan;

        let (migrations, touched) = match &plan.kind {
            PlanKind::Up(migrations) => (migrations, "applied (up)"),
            PlanKind::Down(migrations) => (migrations, "rolled back (down)"),
        };

        if migrations.is_empty() {
            writeln!(f, "No migrations are planned to be {}", touched)?;
        } else {
            let migrations = plan
                .kind
                .migrations_in_exec_order()
                .format_with("\n", |mig, f| f(&format_args!("- {}", mig.name)));

            writeln!(
                f,
                "The following migrations are planned to be {}:\n{}",
                touched, migrations
            )?;
        }

        if !plan.state.pruned.is_empty() {
            let pruned = plan
                .state
                .pruned
                .iter()
                .format_with("\n", |mig, f| f(&format_args!("- {}", mig.name)));

            writeln!(
                f,
                "\n\nThe following migrations are planned to be pruned: {}",
                pruned
            )?;
        }

        Ok(())
    }
}

enum PlanKind {
    Up(Vec<DynMigration>),
    Down(Vec<DynMigration>),
}

impl PlanKind {
    fn to_migration_direction(&self) -> MigrationDirection {
        match self {
            PlanKind::Up(_) => MigrationDirection::Up,
            PlanKind::Down(_) => MigrationDirection::Down,
        }
    }

    fn migrations_in_exec_order(&self) -> impl Iterator<Item = &DynMigration> {
        match self {
            PlanKind::Up(migrations) => Box::new(migrations.iter()) as Box<dyn Iterator<Item = _>>,
            PlanKind::Down(migrations) => Box::new(migrations.iter().rev()),
        }
    }
}

struct StateCtx {
    guard: Option<Box<dyn StateGuard>>,
    pruned: Vec<state::MigrationMeta>,
    state: state::State,
}
