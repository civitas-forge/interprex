use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{OpenClosed, Repository, Result};

platform_number!(PullRequestNumber);

/// Opaque provider identity for a review thread.
///
/// Consumers retain this value only to address a thread returned by the same
/// provider. Its representation has no provider-neutral meaning.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct ReviewThreadId(String);

impl ReviewThreadId {
    pub fn new(value: impl Into<String>) -> std::result::Result<Self, crate::ModelError> {
        let value = value.into();
        if value.is_empty() {
            return Err(crate::ModelError::Empty {
                field: "review thread id",
            });
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PullRequest {
    pub number: PullRequestNumber,
    pub title: String,
    pub state: OpenClosed,
    pub draft: bool,
    pub head_sha: String,
    pub base_sha: String,
    pub author: String,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewComment {
    pub body: String,
    pub author: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewThread {
    pub id: ReviewThreadId,
    pub resolved: bool,
    pub path: Option<String>,
    pub line: Option<u64>,
    pub comments: Vec<ReviewComment>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckConclusion {
    Success,
    Failure,
    Neutral,
    Cancelled,
    TimedOut,
    ActionRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckOutcome {
    pub name: String,
    pub head_sha: String,
    pub conclusion: CheckConclusion,
    pub summary: String,
}

#[async_trait]
pub trait PullRequestsProvider: Send + Sync {
    async fn pull_request(
        &self,
        repository: &Repository,
        number: PullRequestNumber,
    ) -> Result<PullRequest>;
    /// Returns every thread and its complete comment sequence in provider order.
    async fn review_threads(
        &self,
        repository: &Repository,
        number: PullRequestNumber,
    ) -> Result<Vec<ReviewThread>>;
    async fn resolve_thread(
        &self,
        repository: &Repository,
        number: PullRequestNumber,
        thread_id: &ReviewThreadId,
    ) -> Result<()>;
    async fn request_reviewers(
        &self,
        repository: &Repository,
        number: PullRequestNumber,
        reviewers: &[String],
    ) -> Result<()>;
    async fn mark_ready(&self, repository: &Repository, number: PullRequestNumber) -> Result<()>;
    async fn publish_check(
        &self,
        repository: &Repository,
        app_identity: &str,
        outcome: &CheckOutcome,
    ) -> Result<()>;
}
