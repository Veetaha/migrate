use migrate_core::{PlanBuildError, PlanExecError};
use thiserror::Error;

pub(crate) type DynError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Error)]
#[error(transparent)]
pub struct Error {
    #[from]
    source: ErrorKind,
}

#[derive(Debug, Error)]
pub(crate) enum ErrorKind {
    #[error("failed to build the migration plan")]
    PlanBuild(#[source] PlanBuildError),

    #[error("failed to execute the migration plan")]
    PlanExec(#[source] PlanExecError),
}
