//! This crate guarantees stability even when there are breaking changes
//! to `migrate-core`. This is because it serves as an interface for the
//! considerable part of `migrate` ecosystem, namely for different state
//! storage and locks implementations. We would like to avoid updating
//! all of them, especially if they don't reside in our repository.

use async_trait::async_trait;
use std::error::Error;

pub type Result<T, E = Box<dyn Error + Send + Sync>> = std::result::Result<T, E>;

/// State storage is basically an `Option<Vec<u8>>`.
/// The implementations of this trait should not make any assumptions about
/// the state shape (i.e. what the given `Ven<u8>` represents). The given
/// bytes are not even guaranteed to be valid UTF8.
#[async_trait]
pub trait StateClient {
    // FIXME: when fetch or update fail, we don't call unlock()
    // this might be fine, the implementation should handle this,
    // send heartbeats to verify the lock is not poisonned, or is this invariant
    // too complicated for implementations to implement and we might help with
    // this somehow on our high-level end?
    async fn fetch(&mut self) -> Result<Vec<u8>>;
    async fn update(&mut self, state: Vec<u8>) -> Result<()>;
}

#[async_trait]
pub trait StateLock {
    async fn lock(self: Box<Self>) -> Result<Box<dyn StateGuard>>;
}

#[async_trait]
pub trait StateGuard {
    fn client(&mut self) -> &mut dyn StateClient;
    async fn unlock(self: Box<Self>) -> Result<()>;
}

#[test]
fn assert_object_safe() {
    fn _test(_: &dyn StateGuard, _: &dyn StateLock, _: &dyn StateClient) {}
}
