use std::fmt;

pub type MigrateResult<T> = std::result::Result<T, MigrateError>;
pub(crate) type AnyError = Box<dyn std::error::Error>;

#[derive(Debug)]
pub enum MigrateError {
    InconsistentMigrationScripts,
    MigrationScript(AnyError),
    StateCorruption(AnyError),
    StateLock(AnyError),
    StateFetch(AnyError),
    StateUnlock(AnyError),
}

impl std::error::Error for MigrateError {}

impl fmt::Display for MigrateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MigrateError::InconsistentMigrationScripts => f.write_str(
                "Provided migration scripts do not reflect \
                    the applied migrations stack stored in the persistent state",
            ),
            MigrateError::MigrationScript(err) => write!(f, "Migration script failed: {}", err),
            MigrateError::StateCorruption(err) => {
                write!(f, "Migration state is corrupted: {}", err)
            }
            MigrateError::StateLock(err) => {
                write!(f, "Failed to acquire migration state lock: {}", err)
            }
            MigrateError::StateFetch(err) => write!(f, "Failed to fetch migrations: {}", err),
            MigrateError::StateUnlock(err) => {
                write!(f, "Failed to release migration state lock: {}", err)
            }
        }
    }
}
