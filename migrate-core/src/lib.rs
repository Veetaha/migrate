// TODO: uncomment
// #![warn(missing_docs)]
#![warn(unreachable_pub)]
#![warn(rust_2018_idioms)]
// Makes rustc abort compilation if there are any unsafe blocks in the crate.
// Presence of this annotation is picked up by tools such as cargo-geiger
// and lets them ensure that there is indeed no unsafe code as opposed to
// something they couldn't detect (e.g. unsafe added via macro expansion, etc).
#![forbid(unsafe_code)]

mod diff;
mod dyn_migration;
mod error;
mod state;

pub use error::*;
pub use dyn_migration::{MigrationCtxProvider, MigrationRunMode};

use std::fmt;
use dyn_migration::{
    CtxRegistry, DynMigration, DynMigrationScriptCtx, MigrationDirection,
};
use async_trait::async_trait;
use itertools::Itertools;
use migrate_state::{StateGuard, StateLock};
use state::State;
use tracing::{info, info_span, instrument};
use tracing_futures::Instrument;

#[async_trait]
pub trait Migration: Send + 'static {
    type Ctx: Send + 'static;

    async fn up(&mut self, ctx: &mut Self::Ctx) -> Result<(), DynError>;
    async fn down(&mut self, ctx: &mut Self::Ctx) -> Result<(), DynError>;
}

pub struct PlanBuilder {
    ctx_registry: CtxRegistry,
    migrations: Vec<DynMigration>,
    state_lock: Box<dyn StateLock>,
}

impl PlanBuilder {
    pub fn ctx_provider(&mut self, provider: impl MigrationCtxProvider) -> &mut Self {
        self.ctx_registry.insert(provider);
        self
    }

    pub fn migration(
        &mut self,
        name: impl Into<String>,
        migration: impl Migration + 'static,
    ) -> &mut Self {
        self.migrations
            .push(DynMigration::new(name.into(), migration));
        self
    }

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
    pub async fn finish(self, kind: &MigrationKind<'_>) -> Result<Plan, PlanBuildError> {
        info!("Aсquiring the state lock (this may take a moment)...");

        let mut state_guard = self
            .state_lock
            .lock()
            .await
            .map_err(PlanBuildError::StateLock)?;
        let state_client = state_guard.client();

        let mut state = State::decode(
            &state_client
                .fetch()
                .await
                .map_err(PlanBuildError::StateFetch)?,
        )?;

        let mut diff = diff::diff(self.migrations, &mut state.applied_migrations)?;

        let (left_completed, left_pending, kind) = match kind {
            MigrationKind::Up { inclusive_bound } => {
                let left_pending = match inclusive_bound {
                    Some(bound) => {
                        let idx = Self::find_migration(&diff.pending, bound)?;
                        diff.pending.split_off(idx + 1)
                    }
                    None => vec![],
                };
                (diff.completed, left_pending, PlanKind::Up(diff.pending))
            }
            MigrationKind::Down { inclusive_bound } => {
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
            PlanBuildError::UnknownMigration {
                name: bound.to_owned(),
                available: migs.iter().map(|it| it.name.clone()).collect(),
            }
        })
    }
}

#[derive(Debug)]
pub enum MigrationKind<'a> {
    Up { inclusive_bound: Option<&'a str> },
    Down { inclusive_bound: &'a str },
}

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
    pub fn builder(state_lock: impl StateLock + 'static) -> PlanBuilder {
        PlanBuilder {
            ctx_registry: CtxRegistry::new(),
            migrations: Vec::new(),
            state_lock: Box::new(state_lock),
        }
    }

    pub fn display(&self) -> PlanDisplayBuilder<'_> {
        PlanDisplayBuilder { plan: &self }
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

pub struct MigrationsDisplayBuilder<'a>(&'a PlanBuilder);

impl MigrationsDisplayBuilder<'_> {
    pub fn finish(&self) -> MigrationsDisplay<'_> {
        MigrationsDisplay(self)
    }
}

pub struct MigrationsDisplay<'a>(&'a MigrationsDisplayBuilder<'a>);

impl fmt::Display for MigrationsDisplay<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let format = &(self.0).0.migrations
            .iter()
            .enumerate()
            .format_with("\n", |(i, mig), f| f(&format_args!("{}. {}", i + 1, mig.name)));

        write!(f, "{}", format)
    }
}


pub struct PlanDisplayBuilder<'p> {
    plan: &'p Plan,
    // FIXME: add colors support
    // colored: bool,
}

impl PlanDisplayBuilder<'_> {
    pub fn finish(&self) -> PlanDisplay<'_> {
        PlanDisplay(self)
    }
}

pub struct PlanDisplay<'p>(&'p PlanDisplayBuilder<'p>);

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
