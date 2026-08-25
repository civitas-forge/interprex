use async_trait::async_trait;
use postel::{
    CheckOutcome, CodeReview, CodeReviewNumber, CodeReviewsProvider, Repository, Result,
    ReviewFindingStatus, ReviewThreadId,
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

    async fn resolve_thread(
        &self,
        repository: &Repository,
        number: CodeReviewNumber,
        thread_id: &ReviewThreadId,
    ) -> Result<()> {
        let mut state = self.state.write().await;
        let code_review = state
            .code_reviews
            .get_mut(&(repository.clone(), number))
            .ok_or_else(|| missing(format!("code review {number:?} in {repository}")))?;
        for submission in &mut code_review.submissions {
            if let Some(finding) = submission
                .findings
                .iter_mut()
                .find(|finding| &finding.thread_id == thread_id)
            {
                finding.status = ReviewFindingStatus::Resolved;
                return Ok(());
            }
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
