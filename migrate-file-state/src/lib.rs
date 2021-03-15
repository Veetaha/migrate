//! Implementation of storing the migration state in a file on the local file system.
//!
//! See [`FileStateLock`] docs for more details.
#![warn(missing_docs, unreachable_pub, rust_2018_idioms)]
// Makes rustc abort compilation if there are any unsafe blocks in the crate.
// Presence of this annotation is picked up by tools such as cargo-geiger
// and lets them ensure that there is indeed no unsafe code as opposed to
// something they couldn't detect (e.g. unsafe added via macro expansion, etc).
#![forbid(unsafe_code)]

use advisory_lock::{AdvisoryFileLock, FileLockMode};
use async_trait::async_trait;
use fs::File;
use fs_err as fs;
use migrate_state::{Result, StateClient, StateGuard, StateLock};
use std::{
    io::{self, Read, Seek, Write},
    path::PathBuf,
};

/// Implements [`StateLock`] storing the migration state in a file on the local
/// file system. It uses operating system [advisory file locks][advisory-lock]
/// to support state locking.
///
/// Pass the file path in [`FileStateLock::new()`] method. The default conventional
/// file name is `migration-state`. Beware that the format of this file is private,
/// so you shouldn't make any assumptions about it being `json`, `yaml`, `toml`
/// or anything else even UTF-8 encoded.
///
/// Example usage:
///
/// ```no_run
/// use migrate_file_state::FileStateLock;
/// use migrate_core::Plan;
///
/// let state_lock = FileStateLock::new("./migration-state");
///
/// let plan = Plan::builder(state_lock);
/// ```
///
/// [advisory-lock]: https://docs.rs/advisory-lock
pub struct FileStateLock {
    state_file: PathBuf,
}

impl FileStateLock {
    /// Creates migration state file storage. Accepts the file path to the migration
    /// state file.
    ///
    /// If the file at the given path doesn't exist, then the state is considered
    /// uninitialized and a new file will be created once it is updated with the
    /// new state info.
    ///
    /// The default conventional name of the file is `migration-state`
    ///
    /// See [`FileStateLock`] struct docs for more details
    pub fn new(state_file_path: impl Into<PathBuf>) -> Self {
        Self {
            state_file: state_file_path.into(),
        }
    }
}

#[async_trait]
impl StateLock for FileStateLock {
    async fn lock(self: Box<Self>) -> Result<Box<dyn StateGuard>> {
        let file = tokio::task::spawn_blocking(move || {
            fs::OpenOptions::new()
                .read(true)
                .create(true)
                .write(true)
                .open(self.state_file)
                .map_err(|source| FileStateError::Open { source })
        })
        .await
        .expect("The task of creating the file has panicked")?;

        let file = tokio::task::spawn_blocking(move || {
            file.file()
                .lock(FileLockMode::Exclusive)
                .map_err(|source| FileStateError::Lock { source })
                .map(|()| file)
        })
        .await
        .expect("The task of locking the file has panicked")?;

        let client = FileStateClient { file };

        Ok(Box::new(FileStateGuard(client)))
    }
}

struct FileStateGuard(FileStateClient);

#[async_trait]
impl StateGuard for FileStateGuard {
    fn client(&mut self) -> &mut dyn StateClient {
        &mut self.0
    }

    async fn unlock(mut self: Box<Self>) -> Result<()> {
        tokio::task::spawn_blocking(move || (*self).0.file.file().unlock())
            .await
            .expect("The task of unlocking the file has panicked")?;

        Ok(())
    }
}

struct FileStateClient {
    file: File,
}

impl FileStateClient {
    fn seek_start(&mut self) -> Result<()> {
        self.file
            .seek(io::SeekFrom::Start(0))
            .map_err(|source| FileStateError::Seek { source })?;
        Ok(())
    }
}

// FIXME: the operations here are blocking
#[async_trait]
impl StateClient for FileStateClient {
    async fn fetch(&mut self) -> Result<Vec<u8>> {
        self.seek_start()?;

        let mut buf = Vec::new();
        // FIXME: make this calls non-blocking
        self.file
            .read_to_end(&mut buf)
            .map_err(|source| FileStateError::Read { source })?;

        Ok(buf)
    }

    async fn update(&mut self, state: Vec<u8>) -> Result<()> {
        self.seek_start()?;

        // FIXME: make the calls non-blocking

        self.file
            .seek(io::SeekFrom::Start(0))
            .map_err(|source| FileStateError::Seek { source })?;

        self.file
            .set_len(0)
            .map_err(|source| FileStateError::Truncate { source })?;

        self.file
            .write_all(&state)
            .map_err(|source| FileStateError::Update { source })?;

        Ok(())
    }
}

#[derive(Debug, thiserror::Error)]
enum FileStateError {
    #[error("failed to open the migration state file")]
    Open { source: io::Error },

    #[error("failed to read the migration state file")]
    Read { source: io::Error },

    #[error("failed to set the cursor to the beginning of the state file")]
    Seek { source: io::Error },

    #[error("failed to truncate the migration state file")]
    Truncate { source: io::Error },

    #[error("failed to update the migration state file")]
    Update { source: io::Error },

    #[error("failed to lock the migration state file")]
    Lock {
        source: advisory_lock::FileLockError,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    struct StateFileGuard(std::path::PathBuf);
    impl Drop for StateFileGuard {
        fn drop(&mut self) {
            if let Err(err) = std::fs::remove_file(&self.0) {
                eprintln!("Failed to remove state file created in a test: {}", err);
            }
        }
    }

    #[tokio::test]
    async fn run_all() {
        let mut test_id = 0;
        let mut guards = vec![];

        migrate_state_test::run_all(|| {
            let file_state = env::temp_dir().join(format!("file-state-smoke-test-{}", test_id));
            test_id += 1;
            guards.push(StateFileGuard(file_state.clone()));

            move || Box::new(FileStateLock::new(file_state.clone()))
        })
        .await;
    }
}
