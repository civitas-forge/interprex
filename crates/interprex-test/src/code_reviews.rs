use async_trait::async_trait;
use interprex::{
    BranchUpdateError, BranchUpdateObservation, BranchUpdatesProvider, ChangeRequest,
    ChangeRequestCommentsProvider, ChangeRequestHead, ChangeRequestNumber, ChangeRequestState,
    CheckOutcome, CheckRun, CodeReviewsProvider, FindingResolution, FindingResolutionRecord,
    FindingResolutionReply, ProviderError, Repository, Result, ReviewActor, ReviewActorId,
    ReviewActorKind, ReviewAnchor, ReviewAuthor, ReviewComment, ReviewCommentId, ReviewFinding,
    ReviewId, ReviewLineRange, ReviewLocation, ReviewPublicationKey, ReviewPublishingProvider,
    ReviewRequest, ReviewRequestId, ReviewRequestTarget, ReviewRequestTargetInspection,
    ReviewState, ReviewSubmission, ReviewTarget, ReviewTargetsProvider, ReviewTeam, ReviewTeamId,
    ReviewTeamKind, ReviewThread, ReviewThreadId, ReviewThreadStatus, ReviewerApplication,
    ReviewerApplicationsProvider,
};

use crate::state::{FakeProvider, FakeReviewPublication, FakeReviewPublicationKey, missing};

#[async_trait]
impl BranchUpdatesProvider for FakeProvider {
    async fn branch_update(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
    ) -> Result<BranchUpdateObservation> {
        self.state
            .read()
            .await
            .branch_updates
            .get(&(repository.clone(), number))
            .cloned()
            .ok_or_else(|| {
                missing(format!(
                    "branch update for change request {number:?} in {repository}"
                ))
            })
    }

