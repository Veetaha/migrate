use thiserror::Error;

pub type MigrateResult<T> = std::result::Result<T, MigrateError>;
pub(crate) type AnyError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MigrateError {
    #[error(
        "provided migration scripts do not reflect \
        the applied migrations stack stored in the persistent state"
    )]
    #[non_exhaustive]
    InconsistentMigrationScripts,

    #[error("migration script failed")]
    #[non_exhaustive]
    MigrationScript { source: AnyError },

    #[error("migration state is corrupted")]
    #[non_exhaustive]
    StateCorruption { source: AnyError },

    #[error("failed to acquire migration state lock")]
    #[non_exhaustive]
    StateLock { source: AnyError },

    #[error("failed to fetch migrations")]
    #[non_exhaustive]
    StateFetch { source: AnyError },

    #[error("failed to release migration state lock")]
    #[non_exhaustive]
    StateUnlock { source: AnyError },
}
