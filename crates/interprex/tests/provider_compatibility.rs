use async_trait::async_trait;
use interprex::{
    ChangeRequest, ChangeRequestHead, ChangeRequestNumber, CheckOutcome, CheckRun,
    CodeReviewsProvider, FindingResolution, FindingResolutionReply, Repository, Result,
    ReviewRequestTarget, ReviewThreadId,
};

struct ExistingProvider;

#[async_trait]
impl CodeReviewsProvider for ExistingProvider {
    async fn change_request(
        &self,
        _repository: &Repository,
        _number: ChangeRequestNumber,
    ) -> Result<ChangeRequest> {
        unimplemented!()
    }

    async fn open_change_requests(
        &self,
        _repository: &Repository,
        _head: &ChangeRequestHead,
    ) -> Result<Vec<ChangeRequestNumber>> {
        unimplemented!()
    }

    async fn resolve_thread(
        &self,
        _repository: &Repository,
        _number: ChangeRequestNumber,
        _thread_id: &ReviewThreadId,
    ) -> Result<()> {
        unimplemented!()
    }

    async fn resolve_finding(
        &self,
        _repository: &Repository,
        _number: ChangeRequestNumber,
        _thread_id: &ReviewThreadId,
        _resolution: FindingResolution,
        _reply: &FindingResolutionReply,
    ) -> Result<()> {
        unimplemented!()
    }

    async fn request_reviewers(
        &self,
        _repository: &Repository,
        _number: ChangeRequestNumber,
        _reviewers: &[ReviewRequestTarget],
    ) -> Result<()> {
        unimplemented!()
    }

    async fn mark_ready(
        &self,
        _repository: &Repository,
        _number: ChangeRequestNumber,
    ) -> Result<()> {
        unimplemented!()
    }

    async fn checks(&self, _repository: &Repository, _head_sha: &str) -> Result<Vec<CheckRun>> {
        unimplemented!()
    }

    async fn publish_check(
        &self,
        _repository: &Repository,
        _app_name: &str,
        _outcome: &CheckOutcome,
    ) -> Result<()> {
        unimplemented!()
    }
}

#[test]
fn existing_provider_implementations_need_no_branch_update_methods() {
    fn accepts_code_reviews_provider<T: CodeReviewsProvider>() {}

    accepts_code_reviews_provider::<ExistingProvider>();
}
