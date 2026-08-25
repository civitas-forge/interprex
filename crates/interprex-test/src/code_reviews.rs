use async_trait::async_trait;
use interprex::{
    ChangeRequest, ChangeRequestNumber, CheckOutcome, CodeReviewsProvider, Repository, Result,
    ReviewActor, ReviewActorId, ReviewActorKind, ReviewRequest, ReviewRequestId,
    ReviewRequestTarget, ReviewTarget, ReviewTeam, ReviewTeamId, ReviewTeamKind, ReviewThreadId,
    ReviewThreadStatus,
};

use crate::state::{FakeProvider, missing};

#[async_trait]
impl CodeReviewsProvider for FakeProvider {
    async fn change_request(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
    ) -> Result<ChangeRequest> {
        self.state
            .read()
            .await
            .change_requests
            .get(&(repository.clone(), number))
            .cloned()
            .ok_or_else(|| missing(format!("change request {number:?} in {repository}")))
    }

    async fn resolve_thread(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        thread_id: &ReviewThreadId,
    ) -> Result<()> {
        let mut state = self.state.write().await;
        let change_request = state
            .change_requests
            .get_mut(&(repository.clone(), number))
            .ok_or_else(|| missing(format!("change request {number:?} in {repository}")))?;
        let finding = change_request
            .reviews
            .iter_mut()
            .flat_map(|review| review.findings.iter_mut())
            .find(|thread| &thread.id == thread_id);
        let thread = finding.or_else(|| {
            change_request
                .standalone_threads
                .iter_mut()
                .find(|thread| &thread.id == thread_id)
        });
        if let Some(thread) = thread {
            thread.status = ReviewThreadStatus::Resolved;
            return Ok(());
        }
        Err(missing(format!("review thread {}", thread_id.as_str())))
    }

    async fn request_reviewers(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        reviewers: &[ReviewRequestTarget],
    ) -> Result<()> {
        let mut state = self.state.write().await;
        let change_request = state
            .change_requests
            .get_mut(&(repository.clone(), number))
            .ok_or_else(|| missing(format!("change request {number:?} in {repository}")))?;
        for target in reviewers {
            if change_request
                .outstanding_requests
                .iter()
                .any(|request| request.request_target.as_ref() == Some(target))
            {
                continue;
            }
            change_request
                .outstanding_requests
                .push(fake_review_request(repository, number, target));
        }
        Ok(())
    }

    async fn mark_ready(&self, repository: &Repository, number: ChangeRequestNumber) -> Result<()> {
        let mut state = self.state.write().await;
        let change_request = state
            .change_requests
            .get_mut(&(repository.clone(), number))
            .ok_or_else(|| missing(format!("change request {number:?} in {repository}")))?;
        change_request.draft = false;
        Ok(())
    }

    async fn publish_check(
        &self,
        repository: &Repository,
        app_name: &str,
        outcome: &CheckOutcome,
    ) -> Result<()> {
        self.state.write().await.published_checks.push((
            repository.clone(),
            app_name.to_owned(),
            outcome.clone(),
        ));
        Ok(())
    }
}

fn fake_review_request(
    repository: &Repository,
    number: ChangeRequestNumber,
    target: &ReviewRequestTarget,
) -> ReviewRequest {
    let request_target = target.clone();
    let (identity, target) = match target {
        ReviewRequestTarget::User(login) => (
            format!("user:{login}"),
            ReviewTarget::Actor(ReviewActor {
                id: ReviewActorId::new(format!("fake-user:{login}"))
                    .expect("fake user identity is nonempty"),
                login: login.clone(),
                kind: ReviewActorKind::User,
            }),
        ),
        ReviewRequestTarget::Bot(login) => (
            format!("bot:{login}"),
            ReviewTarget::Actor(ReviewActor {
                id: ReviewActorId::new(format!("fake-bot:{login}"))
                    .expect("fake bot identity is nonempty"),
                login: login.clone(),
                kind: ReviewActorKind::Bot,
            }),
        ),
        ReviewRequestTarget::Team(identifier) => {
            let slug = identifier
                .rsplit('/')
                .next()
                .unwrap_or(identifier)
                .to_owned();
            (
                format!("team:{identifier}"),
                ReviewTarget::Team(ReviewTeam {
                    id: ReviewTeamId::new(format!("fake-team:{identifier}"))
                        .expect("fake team identity is nonempty"),
                    slug: slug.clone(),
                    name: slug,
                    kind: ReviewTeamKind::Organization,
                }),
            )
        }
    };
    ReviewRequest {
        id: ReviewRequestId::new(format!(
            "fake-request:{}:{}:{}:{}:{}:{}:{}",
            repository.owner().len(),
            repository.owner(),
            repository.name().len(),
            repository.name(),
            number.get(),
            identity.len(),
            identity
        ))
        .expect("fake request identity is nonempty"),
        target,
        request_target: Some(request_target),
        as_code_owner: false,
    }
}
