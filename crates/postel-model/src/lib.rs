//! Provider-neutral values shared by Postel's domain contracts.
//!
//! This crate owns names and returned facts, not network behavior. Values are
//! deliberately narrower than vendor responses: adding a GitHub field here is
//! a promise every future provider must either honor or explicitly refuse.
//! Identifiers validate at construction so adapters never assemble endpoint
//! paths from unchecked caller strings.

#![forbid(unsafe_code)]

use std::{collections::BTreeMap, fmt, str::FromStr};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum ModelError {
    #[error("{field} must not be empty")]
    Empty { field: &'static str },
    #[error("{field} must not contain '/' or ASCII control characters")]
    InvalidSegment { field: &'static str },
    #[error("repository must have the form owner/name")]
    InvalidRepository,
    #[error("number must be greater than zero")]
    InvalidNumber,
}

fn segment(value: impl Into<String>, field: &'static str) -> Result<String, ModelError> {
    let value = value.into();
    if value.is_empty() {
        return Err(ModelError::Empty { field });
    }
    if value.contains('/') || value.chars().any(char::is_control) {
        return Err(ModelError::InvalidSegment { field });
    }
    Ok(value)
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Repository {
    owner: String,
    name: String,
}

impl Repository {
    pub fn new(owner: impl Into<String>, name: impl Into<String>) -> Result<Self, ModelError> {
        Ok(Self {
            owner: segment(owner, "owner")?,
            name: segment(name, "repository name")?,
        })
    }

    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for Repository {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.owner, self.name)
    }
}

impl FromStr for Repository {
    type Err = ModelError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (owner, name) = value.split_once('/').ok_or(ModelError::InvalidRepository)?;
        if name.contains('/') {
            return Err(ModelError::InvalidRepository);
        }
        Self::new(owner, name)
    }
}

macro_rules! number {
    ($name:ident) => {
        #[derive(
            Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
        )]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            pub fn new(value: u64) -> Result<Self, ModelError> {
                (value > 0)
                    .then_some(Self(value))
                    .ok_or(ModelError::InvalidNumber)
            }

            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
        }
    };
}

number!(IssueNumber);
number!(PullRequestNumber);
number!(RunId);
number!(ReleaseId);
number!(AssetId);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenClosed {
    Open,
    Closed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RepositoryFacts {
    pub repository: Repository,
    pub default_branch: String,
    pub private: bool,
    pub archived: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RepositorySettings {
    pub allow_squash_merge: bool,
    pub allow_merge_commit: bool,
    pub allow_rebase_merge: bool,
    pub delete_branch_on_merge: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RequiredCheck {
    pub context: String,
    pub integration_id: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Ruleset {
    pub id: Option<u64>,
    pub name: String,
    pub active: bool,
    pub target_branch_patterns: Vec<String>,
    pub required_checks: Vec<RequiredCheck>,
    pub required_approvals: u8,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Label {
    pub name: String,
    pub color: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Issue {
    pub number: IssueNumber,
    pub title: String,
    pub body: Option<String>,
    pub state: OpenClosed,
    pub labels: Vec<Label>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PullRequest {
    pub number: PullRequestNumber,
    pub node_id: String,
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
    pub id: String,
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    InProgress,
    Completed,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkflowRun {
    pub id: RunId,
    pub workflow_name: String,
    pub head_sha: String,
    pub status: RunStatus,
    pub conclusion: Option<String>,
    pub html_url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Release {
    pub id: ReleaseId,
    pub tag: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReleaseAsset {
    pub id: AssetId,
    pub name: String,
    pub label: Option<String>,
    pub size: u64,
    pub download_url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct NewRelease {
    pub tag: String,
    pub name: Option<String>,
    pub body: Option<String>,
    pub target: Option<String>,
    pub draft: bool,
    pub prerelease: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DispatchInputs(pub BTreeMap<String, serde_json::Value>);

#[cfg(test)]
mod tests {
    use super::{ModelError, PullRequestNumber, Repository};

    #[test]
    fn repository_round_trips_through_its_canonical_address() {
        let repository: Repository = "faictor/postel".parse().expect("valid repository");
        assert_eq!(repository.to_string(), "faictor/postel");
    }

    #[test]
    fn repository_rejects_extra_path_segments() {
        assert_eq!(
            "faictor/postel/extra".parse::<Repository>(),
            Err(ModelError::InvalidRepository)
        );
    }

    #[test]
    fn platform_numbers_are_one_based() {
        assert_eq!(PullRequestNumber::new(0), Err(ModelError::InvalidNumber));
        assert_eq!(PullRequestNumber::new(42).expect("positive").get(), 42);
    }
}
