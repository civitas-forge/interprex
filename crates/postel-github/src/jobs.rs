//! GitHub Actions operations owned by the jobs domain.
//!
//! Dispatch and cancellation use Octocrab's typed operations because their
//! successful responses have no JSON body. Run responses are normalized into a
//! small status vocabulary; new GitHub states become `Unknown` rather than
//! causing deserialization failure or silently pretending to be completed.

use async_trait::async_trait;
use postel_contracts::{JobsDomain, ProviderError, Result};
use postel_model::{DispatchInputs, Repository, RunId, RunStatus, WorkflowRun};
use serde::Deserialize;

use crate::{GithubProvider, api::external};

#[derive(Deserialize)]
struct GithubRun {
    id: u64,
    #[serde(default)]
    name: String,
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
    Ok(WorkflowRun {
        id: RunId::new(value.id).map_err(|error| ProviderError::External {
            provider: "github",
            operation: "normalize workflow run",
            message: error.to_string(),
        })?,
        workflow_name: value.name,
        head_sha: value.head_sha,
        status,
        conclusion: value.conclusion,
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
    use postel_model::RunStatus;

    #[test]
    fn run_fixture_normalizes_status_without_exporting_octocrab_types() {
        let response: GithubRun =
            serde_json::from_str(include_str!("../tests/fixtures/workflow_run.json"))
                .expect("fixture");
        let run = normalize_run(response).expect("normalizes");
        assert_eq!(run.status, RunStatus::Completed);
        assert_eq!(run.conclusion.as_deref(), Some("success"));
    }
}
