//! TODO: crate-level docs

#![warn(missing_docs, unreachable_pub, rust_2018_idioms)]
// Makes rustc abort compilation if there are any unsafe blocks in the crate.
// Presence of this annotation is picked up by tools such as cargo-geiger
// and lets them ensure that there is indeed no unsafe code as opposed to
// something they couldn't detect (e.g. unsafe added via macro expansion, etc).
#![forbid(unsafe_code)]

use futures::prelude::*;
use migrate_state::StateLock;
use std::time;

const STATE_LOCK_MIN_DURATION: time::Duration = time::Duration::from_secs(1);
const TEST_TIMEOUT: time::Duration = time::Duration::from_secs(30);

async fn expect_within_timeout<F: Future>(fut: F) -> F::Output {
    futures::select! {
        _ = tokio::time::sleep(TEST_TIMEOUT).fuse() => {
            panic!("Timed out ({:?}) waiting for the future to resolve", TEST_TIMEOUT)
        }
        res = fut.fuse() => res,
    }
}

/// Run all the available tests for the given state storage implementation
pub async fn run_all<F>(mut create_state_lock_factory: impl FnMut() -> F)
where
    F: Fn() -> Box<dyn StateLock>,
{
    let factories = (create_state_lock_factory(), create_state_lock_factory());

    futures::join!(smoke_test(factories.0()), locking(&factories.1),);
}

/// Run a simple smoke test that fetches the initial state,
/// updates it and fetches it one more time.
pub async fn smoke_test(state_lock: Box<dyn StateLock>) {
    let mut state = expect_within_timeout(state_lock.lock()).await.unwrap();
    let client = state.client();

    let initial_state = client.fetch().await.unwrap();
    assert_eq!(initial_state, vec![]);

    let new_state = vec![42];
    client.update(new_state.clone()).await.unwrap();
    let saved_state = client.fetch().await.unwrap();

    assert_eq!(saved_state, new_state);

    // FIXME: ensure unlock is always called (even if unwrap panics)
    state.unlock().await.unwrap();
}

/// Test the correctness of state locking implementation
pub async fn locking(create_state_lock: &dyn Fn() -> Box<dyn StateLock>) {
    let lock_state = || expect_within_timeout(create_state_lock().lock());

    // While someone already holds the lock, the second lock should not resolve

    let lock = lock_state().await.unwrap();
    // Wait for some time to check that the second lock is not resolved while
    // we already hold an existing lock
    futures::select! {
        _ = tokio::time::sleep(STATE_LOCK_MIN_DURATION).fuse() => {}
        state = lock_state().fuse() => {
            let state = match state {
                Ok(_) => "<resolved state lock>".to_owned(),
                Err(err) => format!("{:?}", err),
            };
            panic!("Unexpected resolution of the state lock future: {}", state);
        }
    }
    lock.unlock().await.unwrap();

    // Once all the locks were unlocked, acquiring the new one should succeed
    lock_state().await.unwrap().unlock().await.unwrap();
}
