use crate::{DynError, Migration, PlanExecErrorKind};
use async_trait::async_trait;
use std::{any, fmt};

#[async_trait]
pub trait MigrationCtxProvider: Send + 'static {
    type Ctx: Send + 'static;

    async fn create_in_commit_mode(self: Box<Self>) -> Result<Self::Ctx, DynError>;
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
        let prev_ctx = self.0.insert(CtxRegistryEntry::Uninit(Some(Box::new(provider))));
        if let Some(_) = prev_ctx {
            panic!(
                "Tried to register a provider for migration context of type `{}` second time",
                any::type_name::<P::Ctx>(),
            )
        }
    }
}
