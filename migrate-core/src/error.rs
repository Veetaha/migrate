use std::fmt;

use itertools::Itertools;
use thiserror::Error;

use crate::dyn_migration::MigrationRunMode;

pub(crate) type AnyError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PlanBuildError {
    #[error(
        "provided migration scripts do not reflect \
        the applied migrations stack stored in the persistent state"
    )]
    #[non_exhaustive]
    InconsistentMigrationScripts,

    #[error("migration state is corrupted")]
    #[non_exhaustive]
    StateCorruption(#[source] AnyError),

    #[error("failed to acquire migration state lock")]
    #[non_exhaustive]
    StateLock(#[source] AnyError),

    #[error("failed to fetch migrations")]
    #[non_exhaustive]
    StateFetch(#[source] AnyError),

    #[error("unknown migration name specified: {name}, available migrations: [{}] ", available.join(","))]
    #[non_exhaustive]
    UnknownMigration {
        name: String,
        available: Vec<String>,
    },
}

#[derive(Debug)]
pub struct PlanExecError {
    pub(crate) errors: Vec<PlanExecErrorKind>,
}

impl fmt::Display for PlanExecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.errors.iter().format(", "))
    }
}

impl std::error::Error for PlanExecError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.errors[0])
    }
}

#[derive(Debug, Error)]
pub(crate) enum PlanExecErrorKind {
    #[error("migration script failed")]
    ExecMigrationScript(#[source] AnyError),

    #[error("failed to release migration state lock")]
    UnlockState(#[source] AnyError),

    #[error("failed to update the migration state")]
    UpdateState(#[source] AnyError),

    #[error("provider failed to create migration context of type {ctx_type} in run mode: {:?}")]
    CreateMigrationCtx {
        source: AnyError,
        run_mode: MigrationRunMode,
        ctx_type: &'static str,
    },

    // This is a recoverable error that is handled within our code itself
    // it is added to this enum just for simplicity and less code
    #[error("no-commit mode is not supported by the migration context provider")]
    CtxLacksNoCommitMode,
}
