use crate::PlanBuildError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct MigrationMeta {
    pub(crate) name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct State {
    // TODO: handle corrupted migrations
    pub(crate) applied_migrations: Vec<MigrationMeta>,
}

impl State {
    pub(crate) fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }

    pub(crate) fn decode(bytes: &[u8]) -> Result<Self, PlanBuildError> {
        match serde_json::from_slice(bytes)
            .map_err(|err| PlanBuildError::StateCorruption(err.into()))?
        {
            StateRoot::V1(state) => Ok(state),
            // Once we have new versions of state we have to transform them
            // from v1 to v2, then from v2 to v3... until we end up with the latest
            // representation
        }
    }
}

/// The top-level migration state. It is simply the union type of all state
/// shapes that may have been stored. This is required to properly handle
/// migration states created by old versions of our library.
///
/// Once we make breaking changes to the state shape we have to copy,
/// and paste them here, creating a new version for the latest one.
///
/// As for now we have defined only a single version, thus we don't have code
/// for migrating migration states of old versions to newer ones. Let's see
/// how long this lasts...
#[derive(Serialize, Deserialize)]
enum StateRoot {
    V1(State),
}
