use async_trait::async_trait;
use std::error::Error;

pub type Result<T, E = Box<dyn Error>> = std::result::Result<T, E>;

/// State storage is basically an `Option<Vec<u8>>`.
/// The implementations of this trait should not make any assumptions about
/// the state shape (i.e. what the given `Ven<u8>` represents). The given
/// bytes are not even guaranteed to be valid UTF8.
#[async_trait(?Send)]
pub trait StateGuard {
    // FIXME: extract this trait to a separate crate which guarantees stability even
    // when there are breaking changes to other parts of `migrate-core`?

    // FIXME: when fetch or update fail, we don't call unlock()
    // this might be fine, the implementation should handle this,
    // send heartbeats to verify the lock is not poisonned, or is this invariant
    // too complicated for implementations to implement and we might help with
    // this somehow on our high-level end?

    async fn fetch(&self) -> Result<Vec<u8>>;
    async fn update(&mut self, state: Vec<u8>) -> Result<()>;

    async fn unlock(self: Box<Self>) -> Result<()>;
}

#[async_trait(?Send)]
pub trait StateLock {
    /// id string is guaranteed to be non-empty and contain only the symbols:
    /// ```not_rust
    /// /a-zA-Z0-9\-_/
    /// ```
    async fn lock<'l>(&'l mut self) -> Result<Box<dyn StateGuard + 'l>>;
}
