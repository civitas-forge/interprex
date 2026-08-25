use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{Repository, Result};

platform_number!(RunId);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    InProgress,
    Completed,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunConclusion {
    Success,
    Failure,
    Neutral,
    Cancelled,
    Skipped,
    TimedOut,
    ActionRequired,
    Stale,
    StartupFailure,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorkflowRun {
    pub id: RunId,
    pub workflow_name: Option<String>,
    pub head_sha: String,
    pub status: RunStatus,
    pub conclusion: Option<RunConclusion>,
    pub html_url: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DispatchInputs(pub BTreeMap<String, serde_json::Value>);

#[async_trait]
pub trait JobsProvider: Send + Sync {
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
