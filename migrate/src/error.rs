use migrate_core::{PlanBuildError, PlanExecError};

pub(crate) type DynError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum MigrateRunError {
    #[error("failed to build the migration plan")]
    #[non_exhaustive]
    PlanBuild(#[from] PlanBuildError),

    #[error("failed to execute the migration plan")]
    #[non_exhaustive]
    PlanExec(#[from] PlanExecError),
}
