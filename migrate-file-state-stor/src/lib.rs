use advisory_lock::{AdvisoryFileLock, FileLockMode};
use anyhow::Context;
use async_trait::async_trait;
use migrate_state::{Result, StateGuard, StateLock};
use std::{
    cell::RefCell,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub struct FileStateStorLock {
    state_file: PathBuf,
}

impl FileStateStorLock {
    pub fn new(state_file_path: impl Into<PathBuf>) -> Self {
        Self {
            state_file: state_file_path.into(),
        }
    }
}

#[async_trait(?Send)]
impl StateLock for FileStateStorLock {
    async fn lock<'l>(&'l mut self) -> Result<Box<dyn StateGuard + 'l>> {
        let mut file = AdvisoryFileLock::new(&self.state_file, FileLockMode::Exclusive)?;
        file.lock()?;

        Ok(Box::new(FileStateGuard {
            path: &self.state_file,
            file: RefCell::new(file),
        }))
    }
}

struct FileStateGuard<'p> {
    path: &'p Path,
    file: RefCell<AdvisoryFileLock>,
}

#[async_trait(?Send)]
impl StateGuard for FileStateGuard<'_> {
    async fn fetch(&self) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        self.file
            .borrow_mut()
            .read_to_end(&mut buf)
            .with_context(|| format!("failed to read state file {}", self.path.display()))?;
        Ok(buf)
    }

    async fn update(&mut self, state: Vec<u8>) -> Result<()> {
        self.file
            .get_mut()
            .write_all(&state)
            .with_context(|| format!("failed to update state file {}", self.path.display()))?;
        Ok(())
    }

    async fn unlock(mut self: Box<Self>) -> Result<()> {
        self.file.get_mut().unlock()?;
        Ok(())
    }
}
