use crate::{DynError, Migration, PlanExecErrorKind};
use async_trait::async_trait;
use std::{any, fmt};

/// Gives methods for creating the context for the migration.
/// This should most likely create a database client, or initialize some
/// state, for example ensure an executable is installed.
#[async_trait]
pub trait MigrationCtxProvider: Send + 'static {
    /// The type of that this provider creates.
    /// There must be only one provider for the given type, because the Rust
    /// type id will be used as a key to lookup the context for the migration.
    type Ctx: Send + 'static;

    /// Create the context for real migration. All the changes that will be made
    /// to the target migration object should be applied for real.
    async fn create_in_commit_mode(self: Box<Self>) -> Result<Self::Ctx, DynError>;

    /// Create the context for no-commit (or dry-run) migration. All the changes that will be made
    /// to the target migration object should not be applied for real.
    ///
    /// The no-commit migration context will most likely just log what would be
    /// executed when the migration runs for real.
    async fn create_in_no_commit_mode(self: Box<Self>) -> Option<Result<Self::Ctx, DynError>>;
}

pub(crate) struct DynMigration {
    pub(crate) name: String,
    pub(crate) script: Box<dyn DynMigrationScript>,
}

impl DynMigration {
    pub(crate) fn new(name: String, migration: impl Migration + 'static) -> DynMigration {
        Self {
            name,
            script: Box::new(migration),
        }
    }
}

impl fmt::Debug for DynMigration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { name, script: _ } = self;

        f.debug_struct("DynMigration")
            .field("name", name)
            .field("script", &"Box<dyn MigrationScript>")
            .finish()
    }
}

/// Behavioral toggle for the migration execution
#[derive(Debug, Copy, Clone)]
pub enum MigrationRunMode {
    /// Commit changes to the migration target while executing the migration
    Commit,
    /// Don't commit any changes to the migration target, just debug or trace
    /// all the operations that are performed using some internal mock setup
    NoCommit,
}

pub(crate) enum MigrationDirection {
    Up,
    Down,
}

impl fmt::Display for MigrationDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MigrationDirection::Up => f.write_str("up"),
            MigrationDirection::Down => f.write_str("down"),
        }
    }
}

pub(crate) struct DynMigrationScriptCtx<'reg> {
    pub(crate) ctx_registry: &'reg mut CtxRegistry,
    pub(crate) run_mode: MigrationRunMode,
    pub(crate) direction: MigrationDirection,
}

/// This wrapper is used to erase the type of the inner migration context.
/// It uses `CtxRegistry` as it's own context to query and forward the
/// required context value dynamically at runtime.
#[async_trait]
pub(crate) trait DynMigrationScript {
    async fn exec(&mut self, ctx: &mut DynMigrationScriptCtx<'_>) -> Result<(), PlanExecErrorKind>;
}

#[async_trait]
impl<Mig: Migration> DynMigrationScript for Mig {
    async fn exec(&mut self, ctx: &mut DynMigrationScriptCtx<'_>) -> Result<(), PlanExecErrorKind> {
        let migration_ctx = ctx.ctx_registry.get_mut(ctx.run_mode).await?;
        let result = match ctx.direction {
            MigrationDirection::Up => self.up(migration_ctx).await,
            MigrationDirection::Down => self.down(migration_ctx).await,
        };
        result.map_err(PlanExecErrorKind::ExecMigrationScript)
    }
}

enum CtxRegistryEntry<Ctx> {
    // Option is required to consume the box during context initialization.
    Uninit(Option<Box<dyn MigrationCtxProvider<Ctx = Ctx>>>),
    Init(Ctx),
    CtxLacksNoCommitMode,
}

impl<Ctx> CtxRegistryEntry<Ctx> {
    fn set_init(&mut self, ctx: Ctx) -> &mut Ctx {
        *self = Self::Init(ctx);
        match self {
            Self::Init(it) => it,
            _ => unreachable!("BUG: we've set the enum to `Init` variant!"),
        }
    }
}

/// Thin wrapper over `anymap` that allows for storing heterogeneous
/// types and basically provides migration context dependency injection
/// with the type as a DI token (key).
pub(crate) struct CtxRegistry(anymap::Map<dyn anymap::any::Any + Send>);

impl CtxRegistry {
    pub(crate) fn new() -> Self {
        Self(anymap::Map::new())
    }

    async fn get_mut<Ctx: Send + 'static>(
        &mut self,
        run_mode: MigrationRunMode,
    ) -> Result<&mut Ctx, PlanExecErrorKind> {
        let entry: &mut CtxRegistryEntry<Ctx> = self.0.get_mut().unwrap_or_else(|| {
            panic!(
                "Tried to use migration context of type {}, but no provider for it is registered",
                any::type_name::<Ctx>(),
            )
        });

        let provider = match entry {
            CtxRegistryEntry::Init(ctx) => return Ok(ctx),
            CtxRegistryEntry::CtxLacksNoCommitMode => {
                return Err(PlanExecErrorKind::CtxLacksNoCommitMode)
            }
            CtxRegistryEntry::Uninit(provider) => provider,
        };

        let provider = provider.take().expect(
            "BUG: this method should not be called after the provider \
            has failed to create the context",
        );

        let result = match run_mode {
            MigrationRunMode::Commit => provider.create_in_commit_mode().await,
            MigrationRunMode::NoCommit => {
                provider.create_in_no_commit_mode().await.ok_or_else(|| {
                    *entry = CtxRegistryEntry::CtxLacksNoCommitMode;
                    PlanExecErrorKind::CtxLacksNoCommitMode
                })?
            }
        };

        let ctx = result.map_err(|source| PlanExecErrorKind::CreateMigrationCtx {
            source,
            run_mode,
            ctx_type: any::type_name::<Ctx>(),
        })?;

        Ok(entry.set_init(ctx))
    }

    pub(crate) fn insert<P: MigrationCtxProvider>(&mut self, provider: P) {
        let prev_ctx = self
            .0
            .insert(CtxRegistryEntry::Uninit(Some(Box::new(provider))));
        if prev_ctx.is_some() {
            panic!(
                "Tried to register a provider for migration context of type `{}` second time",
                any::type_name::<P::Ctx>(),
            )
        }
    }
}
