use advisory_lock::{AdvisoryFileLock, FileLockMode};
use async_trait::async_trait;
use fs::File;
use fs_err as fs;
use migrate_state::{Result, StateClient, StateGuard, StateLock};
use std::{
    io::{self, Read, Seek, Write},
    marker::PhantomData,
    path::PathBuf,
};

pub struct FileStateLock {
    state_file: PathBuf,
}

impl FileStateLock {
    /// File
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

        Ok(Box::new(FileStateGuard(client, PhantomData)))
    }
}

struct FileStateGuard(FileStateClient, PhantomData<&'static mut ()>);

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

// FIXME: the operations here are blocking
#[async_trait]
impl StateClient for FileStateClient {
    async fn fetch(&mut self) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        // FIXME: make this calls non-blocking
        self.file
            .read_to_end(&mut buf)
            .map_err(|source| FileStateError::Read { source })?;

        Ok(buf)
    }

    async fn update(&mut self, state: Vec<u8>) -> Result<()> {
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

    #[error("failed to lock  the migration state file")]
    Lock {
        source: advisory_lock::FileLockError,
    },
}

#[test]
fn impl_sync() {
    fn assert_is_sync<T: Sync>() {}
    assert_is_sync::<FileStateGuard>();
}
