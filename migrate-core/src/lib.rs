#![warn(unreachable_pub)]

mod diff;
mod error;
mod state;

pub use error::*;

use async_trait::async_trait;
use migrate_state::StateLock;
use state::State;
use tracing::instrument;

#[async_trait(?Send)]
pub trait Migration {
    type Ctx: 'static;

    async fn up(&mut self, ctx: &mut Self::Ctx) -> Result<(), AnyError>;
    async fn down(&mut self, ctx: &mut Self::Ctx) -> Result<(), AnyError>;
}

struct TypeErasedMigration<Mig>(Mig);

#[async_trait(?Send)]
impl<Mig: Migration> Migration for TypeErasedMigration<Mig> {
    type Ctx = CtxRegistry;

    async fn up(&mut self, ctx: &mut CtxRegistry) -> Result<(), AnyError> {
        self.0.up(ctx.get_mut()).await
    }

    async fn down(&mut self, ctx: &mut CtxRegistry) -> Result<(), AnyError> {
        self.0.down(ctx.get_mut()).await
    }
}

struct CtxRegistry(anymap::AnyMap);
impl CtxRegistry {
    fn get_mut<T: 'static>(&mut self) -> &mut T {
        self.0.get_mut().unwrap_or_else(|| {
            panic!(
                "Tried to use migration context of type {}, but it is not registered",
                std::any::type_name::<T>(),
            )
        })
    }
    fn insert<T: 'static>(&mut self, ctx: T) {
        self.0.insert(ctx).unwrap_or_else(|| {
            panic!(
                "Tried to register migration context of type {} second time",
                std::any::type_name::<T>(),
            )
        });
    }
}

pub struct MigrateCoreCtx {
    ctx_registry: CtxRegistry,
    migrations: Vec<NamedMigration>,
    state_lock: Box<dyn StateLock>,
}

struct NamedMigration {
    name: String,
    migration: Box<dyn Migration<Ctx = CtxRegistry>>,
}

impl<'a> MigrateCoreCtx {
    pub fn ctx<T: 'static>(&mut self, ctx: T) {
        self.ctx_registry.insert(ctx);
    }

    pub fn migration(&mut self, name: impl Into<String>, migration: impl Migration + 'static) {
        let name = name.into();

        self.migrations.push(NamedMigration {
            name: name.into(),
            migration: Box::new(TypeErasedMigration(migration)),
        });
    }

    // TODO: this accepts the state and the plan target
    // (execute all, rollback_to_the_specified, execute_to_the_specified, or whatever?)
    async fn _plan() {
        // TODO: make a plan command that prints diff to console
        // maybe it should return `MigrationPlan` struct which our `MigrateCoreCtx`
        // can accept (or plan will store a reference to it) to execute or rollback
    }

    #[instrument(skip(self))]
    pub async fn apply_migrations(&mut self) -> MigrateResult<()> {
        let state_guard = self
            .state_lock
            .lock()
            .await
            .map_err(|source| MigrateError::StateLock { source })?;
        let state_client = state_guard.client();

        let state = State::decode(
            &state_client
                .fetch()
                .await
                .map_err(|source| MigrateError::StateFetch { source })?,
        )?;

        let diff = diff::diff(&mut self.migrations, &state.applied_migrations)?;

        eprintln!("Applying migrations up...");

        for migration in diff.pending {
            migration
                .migration
                .up(&mut self.ctx_registry)
                .await
                // TODO: record the migration as `tainted` (this is concept taken from `terraform`) if it fails,
                // or handle it somehow else?
                .map_err(|source| MigrateError::MigrationScript { source })?;
        }

        state_guard
            .unlock()
            .await
            .map_err(|source| MigrateError::StateUnlock { source })?;

        Ok(())
    }

    pub async fn rollback_migrations() -> MigrateResult<()> {
        todo!()
    }
}

enum Plan {
    Up(Option<MigrationBound>),
    Down(Option<MigrationBound>),
}

enum MigrationBound {
    Inclusive(String),
    Exclusive(String),
}
