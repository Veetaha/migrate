use advisory_lock::{AdvisoryFileLock, FileLockMode};
use anyhow::Context;
use async_trait::async_trait;
use migrate_state::{Result, StateClient, StateGuard, StateLock};
use std::{
    io::{Read, Write},
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
        let mut file = AdvisoryFileLock::new(&self.state_file, FileLockMode::Exclusive)?;

        // TODO: make this non-blocking (probably contribute async support upstream?)
        file.lock()?;

        let client = FileStateClient {
            path: self.state_file,
            file,
        };

        Ok(Box::new(FileStateGuard(client, PhantomData)))
    }
}

#[test]
fn not_sync() {
    fn assert_is_sync<T: Sync>() {}
    assert_is_sync::<FileStateGuard>();
}

struct FileStateGuard(FileStateClient, PhantomData<&'static mut ()>);

#[async_trait]
impl StateGuard for FileStateGuard {
    fn client(&mut self) -> &mut dyn StateClient {
        &mut self.0
    }

    async fn unlock(mut self: Box<Self>) -> Result<()> {
        self.0.file.unlock()?;
        Ok(())
    }
}

struct FileStateClient {
    path: PathBuf,
    // Mutex is required, because `read` operations on `io::Read` require `&mut self`
    file: AdvisoryFileLock,
}

// FIXME: the operations here are blocking
#[async_trait]
impl StateClient for FileStateClient {
    async fn fetch(&mut self) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        self.file
            .read_to_end(&mut buf)
            .with_context(|| format!("failed to read state file {}", self.path.display()))?;
        Ok(buf)
    }

    async fn update(&mut self, state: Vec<u8>) -> Result<()> {
        self.file
            .write_all(&state)
            .with_context(|| format!("failed to update state file {}", self.path.display()))?;
        Ok(())
    }
}
