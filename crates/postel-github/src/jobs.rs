//! GitHub Actions operations owned by the jobs domain.
//!
//! Dispatch and cancellation use Octocrab's typed operations because their
//! successful responses have no JSON body. Run responses are normalized into a
//! small status and conclusion vocabularies; new GitHub values become
//! `Unknown` rather than causing deserialization failure or silently
//! pretending to be completed. An omitted workflow name remains absent.

use async_trait::async_trait;
use postel_contracts::{JobsDomain, ProviderError, Result};
use postel_model::{DispatchInputs, Repository, RunConclusion, RunId, RunStatus, WorkflowRun};
use serde::Deserialize;

use crate::{GithubProvider, api::external};

#[derive(Deserialize)]
struct GithubRun {
    id: u64,
    name: Option<String>,
    head_sha: String,
    status: String,
    conclusion: Option<String>,
    html_url: String,
}

fn normalize_run(value: GithubRun) -> Result<WorkflowRun> {
    let status = match value.status.as_str() {
        "queued" | "waiting" | "pending" => RunStatus::Queued,
        "in_progress" | "requested" => RunStatus::InProgress,
        "completed" => RunStatus::Completed,
        _ => RunStatus::Unknown,
    };
    let conclusion = value.conclusion.map(|value| match value.as_str() {
        "success" => RunConclusion::Success,
        "failure" => RunConclusion::Failure,
        "neutral" => RunConclusion::Neutral,
        "cancelled" => RunConclusion::Cancelled,
        "skipped" => RunConclusion::Skipped,
        "timed_out" => RunConclusion::TimedOut,
        "action_required" => RunConclusion::ActionRequired,
        "stale" => RunConclusion::Stale,
        "startup_failure" => RunConclusion::StartupFailure,
        _ => RunConclusion::Unknown,
    });
    Ok(WorkflowRun {
        id: RunId::new(value.id).map_err(|error| ProviderError::External {
            provider: "github",
            operation: "normalize workflow run",
            message: error.to_string(),
        })?,
        workflow_name: value.name,
        head_sha: value.head_sha,
        status,
        conclusion,
        html_url: value.html_url,
    })
}

#[async_trait]
impl JobsDomain for GithubProvider {
    async fn dispatch(
        &self,
        repository: &Repository,
        workflow: &str,
        git_ref: &str,
        inputs: &DispatchInputs,
    ) -> Result<()> {
        self.user()?
            .actions()
            .create_workflow_dispatch(repository.owner(), repository.name(), workflow, git_ref)
            .inputs(serde_json::Value::Object(
                inputs.0.clone().into_iter().collect(),
            ))
            .send()
            .await
            .map_err(|error| external("dispatch workflow", error))?;
        Ok(())
    }

    async fn run(&self, repository: &Repository, run_id: RunId) -> Result<WorkflowRun> {
        let response: GithubRun = self
            .user()?
            .get(
                format!("/repos/{repository}/actions/runs/{}", run_id.get()),
                None::<&()>,
            )
            .await
            .map_err(|error| {
                crate::api::read_error(
                    "read workflow run",
                    format!("workflow run {} in {repository}", run_id.get()),
                    error,
                )
            })?;
        normalize_run(response)
    }

    async fn cancel_run(&self, repository: &Repository, run_id: RunId) -> Result<()> {
        self.user()?
            .actions()
            .cancel_workflow_run(repository.owner(), repository.name(), run_id.get().into())
            .await
            .map_err(|error| external("cancel workflow run", error))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{GithubRun, normalize_run};
    use postel_model::{RunConclusion, RunStatus};

    #[test]
    fn run_fixture_normalizes_status_without_exporting_octocrab_types() {
        let response: GithubRun =
            serde_json::from_str(include_str!("../tests/fixtures/workflow_run.json"))
                .expect("fixture");
        let run = normalize_run(response).expect("normalizes");
        assert_eq!(run.status, RunStatus::Completed);
        assert_eq!(run.workflow_name.as_deref(), Some("quality"));
        assert_eq!(run.conclusion, Some(RunConclusion::Success));
    }

    #[test]
    fn omitted_name_stays_absent_and_new_conclusions_are_explicitly_unknown() {
        let response: GithubRun = serde_json::from_str(
            r#"{
                "id": 123456,
                "head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "status": "completed",
                "conclusion": "future_value",
                "html_url": "https://example.invalid/run"
            }"#,
        )
        .expect("response");
        let run = normalize_run(response).expect("normalizes");
        assert_eq!(run.workflow_name, None);
        assert_eq!(run.conclusion, Some(RunConclusion::Unknown));
    }
}
