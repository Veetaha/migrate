use crate::dyn_migration::MigrationRunMode;
use itertools::Itertools;
use std::fmt;
use thiserror::Error;

pub(crate) type DynError = Box<dyn std::error::Error + Send + Sync>;

/// Error returned as a result of [`PlanBuilder::build()`](crate::PlanBuilder::build)
#[derive(Debug, Error)]
#[error(transparent)]
pub struct PlanBuildError {
    #[from]
    source: PlanBuildErrorKind,
}

#[derive(Debug, Error)]
pub(crate) enum PlanBuildErrorKind {
    #[error(
        "provided migration scripts do not reflect the applied migrations stack \
        stored in the persistent state storage"
    )]
    InconsistentMigrationScripts,

    #[error(
        "failed to decode the migration state (maybe it is corrupted?), read state: {}",
        String::from_utf8(read_state.clone()).unwrap_or_else(|it| format!("{:?}", it.into_bytes()))
    )]
    StateDecode {
        read_state: Vec<u8>,
        source: DynError,
    },

    #[error("failed to acquire migration state lock")]
    StateLock(#[source] DynError),

    #[error("failed to fetch migrations")]
    StateFetch(#[source] DynError),

    #[error("unknown migration name specified: {name}, available migrations: [{}] ", available.join(","))]
    UnknownMigration {
        name: String,
        available: Vec<String>,
    },
}

/// Error returned as a result of [`Plan::exec()`](crate::Plan::exec)
#[derive(Debug)]
pub struct PlanExecError {
    pub(crate) errors: Vec<PlanExecErrorKind>,
}

impl fmt::Display for PlanExecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to execute the migration plan")?;
        let additional_errors = &self.errors[1..];
        if !additional_errors.is_empty() {
            write!(
                f,
                " Additional errors: {}",
                additional_errors.iter().format(", ")
            )?;
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