    async fn update_change_request_branch(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        expected_head_sha: &str,
    ) -> std::result::Result<(), BranchUpdateError> {
        if expected_head_sha.is_empty() {
            return Err(ProviderError::InvalidInput {
                provider: "fake",
                fact: "expected change-request head must not be empty".to_owned(),
            }
            .into());
        }
        let mut state = self.state.write().await;
        let observation = state
            .branch_updates
            .get(&(repository.clone(), number))
            .ok_or_else(|| {
                missing(format!(
                    "branch update for change request {number:?} in {repository}"
                ))
            })
            .map_err(BranchUpdateError::from)?;
        if observation.commit_range.head_sha != expected_head_sha {
            return Err(BranchUpdateError::StaleHead {
                expected_head_sha: expected_head_sha.to_owned(),
                observed_head_sha: observation.commit_range.head_sha.clone(),
            });
        }
        state.accepted_branch_updates.push((
            repository.clone(),
            number,
            expected_head_sha.to_owned(),
        ));
        Ok(())
    }
}

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
impl ChangeRequestCommentsProvider for FakeProvider {
    async fn create_unanchored_comment(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        body: &str,
    ) -> Result<ReviewCommentId> {
        let mut state = self.state.write().await;
        let change_request = state
            .change_requests
            .get_mut(&(repository.clone(), number))
            .ok_or_else(|| missing(format!("change request {number:?} in {repository}")))?;
        let comment_number = change_request.unanchored_comments.len() + 1;
        let id = ReviewCommentId::new(format!(
            "fake-comment:{}:{}:{}:{}:{}:{comment_number}",
            repository.owner().len(),
            repository.owner(),
            repository.name().len(),
            repository.name(),
            number.get(),
        ))
        .expect("fake comment identity is nonempty");
        let created_at = change_request
            .unanchored_comments
            .iter()
            .map(|comment| comment.updated_at.unwrap_or(comment.created_at))
            .chain([change_request.updated_at])
            .max()
            .expect("the observed change supplies one timestamp")
            + std::time::Duration::from_micros(1);
        change_request.unanchored_comments.push(ReviewComment {
            id: id.clone(),
            author: ReviewActor {
                id: ReviewActorId::new("fake-provider:authenticated-actor")
                    .expect("fake actor identity is nonempty"),
                login: "fake-provider".to_owned(),
                kind: ReviewActorKind::User,
            },
            body: body.to_owned(),
            created_at,
            updated_at: None,
        });
        Ok(id)
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

#[async_trait]
impl ReviewerApplicationsProvider for FakeProvider {
    async fn resolve_reviewer_application(
        &self,
        repository: &Repository,
        slug: &str,
    ) -> Result<ReviewerApplication> {
        self.state
            .read()
            .await
            .reviewer_applications
            .get(&(repository.clone(), slug.to_owned()))
            .cloned()
            .ok_or_else(|| missing(format!("reviewer application {slug} in {repository}")))
    }
}

#[async_trait]
impl ReviewPublishingProvider for FakeProvider {
    async fn publish_review(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        reviewer: &ReviewerApplication,
        submission: &ReviewSubmission,
    ) -> Result<ReviewId> {
        let publication_key =
            fake_publication_key(repository, number, reviewer, submission.publication_key());
        let mut state = self.state.write().await;
        if let Some(existing) = state.review_publications.get(&publication_key) {
            if existing.submission == *submission {
                return Ok(existing.review_id.clone());
            }
            return Err(ProviderError::InvalidInput {
                provider: "fake",
                fact: format!(
                    "review publication key {:?} already names a different review",
                    submission.publication_key().as_str()
                ),
            });
        }

        let change_request = state
            .change_requests
            .get_mut(&(repository.clone(), number))
            .ok_or_else(|| missing(format!("change request {number:?} in {repository}")))?;
        let submitted_at = next_review_timestamp(change_request);
        let review_id = fake_review_id(repository, number, reviewer, submission.publication_key());
        change_request.reviews.push(fake_review(
            reviewer,
            submission,
            review_id.clone(),
            ReviewState::Submitted {
                disposition: submission.disposition().into(),
                submitted_at,
            },
            submitted_at,
        ));
        change_request.updated_at = submitted_at;
        state.review_publications.insert(
            publication_key,
            FakeReviewPublication {
                submission: submission.clone(),
                review_id: review_id.clone(),
            },
        );
        Ok(review_id)
    }

    async fn resume_review_publication(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        reviewer: &ReviewerApplication,
        key: &ReviewPublicationKey,
    ) -> Result<Option<ReviewId>> {
        let publication_key = fake_publication_key(repository, number, reviewer, key);
        let mut state = self.state.write().await;
        if !state
            .change_requests
            .contains_key(&(repository.clone(), number))
        {
            return Err(missing(format!(
                "change request {number:?} in {repository}"
            )));
        }
        let Some(publication) = state.review_publications.get(&publication_key).cloned() else {
            return Ok(None);
        };
        let change_request = state
            .change_requests
            .get_mut(&(repository.clone(), number))
            .ok_or_else(|| missing(format!("change request {number:?} in {repository}")))?;
        let submitted_at = next_review_timestamp(change_request);
        let review = change_request
            .reviews
            .iter_mut()
            .find(|review| review.id == publication.review_id)
            .ok_or_else(|| unreconciled_publication("recorded review is absent"))?;
        match &review.state {
            ReviewState::Draft => {
                review.state = ReviewState::Submitted {
                    disposition: publication.submission.disposition().into(),
                    submitted_at,
                };
                change_request.updated_at = submitted_at;
            }
            ReviewState::Submitted { disposition, .. }
                if *disposition != publication.submission.disposition().into() =>
            {
                return Err(unreconciled_publication(
                    "submitted review has a different disposition",
                ));
            }
            ReviewState::Submitted { .. } => {}
        }
        Ok(Some(publication.review_id))
    }
}

impl FakeProvider {
    /// Seeds the state left after a provider created one complete draft but the
    /// caller lost the publication response before final submission.
    pub async fn seed_pending_review_publication(
        &self,
        repository: Repository,
        number: ChangeRequestNumber,
        reviewer: ReviewerApplication,
        submission: ReviewSubmission,
    ) -> Result<ReviewId> {
        let publication_key =
            fake_publication_key(&repository, number, &reviewer, submission.publication_key());
        let mut state = self.state.write().await;
        if state.review_publications.contains_key(&publication_key) {
            return Err(ProviderError::InvalidInput {
                provider: "fake",
                fact: format!(
                    "review publication key {:?} is already seeded",
                    submission.publication_key().as_str()
                ),
            });
        }
        let change_request = state
            .change_requests
            .get_mut(&(repository.clone(), number))
            .ok_or_else(|| missing(format!("change request {number:?} in {repository}")))?;
        let created_at = next_review_timestamp(change_request);
        let review_id =
            fake_review_id(&repository, number, &reviewer, submission.publication_key());
        change_request.reviews.push(fake_review(
            &reviewer,
            &submission,
            review_id.clone(),
            ReviewState::Draft,
            created_at,
        ));
        change_request.updated_at = created_at;
        state.review_publications.insert(
            publication_key,
            FakeReviewPublication {
                submission,
                review_id: review_id.clone(),
            },
        );
        Ok(review_id)
    }
}

fn fake_review_id(
    repository: &Repository,
    number: ChangeRequestNumber,
    reviewer: &ReviewerApplication,
    key: &ReviewPublicationKey,
) -> ReviewId {
    ReviewId::new(format!(
        "fake-review:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
        repository.owner().len(),
        repository.owner(),
        repository.name().len(),
        repository.name(),
        number.get(),
        reviewer.app().id.as_str().len(),
        reviewer.app().id.as_str(),
        reviewer.bot().id.as_str().len(),
        reviewer.bot().id.as_str(),
        key.as_str().len(),
        key.as_str(),
    ))
    .expect("fake review identity is nonempty")
}

fn fake_publication_key(
    repository: &Repository,
    number: ChangeRequestNumber,
    reviewer: &ReviewerApplication,
    key: &ReviewPublicationKey,
) -> FakeReviewPublicationKey {
    (
        repository.clone(),
        number,
        reviewer.app().id.clone(),
        reviewer.bot().id.clone(),
        key.clone(),
    )
}

fn next_review_timestamp(change_request: &ChangeRequest) -> chrono::DateTime<chrono::Utc> {
    change_request
        .reviews
        .iter()
        .filter_map(|review| match review.state {
            ReviewState::Draft => None,
            ReviewState::Submitted { submitted_at, .. } => Some(submitted_at),
        })
        .chain([change_request.updated_at])
        .max()
        .expect("the observed change supplies one timestamp")
        + std::time::Duration::from_micros(1)
}

fn fake_review(
    reviewer: &ReviewerApplication,
    submission: &ReviewSubmission,
    review_id: ReviewId,
    state: ReviewState,
    created_at: chrono::DateTime<chrono::Utc>,
) -> interprex::Review {
    let findings = submission
        .findings()
        .iter()
        .enumerate()
        .map(|(index, finding)| {
            let line_range = ReviewLineRange {
                start: None,
                end: finding.line(),
            };
            let identity = format!("{}:{}", review_id.as_str(), index + 1);
            ReviewFinding {
                thread: ReviewThread {
                    id: ReviewThreadId::new(format!("fake-thread:{identity}"))
                        .expect("fake thread identity is nonempty"),
                    location: ReviewLocation {
                        path: finding.path().to_owned(),
                        anchor: ReviewAnchor::Lines {
                            side: finding.side().clone(),
                            original: line_range.clone(),
                            current: Some(line_range),
                        },
                    },
                    outdated: false,
                    status: ReviewThreadStatus::Open,
                    comment: ReviewComment {
                        id: ReviewCommentId::new(format!("fake-comment:{identity}"))
                            .expect("fake comment identity is nonempty"),
                        author: reviewer.bot().clone(),
                        body: finding.body().to_owned(),
                        created_at,
                        updated_at: None,
                    },
                    replies: Vec::new(),
                },
                resolution: None,
            }
        })
        .collect();
    interprex::Review {
        id: review_id,
        author: ReviewAuthor::Other(reviewer.bot().clone()),
        via_app: Some(reviewer.app().clone()),
        revision: submission.revision().clone(),
        state,
        summary: Some(submission.summary().to_owned()),
        findings,
    }
}

fn unreconciled_publication(message: impl Into<String>) -> ProviderError {
    ProviderError::External {
        provider: "fake",
        operation: "resume review publication",
        message: message.into(),
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
