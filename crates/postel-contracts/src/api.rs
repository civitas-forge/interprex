//! Stable contract interface and structured provider failures.

use std::fmt;

use async_trait::async_trait;
use bytes::Bytes;
use postel_model::{
    CheckOutcome, DispatchInputs, Issue, IssueNumber, Label, NewRelease, PullRequest,
    PullRequestNumber, Release, ReleaseAsset, Repository, RepositoryFacts, RepositorySettings,
    ReviewThread, Ruleset, RunId, WorkflowRun,
};
use secrecy::SecretString;
use thiserror::Error;

pub const REPO_PROVIDER_ENV: &str = "POSTEL_REPO_PROVIDER";
pub const TRACKER_PROVIDER_ENV: &str = "POSTEL_TRACKER_PROVIDER";
pub const PR_PROVIDER_ENV: &str = "POSTEL_PR_PROVIDER";
pub const JOBS_PROVIDER_ENV: &str = "POSTEL_JOBS_PROVIDER";
pub const RELEASES_PROVIDER_ENV: &str = "POSTEL_RELEASES_PROVIDER";
pub const DEFAULT_PROVIDER: &str = "github";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderSelections {
    pub repo: String,
    pub tracker: String,
    pub pr: String,
    pub jobs: String,
    pub releases: String,
}

impl ProviderSelections {
    pub fn from_lookup(mut lookup: impl FnMut(&str) -> Option<String>) -> Self {
        let value = |value: Option<String>| {
            value
                .filter(|candidate| !candidate.trim().is_empty())
                .unwrap_or_else(|| DEFAULT_PROVIDER.to_owned())
        };
        Self {
            repo: value(lookup(REPO_PROVIDER_ENV)),
            tracker: value(lookup(TRACKER_PROVIDER_ENV)),
            pr: value(lookup(PR_PROVIDER_ENV)),
            jobs: value(lookup(JOBS_PROVIDER_ENV)),
            releases: value(lookup(RELEASES_PROVIDER_ENV)),
        }
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ProviderError {
    #[error("{provider} cannot express required fact: {fact}")]
    Refused {
        provider: &'static str,
        fact: String,
    },
    #[error("{entity} was not found")]
    NotFound { entity: String },
    #[error("missing {kind} credential for identity {identity}")]
    MissingCredential {
        identity: String,
        kind: &'static str,
    },
    #[error("provider configuration from {origin} failed: {reason}")]
    Configuration {
        origin: ConfigurationSource,
        reason: String,
    },
    #[error("{provider} {operation} failed: {message}")]
    External {
        provider: &'static str,
        operation: &'static str,
        message: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigurationSource {
    Direct,
    File(String),
}

impl fmt::Display for ConfigurationSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Direct => formatter.write_str("direct construction"),
            Self::File(path) => write!(formatter, "file {path}"),
        }
    }
}

pub type Result<T> = std::result::Result<T, ProviderError>;

#[async_trait]
pub trait RepoDomain: Send + Sync {
    async fn repository(&self, repository: &Repository) -> Result<RepositoryFacts>;
    async fn settings(&self, repository: &Repository) -> Result<RepositorySettings>;
    async fn apply_settings(
        &self,
        repository: &Repository,
        settings: &RepositorySettings,
    ) -> Result<RepositorySettings>;
    async fn rulesets(&self, repository: &Repository) -> Result<Vec<Ruleset>>;
    async fn upsert_ruleset(&self, repository: &Repository, ruleset: &Ruleset) -> Result<Ruleset>;
    async fn put_secret(
        &self,
        repository: &Repository,
        name: &str,
        value: SecretString,
    ) -> Result<()>;
}

#[async_trait]
pub trait TrackerDomain: Send + Sync {
    async fn issue(&self, repository: &Repository, number: IssueNumber) -> Result<Issue>;
    async fn labels(&self, repository: &Repository) -> Result<Vec<Label>>;
    async fn upsert_label(&self, repository: &Repository, label: &Label) -> Result<Label>;
}

#[async_trait]
pub trait PrDomain: Send + Sync {
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
    async fn resolve_thread(&self, thread_id: &str) -> Result<()>;
    async fn request_reviewers(
        &self,
        repository: &Repository,
        number: PullRequestNumber,
        reviewers: &[String],
    ) -> Result<()>;
    async fn mark_ready(&self, pull_request_node_id: &str) -> Result<()>;
    async fn publish_check(
        &self,
        repository: &Repository,
        app_identity: &str,
        outcome: &CheckOutcome,
    ) -> Result<()>;
}

#[async_trait]
pub trait JobsDomain: Send + Sync {
    async fn dispatch(
        &self,
        repository: &Repository,
        workflow: &str,
        git_ref: &str,
        inputs: &DispatchInputs,
    ) -> Result<()>;
    async fn run(&self, repository: &Repository, run_id: RunId) -> Result<WorkflowRun>;
    async fn cancel_run(&self, repository: &Repository, run_id: RunId) -> Result<()>;
}

#[async_trait]
pub trait ReleasesDomain: Send + Sync {
    async fn release_by_tag(&self, repository: &Repository, tag: &str) -> Result<Release>;
    async fn create_release(
        &self,
        repository: &Repository,
        release: &NewRelease,
    ) -> Result<Release>;
    async fn upload_asset(
        &self,
        repository: &Repository,
        release_id: postel_model::ReleaseId,
        name: &str,
        label: Option<&str>,
        content: Bytes,
    ) -> Result<ReleaseAsset>;
    async fn download_asset(
        &self,
        repository: &Repository,
        asset_id: postel_model::AssetId,
    ) -> Result<Bytes>;
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_PROVIDER, PR_PROVIDER_ENV, ProviderSelections};

    #[test]
    fn selections_are_independent_and_default_to_github() {
        let selections = ProviderSelections::from_lookup(|name| {
            (name == PR_PROVIDER_ENV).then(|| "gitlab".to_owned())
        });
        assert_eq!(selections.pr, "gitlab");
        assert_eq!(selections.repo, DEFAULT_PROVIDER);
        assert_eq!(selections.jobs, DEFAULT_PROVIDER);
    }

    #[test]
    fn blank_selection_is_treated_as_unset() {
        let selections = ProviderSelections::from_lookup(|_| Some("  ".to_owned()));
        assert_eq!(selections.tracker, DEFAULT_PROVIDER);
    }
}
