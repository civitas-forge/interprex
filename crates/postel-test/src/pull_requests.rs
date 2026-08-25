use async_trait::async_trait;
use postel::{
    CheckOutcome, PullRequest, PullRequestNumber, PullRequestsProvider, Repository, Result,
    ReviewThread, ReviewThreadId,
};

use crate::state::{FakeProvider, missing};

#[async_trait]
impl PullRequestsProvider for FakeProvider {
    async fn pull_request(
        &self,
        repository: &Repository,
        number: PullRequestNumber,
    ) -> Result<PullRequest> {
        self.state
            .read()
            .await
            .pull_requests
            .get(&(repository.clone(), number))
            .cloned()
            .ok_or_else(|| missing(format!("pull request {number:?} in {repository}")))
    }

    async fn review_threads(
        &self,
        repository: &Repository,
        number: PullRequestNumber,
    ) -> Result<Vec<ReviewThread>> {
        Ok(self
            .state
            .read()
            .await
            .threads
            .get(&(repository.clone(), number))
            .cloned()
            .unwrap_or_default())
    }

    async fn resolve_thread(
        &self,
        repository: &Repository,
        number: PullRequestNumber,
        thread_id: &ReviewThreadId,
    ) -> Result<()> {
        let mut state = self.state.write().await;
        let threads = state
            .threads
            .get_mut(&(repository.clone(), number))
            .ok_or_else(|| missing(format!("review threads for pull request {number:?}")))?;
        if let Some(thread) = threads.iter_mut().find(|thread| &thread.id == thread_id) {
            thread.resolved = true;
            return Ok(());
        }
        Err(missing(format!("review thread {}", thread_id.as_str())))
    }

    async fn request_reviewers(
        &self,
        repository: &Repository,
        number: PullRequestNumber,
        reviewers: &[String],
    ) -> Result<()> {
        self.state
            .write()
            .await
            .requested_reviewers
            .insert((repository.clone(), number), reviewers.to_vec());
        Ok(())
    }

    async fn mark_ready(&self, repository: &Repository, number: PullRequestNumber) -> Result<()> {
        let mut state = self.state.write().await;
        let pull_request = state
            .pull_requests
            .get_mut(&(repository.clone(), number))
            .ok_or_else(|| missing(format!("pull request {number:?} in {repository}")))?;
        pull_request.draft = false;
        Ok(())
    }

    async fn publish_check(
        &self,
        repository: &Repository,
        app_identity: &str,
        outcome: &CheckOutcome,
    ) -> Result<()> {
        self.state.write().await.published_checks.push((
            repository.clone(),
            app_identity.to_owned(),
            outcome.clone(),
        ));
        Ok(())
    }
}
