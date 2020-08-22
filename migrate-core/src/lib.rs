#![warn(unreachable_pub)]

mod diff;
mod error;
mod state;

pub use error::*;

use async_trait::async_trait;
use diff::MigrationsDiff;
use itertools::{EitherOrBoth, Itertools};
use migrate_state::StateLock;
use state::{MigrationMeta, State};
use tracing::{info, instrument};

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
            .map_err(MigrateError::StateLock)?;

        let state = State::decode(
            &state_guard
                .fetch()
                .await
                .map_err(MigrateError::StateFetch)?,
        )?;

        Ok(())
    }

    pub async fn rollback_migrations() -> MigrateResult<()> {
        todo!()
    }
}

// /**
//  * Adapter determines where executed migration names will be stored and what will be
//  * passed to `migrate` and `rollback` function as a parameter.
//  */
//  export interface Adapter<P = unknown> {
//     /**
//      * Returns the client that is passed as a parameter to `migrate()` and `rollback()`.
//      */
//     connect(): Promise<P>;

//     /**
//      * Releases the resources (if any) allocated by the adapter internally.
//      */
//     disconnect(): Promise<void>;

//     /**
//      * Returns an absolute path to the template that is used by `east create <migration-name>`
//      * If adapter supports multiple languages it should check for the extension
//      * name and return the path to the appropriate template for the given
//      * file extension, otherwise an error should be thrown.
//      *
//      * @param sourceMigrationExtension defines the file extension for the created
//      * migration without the leading dot (e.g. 'js', 'ts', etc.)
//      */
//     getTemplatePath(sourceMigrationExtension: string): string;

//     /**
//      * Returns the entire list of all executed migration names.
//      */
//     getExecutedMigrationNames(): Promise<string[]>;

//     /**
//      * Marks the migration under `migrationName` as executed in the backing migration state storage.
//      */
//     markExecuted(migrationName: string): Promise<void>;

//     /**
//      * Unmarks migration under `migrationName` as executed in the backing migration state storage.
//      */
//     unmarkExecuted(migrationName: string): Promise<void>;
// }
