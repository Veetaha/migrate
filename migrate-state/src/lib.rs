//! Traits related to migration state storage.
//!
//! They are separate from [`migrate-core`] crate to guarantee more stability,
//! even when there are breaking changes to [`migrate-core`] crate.
//! This is because it serves as an interface for the considerable part of
//! [`migrate`] ecosystem, namely for different state storage and locks
//! implementations. We would like to avoid updating all of them, especially
//! if they don't reside in our repository.
//!
//! [`migrate`]: https://docs.rs/migrate
//! [`migrate-core`]: https://docs.rs/migrate-core
#![warn(missing_docs, unreachable_pub, rust_2018_idioms)]
// Makes rustc abort compilation if there are any unsafe blocks in the crate.
// Presence of this annotation is picked up by tools such as cargo-geiger
// and lets them ensure that there is indeed no unsafe code as opposed to
// something they couldn't detect (e.g. unsafe added via macro expansion, etc).
#![forbid(unsafe_code)]

use async_trait::async_trait;
use std::error::Error;

/// Type alias for the [`std::result::Result`] type used in the traits
pub type Result<T, E = Box<dyn Error + Send + Sync>> = std::result::Result<T, E>;

/// Client for the migration state storage.
///
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

    /// Return all the stored bytes in the storage.
    ///
    /// If the storage wasn't yet initialized with `update()` call previously
    /// then it should return `Ok(vec![])` (empty vector), otherwise the value
    /// stored with the most recent `update()` call should be returned
    async fn fetch(&mut self) -> Result<Vec<u8>>;

    /// Puts the given bytes into the storage.
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

/// The lock over a migration state storage.
///
/// It guards the underlying migration state preventing concurrent access
/// from multiple threads and processes. Ideally, this should be a distributed
/// lock implementation.
///
/// The main method of this trait is [`StateLock::lock()`], see its docs for more
/// details.
#[async_trait]
pub trait StateLock {
    /// # General concept
    ///
    /// Acquires the exclusive lock to the migration state.
    ///
    /// Acquiring the exclusive lock means that no other subjects
    /// (threads, current and other remote compute instance's processes)
    /// can access the state. The future returned by this method should
    /// be resolved only once the lock is unlocked (via [`StateGuard::unlock()`])
    /// if it is currently locked, or resolve right away if no other subject is
    /// holding the lock.
    ///
    /// This means that if some other subject is already holding a lock,
    /// we should wait for it to unlock it (by awaiting the returned future to resolve).
    ///
    /// The lock has to be held until a call to [`StateGuard::unlock()`] on
    /// the returned [`StateGuard`] implementation.
    ///
    /// The described behavior is expected when the `force` parameter is [`false`]
    ///
    /// # Boolean `force` parameter
    ///
    /// When the `force` boolean parameter is set to [`true`], the method must
    /// acquire the exclusive lock even if it currently acquired by some other subject
    /// and provide the access to the unrelying storage for the state regardless.
    ///
    /// This operation is dangerous, because it bypasses the locking mechanism
    /// which may lead to concurrent state storage mutations.
    /// It exists to help circumvent the situations where some subject has
    /// died without unlocking the lock, thus leaving it locked potentially
    /// forver.
    async fn lock(self: Box<Self>, force: bool) -> Result<Box<dyn StateGuard>>;
}

/// Object returned from [`StateLock::lock()`] that while alive
/// holds the lock on the state storage preventing concurrent access to it
/// from multiple threads and processes.
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
