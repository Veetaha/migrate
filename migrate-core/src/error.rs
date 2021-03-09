use std::fmt;

use itertools::Itertools;
use thiserror::Error;

use crate::dyn_migration::MigrationRunMode;

pub(crate) type DynError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PlanBuildError {
    #[error(
        "provided migration scripts do not reflect the applied migrations stack \
        stored in the persistent state storage"
    )]
    #[non_exhaustive]
    InconsistentMigrationScripts,

    #[error(
        "failed to decode the migration state (maybe it is corrupted?), read state: {}",
        String::from_utf8(read_state.clone()).unwrap_or_else(|it| format!("{:?}", it.into_bytes()))
    )]
    #[non_exhaustive]
    StateDecode {
        read_state: Vec<u8>,
        source: DynError
    },

    #[error("failed to acquire migration state lock")]
    #[non_exhaustive]
    StateLock(#[source] DynError),

    #[error("failed to fetch migrations")]
    #[non_exhaustive]
    StateFetch(#[source] DynError),

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
        write!(f, "failed to execute the migration plan")?;
        let additional_errors = &self.errors[1..];
        if !additional_errors.is_empty() {
            write!(f, " Additional errors: {}", additional_errors.iter().format(", "))?;
        }
        Ok(())
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
    ExecMigrationScript(#[source] DynError),

    #[error("failed to release migration state lock")]
    UnlockState(#[source] DynError),

    #[error("failed to update the migration state")]
    UpdateState(#[source] DynError),

    #[error("provider failed to create migration context of type {ctx_type} in run mode: {:?}")]
    CreateMigrationCtx {
        source: DynError,
        run_mode: MigrationRunMode,
        ctx_type: &'static str,
    },

    // This is a recoverable error that is handled within our code itself
    // it is added to this enum just for simplicity and less code
    #[error("no-commit mode is not supported by the migration context provider")]
    CtxLacksNoCommitMode,
}
