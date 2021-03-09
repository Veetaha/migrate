//! This crate guarantees stability even when there are breaking changes
//! to `migrate-core`. This is because it serves as an interface for the
//! considerable part of `migrate` ecosystem, namely for different state
//! storage and locks implementations. We would like to avoid updating
//! all of them, especially if they don't reside in our repository.

use async_trait::async_trait;
use std::error::Error;

pub type Result<T, E = Box<dyn Error + Send + Sync>> = std::result::Result<T, E>;

/// State storage is basically a [`Vec`]`<`[`u8`]`>`.
/// The implementations of this trait should not make any assumptions about
/// the state shape (i.e. what the given [`Vec`]`<`[`u8`]`>` represents). The given
/// bytes are not even guaranteed to be valid UTF8.
#[async_trait]
pub trait StateClient {
    // FIXME: when fetch or update fail, we don't call unlock()
    // this might be fine, the implementation should handle this,
    // send heartbeats to verify the lock is not poisonned, or is this invariant
    // too complicated for implementations to implement and we might help with
    // this somehow on our high-level end?

    /// Return the all the stored bytes in the storage.
    ///
    /// If the storage wasn't yet initialized with `update()` call previously
    /// then it should return `Ok(None)`, otherwise the value stored
    /// with the most recent `update()` call should be returned
    async fn fetch(&mut self) -> Result<Vec<u8>>;

    /// Stores the given bytes in the storage.
    ///
    /// It shouldn't make any assumptions about what these bytes represent,
    /// there are no guarantees about the byte pattern `migrate` uses to
    /// store the serialized migration state representation.
    ///
    /// For the first ever call to [`update()`](Self::update) it should
    /// initialize the storage with the given bytes, and if [`fetch()`](Self::fetch)
    /// was called before the intialization hapenned, then [`fetch()`](Self::fetch)
    /// should return `Ok(None)`.
    async fn update(&mut self, state: Vec<u8>) -> Result<()>;
}

#[async_trait]
pub trait StateLock {
    /// Acquire the exclusive lock to the migration state.
    ///
    /// Acquiring the exclusive lock means that no other subjects
    /// (threads, current and other remote compute instance's processes)
    /// can access the state. The future returned by this method should
    /// be resolved only once the lock is unlocked (via [`StateGuard::unlock()`]).
    /// This means that if some other subject is already holding a lock,
    /// we should wait for it to unlock it (by awaiting the returned future to resolve).
    ///
    /// The lock has to be held until we call [`StateGuard::unlock()`] on
    /// the returned [`StateGuard`] implementation.
    async fn lock(self: Box<Self>) -> Result<Box<dyn StateGuard>>;
}

#[async_trait]
pub trait StateGuard {
    /// Returns the [`StateClient`] to be used to access the migration state
    /// while this [`StateGuard`] hold the lock.
    fn client(&mut self) -> &mut dyn StateClient;

    /// Unlocks the currently held migration state lock allowing for
    /// other subjects to acquire it with [`StateLock::lock()`] once again
    async fn unlock(self: Box<Self>) -> Result<()>;
}

#[test]
fn object_safety() {
    fn _test(_: &dyn StateGuard, _: &dyn StateLock, _: &dyn StateClient) {}
}
