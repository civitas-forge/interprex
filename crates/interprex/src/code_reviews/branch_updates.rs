use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::CommitRange;
use crate::ProviderError;

/// Whether an applicable provider rule requires a change request's head to
/// contain the current target-branch tip.
///
/// This fact is independent of merge conflicts. A provider can require an
/// update for a change request that it otherwise reports as mergeable.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchUpdateRequirement {
    Required,
    NotRequired,
}

/// Whether the observed head contains the observed target-branch tip.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchFreshness {
    Current,
    /// The head does not contain the observed target-branch tip. The commits
    /// can be direct ancestors or can have diverged.
    Behind,
}

/// The provider's branch-update facts for one change request observation.
///
/// `commit_range` states the exact target and head revisions used to compute
/// `freshness`. A later read can differ. Callers pass the observed
/// `commit_range.head_sha` to the provider's update operation when they decide
/// an update is appropriate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BranchUpdateObservation {
    pub commit_range: CommitRange,
    pub requirement: BranchUpdateRequirement,
    pub freshness: BranchFreshness,
}

/// Failure to apply an exact-head branch update.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum BranchUpdateError {
    /// The change request no longer has the head named by the caller.
    #[error(
        "change request head changed: expected {expected_head_sha}, observed {observed_head_sha}"
    )]
    StaleHead {
        expected_head_sha: String,
        observed_head_sha: String,
    },
    /// A provider credential, lookup, validation, transport or refusal error.
    #[error(transparent)]
    Provider(#[from] ProviderError),
}

impl BranchUpdateObservation {
    /// Whether the applicable rule requires the observed head to be updated.
    ///
    /// This combines provider facts only. The caller still decides whether
    /// and when to request an update.
    #[must_use]
    pub fn update_required(&self) -> bool {
        self.requirement == BranchUpdateRequirement::Required
            && self.freshness == BranchFreshness::Behind
    }
}
