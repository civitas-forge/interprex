use async_trait::async_trait;
use interprex::{
    ChangeRequest, ChangeRequestHead, ChangeRequestNumber, ChangeRequestState, CheckOutcome,
    CheckRun, CodeReviewsProvider, FindingResolution, FindingResolutionRecord,
    FindingResolutionReply, ProviderError, Repository, Result, ReviewActor, ReviewActorId,
    ReviewActorKind, ReviewComment, ReviewCommentId, ReviewRequest, ReviewRequestId,
    ReviewRequestTarget, ReviewRequestTargetInspection, ReviewTarget, ReviewTargetsProvider,
    ReviewTeam, ReviewTeamId, ReviewTeamKind, ReviewThreadId, ReviewThreadStatus,
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

    async fn open_change_requests(
        &self,
        repository: &Repository,
        head: &ChangeRequestHead,
    ) -> Result<Vec<ChangeRequestNumber>> {
        Ok(self
            .state
            .read()
            .await
            .change_requests
            .iter()
            .filter(|((targeted, _), change_request)| {
                targeted == repository
                    && change_request.state == ChangeRequestState::Open
                    && change_request.head.as_ref() == Some(head)
            })
            .map(|((_, number), _)| *number)
            .collect())
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
        if let Some(finding) = change_request
            .reviews
            .iter_mut()
            .flat_map(|review| review.findings.iter_mut())
            .find(|thread| &thread.id == thread_id)
        {
            finding.status = ReviewThreadStatus::Resolved;
            return Ok(());
        }
        if let Some(thread) = change_request
            .standalone_threads
            .iter_mut()
            .find(|thread| &thread.id == thread_id)
        {
            thread.status = ReviewThreadStatus::Resolved;
            return Ok(());
        }
        Err(missing(format!("review thread {}", thread_id.as_str())))
    }

    async fn resolve_finding(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        thread_id: &ReviewThreadId,
        resolution: FindingResolution,
        reply: &FindingResolutionReply,
    ) -> Result<()> {
        let mut state = self.state.write().await;
        let change_request = state
            .change_requests
            .get_mut(&(repository.clone(), number))
            .ok_or_else(|| missing(format!("change request {number:?} in {repository}")))?;
        let observed_at = change_request.updated_at;
        let finding = change_request
            .reviews
            .iter_mut()
            .flat_map(|review| review.findings.iter_mut())
            .find(|thread| &thread.id == thread_id)
            .ok_or_else(|| missing(format!("finding thread {}", thread_id.as_str())))?;
        if let Some(FindingResolutionRecord::Unsupported {
            metadata_format, ..
        }) = &finding.resolution
        {
            return Err(ProviderError::Unrepresentable {
                provider: "fake",
                fact: format!(
                    "finding thread {} contains unsupported resolution metadata format {metadata_format}",
                    thread_id.as_str()
                ),
            });
        }
        if matches!(
            &finding.resolution,
            Some(FindingResolutionRecord::Supported {
                resolution: recorded,
                ..
            }) if *recorded == resolution
        ) {
            finding.status = ReviewThreadStatus::Resolved;
            return Ok(());
        }
        let author = ReviewActor {
            id: ReviewActorId::new("fake-provider:authenticated-actor")
                .expect("fake actor identity is nonempty"),
            login: "fake-provider".to_owned(),
            kind: ReviewActorKind::User,
        };
        let written_at = finding
            .replies
            .iter()
            .map(|comment| comment.updated_at.unwrap_or(comment.created_at))
            .chain([observed_at])
            .max()
            .expect("the observed change supplies one timestamp")
            + std::time::Duration::from_micros(1);
        let reply_number = finding
            .replies
            .iter()
            .filter(|comment| comment.id.as_str().starts_with("fake-resolution:"))
            .count()
            + 1;
        let reply_id = ReviewCommentId::new(format!(
            "fake-resolution:{}:{reply_number}",
            thread_id.as_str()
        ))
        .expect("fake resolution identity is nonempty");
        let source_reply = ReviewComment {
            id: reply_id,
            author,
            body: reply.as_str().to_owned(),
            created_at: written_at,
            updated_at: None,
        };
        let source_reply_id = source_reply.id.clone();
        finding.replies.push(source_reply);
        finding.resolution = Some(FindingResolutionRecord::Supported {
            resolution,
            source_reply_id,
        });
        finding.status = ReviewThreadStatus::Resolved;
        Ok(())
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
        let observed_at = change_request.updated_at;
        for target in reviewers {
            if change_request
                .outstanding_requests
                .iter()
                .any(|request| request.request_target.as_ref() == Some(target))
            {
                continue;
            }
            let requested_at = change_request
                .outstanding_requests
                .iter()
                .filter_map(|request| request.requested_at)
                .chain([observed_at])
                .max()
                .expect("the observed change supplies one timestamp")
                + std::time::Duration::from_micros(1);
            change_request
                .outstanding_requests
                .push(fake_review_request(
                    repository,
                    number,
                    target,
                    requested_at,
                ));
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

    async fn checks(&self, repository: &Repository, head_sha: &str) -> Result<Vec<CheckRun>> {
        Ok(self
            .state
            .read()
            .await
            .check_runs
            .get(&(repository.clone(), head_sha.to_owned()))
            .cloned()
            .unwrap_or_default())
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

#[async_trait]
impl ReviewTargetsProvider for FakeProvider {
    async fn inspect_review_request_target(
        &self,
        repository: &Repository,
        target: &ReviewRequestTarget,
    ) -> Result<ReviewRequestTargetInspection> {
        Ok(self
            .state
            .read()
            .await
            .review_target_observations
            .iter()
            .find(|(seeded_repository, seeded_target, _)| {
                seeded_repository == repository && seeded_target == target
            })
            .map_or(
                ReviewRequestTargetInspection::NotResolvable,
                |(_, _, observed)| {
                    ReviewRequestTargetInspection::from_observation(target, observed.clone())
                },
            ))
    }
}

/// Builds the request the fake records for one newly requested target.
///
/// `requested_at` comes from the seeded observation rather than the wall
/// clock: the caller advances it past the change request's last update and
/// past every request already outstanding, so a test that seeds the same
/// change request twice reads the same times both times and still sees
/// requests ordered by when they were made.
fn fake_review_request(
    repository: &Repository,
    number: ChangeRequestNumber,
    target: &ReviewRequestTarget,
    requested_at: chrono::DateTime<chrono::Utc>,
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
        requested_at: Some(requested_at),
        as_code_owner: false,
    }
}
