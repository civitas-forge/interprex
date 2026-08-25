use async_trait::async_trait;
use postel::{
    CheckOutcome, CodeReview, CodeReviewNumber, CodeReviewsProvider, Repository, Result,
    ReviewThread, ReviewThreadId,
};

use crate::state::{FakeProvider, missing};

#[async_trait]
impl CodeReviewsProvider for FakeProvider {
    async fn code_review(
        &self,
        repository: &Repository,
        number: CodeReviewNumber,
    ) -> Result<CodeReview> {
        self.state
            .read()
            .await
            .code_reviews
            .get(&(repository.clone(), number))
            .cloned()
            .ok_or_else(|| missing(format!("code review {number:?} in {repository}")))
    }

    async fn review_threads(
        &self,
        repository: &Repository,
        number: CodeReviewNumber,
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
        number: CodeReviewNumber,
        thread_id: &ReviewThreadId,
    ) -> Result<()> {
        let mut state = self.state.write().await;
        let threads = state
            .threads
            .get_mut(&(repository.clone(), number))
            .ok_or_else(|| missing(format!("review threads for code review {number:?}")))?;
        if let Some(thread) = threads.iter_mut().find(|thread| &thread.id == thread_id) {
            thread.resolved = true;
            return Ok(());
        }
        Err(missing(format!("review thread {}", thread_id.as_str())))
    }

    async fn request_reviewers(
        &self,
        repository: &Repository,
        number: CodeReviewNumber,
        reviewers: &[String],
    ) -> Result<()> {
        self.state
            .write()
            .await
            .requested_reviewers
            .insert((repository.clone(), number), reviewers.to_vec());
        Ok(())
    }

    async fn mark_ready(&self, repository: &Repository, number: CodeReviewNumber) -> Result<()> {
        let mut state = self.state.write().await;
        let code_review = state
            .code_reviews
            .get_mut(&(repository.clone(), number))
            .ok_or_else(|| missing(format!("code review {number:?} in {repository}")))?;
        code_review.draft = false;
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
