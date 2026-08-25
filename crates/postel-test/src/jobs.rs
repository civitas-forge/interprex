use async_trait::async_trait;
use postel::{DispatchInputs, JobsProvider, Repository, Result, RunId, WorkflowRun};

use crate::state::{FakeProvider, missing};

#[async_trait]
impl JobsProvider for FakeProvider {
    async fn dispatch(
        &self,
        repository: &Repository,
        workflow: &str,
        git_ref: &str,
        inputs: &DispatchInputs,
    ) -> Result<()> {
        self.state.write().await.dispatches.push((
            repository.clone(),
            workflow.to_owned(),
            git_ref.to_owned(),
            inputs.clone(),
        ));
        Ok(())
    }

    async fn run(&self, repository: &Repository, run_id: RunId) -> Result<WorkflowRun> {
        self.state
            .read()
            .await
            .runs
            .get(&(repository.clone(), run_id))
            .cloned()
            .ok_or_else(|| missing(format!("workflow run {run_id:?} in {repository}")))
    }

    async fn cancel_run(&self, repository: &Repository, run_id: RunId) -> Result<()> {
        self.state
            .write()
            .await
            .cancelled_runs
            .push((repository.clone(), run_id));
        Ok(())
    }
}
