use std::time::Duration;

use async_trait::async_trait;
use interprex::{
    ChangeRequestNumber, ProviderError, ProviderTextRecord, Repository, Result, ReviewDiffSide,
    ReviewId, ReviewPublicationKey, ReviewPublishingProvider, ReviewSubmission,
    ReviewSubmissionDisposition, ReviewerApplication,
};
use octocrab::Page;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    GithubProvider,
    client::{ConfiguredApp, authenticated_external, is_not_found},
};

use super::change_requests::GithubReview;

const PUBLICATION_NAMESPACE: &str = "interprex";
const PUBLICATION_NAME: &str = "review-publication";
const PUBLICATION_VERSION: u64 = 1;
const PUBLICATION_IDENTIFIER: &str = "interprex:review-publication";
const PUBLICATION_CARRIER: &str = "<!-- interprex:review-publication";
const MAX_OBSERVED_PULL_REQUEST_COMMITS: usize = 250;
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct PublicationRecord {
    version: u64,
    key: String,
    digest: String,
    disposition: ReviewSubmissionDisposition,
}

#[derive(Deserialize)]
struct PullRequestCommit {
    sha: String,
}

struct Publication<'a> {
    review: &'a GithubReview,
    record: PublicationRecord,
}

#[derive(Clone, Copy)]
struct PublicationScope<'a> {
    app: &'a ConfiguredApp,
    repository: &'a Repository,
    number: ChangeRequestNumber,
    reviewer: &'a ReviewerApplication,
    key: &'a ReviewPublicationKey,
    digest: Option<&'a str>,
    submission: Option<&'a ReviewSubmission>,
}

impl GithubProvider {
    fn reviewer_app(&self, reviewer: &ReviewerApplication) -> Result<&ConfiguredApp> {
        let configured = self.configured_app(&reviewer.app().slug)?;
        if configured.app_id.to_string() != reviewer.app().id.as_str() {
            return Err(ProviderError::Configuration {
                origin: configured.source.clone(),
                reason: format!(
                    "named app {} has APP_ID {}, but the resolved reviewer has provider app ID {}",
                    reviewer.app().slug,
                    configured.app_id,
                    reviewer.app().id.as_str()
                ),
            });
        }
        Ok(configured)
    }

    fn matching_publication<'a>(
        &self,
        reviews: &'a [GithubReview],
        reviewer: &ReviewerApplication,
        key: &ReviewPublicationKey,
        expected_digest: Option<&str>,
        expected_submission: Option<&ReviewSubmission>,
    ) -> Result<Option<Publication<'a>>> {
        let mut matches = Vec::new();
        for review in reviews {
            if !same_reviewer(review, reviewer) {
                continue;
            }
            if review.user.as_ref().and_then(|user| user.kind.as_deref()) != Some("Bot") {
                return Err(reconciliation_error(
                    "exact-identity publication review author is not a bot",
                ));
            }
            let records = publication_records(&review.body)?;
            if records.is_empty() {
                if review.state == "PENDING"
                    && expected_submission.is_some_and(|submission| {
                        review.commit_id == submission.revision().head_sha
                            && review.body.starts_with(submission.summary())
                    })
                {
                    return Err(reconciliation_error(
                        "an exact-identity pending review matches the submission but has no publication record",
                    ));
                }
                continue;
            }
            if records.len() != 1 {
                return Err(reconciliation_error(format!(
                    "exact-identity review {} contains {} publication records",
                    review.node_id,
                    records.len()
                )));
            }
            for record in records {
                if record.key == key.as_str() {
                    matches.push(Publication { review, record });
                }
            }
        }

        let publication = match matches.len() {
            0 => return Ok(None),
            1 => matches.pop().expect("one publication"),
            count => {
                return Err(reconciliation_error(format!(
                    "found {count} exact-identity reviews carrying publication key {}",
                    key.as_str()
                )));
            }
        };
        validate_review_record(publication.review, &publication.record)?;
        if let Some(expected_digest) = expected_digest
            && publication.record.digest != expected_digest
        {
            return Err(ProviderError::InvalidInput {
                provider: "github",
                fact: format!(
                    "review publication key {} already identifies a different submission",
                    key.as_str()
                ),
            });
        }
        if let Some(submission) = expected_submission
            && (publication.review.commit_id != submission.revision().head_sha
                || publication.record.disposition != submission.disposition())
        {
            return Err(reconciliation_error(
                "matching publication record contradicts the submitted revision or disposition",
            ));
        }
        Ok(Some(publication))
    }

    async fn read_matching_publication(
        &self,
        scope: PublicationScope<'_>,
    ) -> Result<Option<OwnedPublication>> {
        let reviews = self
            .github_reviews_with(&scope.app.read, scope.repository, scope.number)
            .await?;
        self.matching_publication(
            &reviews,
            scope.reviewer,
            scope.key,
            scope.digest,
            scope.submission,
        )
        .map(|publication| publication.map(OwnedPublication::from))
    }

    async fn require_review_revision(
        &self,
        app: &ConfiguredApp,
        repository: &Repository,
        number: ChangeRequestNumber,
        revision: &str,
    ) -> Result<()> {
        let page: Page<PullRequestCommit> = app
            .read
            .get(
                format!("/repos/{repository}/pulls/{}/commits", number.get()),
                Some(&[("per_page", 100)]),
            )
            .await
            .map_err(|error| {
                if is_not_found(&error) {
                    ProviderError::NotFound {
                        entity: format!("change request {} in {repository}", number.get()),
                    }
                } else {
                    authenticated_external("read change request revisions", &error)
                }
            })?;
        let commits = app
            .read
            .all_pages(page)
            .await
            .map_err(|error| authenticated_external("read change request revisions", &error))?;
        if commits.iter().any(|commit| commit.sha == revision) {
            return Ok(());
        }
        if commits.len() >= MAX_OBSERVED_PULL_REQUEST_COMMITS {
            return Err(reconciliation_error(
                "GitHub capped the change request revision observation before the requested revision was found",
            ));
        }
        Err(ProviderError::NotFound {
            entity: format!(
                "revision {revision} on change request {} in {repository}",
                number.get()
            ),
        })
    }

    async fn submit_publication(
        &self,
        scope: PublicationScope<'_>,
        pending: &OwnedPublication,
    ) -> Result<ReviewId> {
        let event = github_event(pending.record.disposition);
        let path = format!(
            "/repos/{}/pulls/{}/reviews/{}/events",
            scope.repository,
            scope.number.get(),
            pending.numeric_id
        );
        let write = tokio::time::timeout(
            WRITE_TIMEOUT,
            scope
                .app
                .write
                .post::<_, serde_json::Value>(path, Some(&json!({ "event": event }))),
        )
        .await;
        let write_error = match write {
            Ok(Ok(_)) => None,
            Ok(Err(error)) => Some(authenticated_external("submit review", &error)),
            Err(_) => Some(reconciliation_error("submit review timed out")),
        };

        match self.read_matching_publication(scope).await? {
            Some(publication) if publication.review_id != pending.review_id => Err(
                reconciliation_error("GitHub reread a different review after submission"),
            ),
            Some(publication) if publication.is_submitted() => Ok(publication.review_id),
            Some(_) => Err(write_error.unwrap_or_else(|| {
                reconciliation_error("GitHub accepted review submission but reread it as pending")
            })),
            None => Err(write_error.unwrap_or_else(|| {
                reconciliation_error("GitHub accepted review submission but reread found no review")
            })),
        }
    }

    async fn finish_publication(
        &self,
        scope: PublicationScope<'_>,
        publication: OwnedPublication,
    ) -> Result<ReviewId> {
        if publication.is_submitted() {
            return Ok(publication.review_id);
        }
        self.submit_publication(scope, &publication).await
    }

    async fn reconcile_create(
        &self,
        scope: PublicationScope<'_>,
        create_error: ProviderError,
    ) -> Result<ReviewId> {
        match self.read_matching_publication(scope).await? {
            Some(publication) => self.finish_publication(scope, publication).await,
            None => Err(create_error),
        }
    }
}

#[async_trait]
impl ReviewPublishingProvider for GithubProvider {
    async fn publish_review(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        reviewer: &ReviewerApplication,
        submission: &ReviewSubmission,
    ) -> Result<ReviewId> {
        let app = self.reviewer_app(reviewer)?;
        validate_submission_summary(submission.summary())?;
        let digest = submission_digest(submission)?;
        let scope = PublicationScope {
            app,
            repository,
            number,
            reviewer,
            key: submission.publication_key(),
            digest: Some(&digest),
            submission: Some(submission),
        };
        if let Some(publication) = self.read_matching_publication(scope).await? {
            if publication.is_submitted() {
                return Ok(publication.review_id);
            }
            self.require_review_revision(app, repository, number, &submission.revision().head_sha)
                .await?;
            return self.finish_publication(scope, publication).await;
        }

        self.require_review_revision(app, repository, number, &submission.revision().head_sha)
            .await?;

        let record = PublicationRecord {
            version: PUBLICATION_VERSION,
            key: submission.publication_key().as_str().to_owned(),
            digest: digest.clone(),
            disposition: submission.disposition(),
        };
        let body = publication_body(submission.summary(), &record)?;
        let comments = submission
            .findings()
            .iter()
            .map(|finding| {
                json!({
                    "path": finding.path(),
                    "line": finding.line().get(),
                    "side": match finding.side() {
                        ReviewDiffSide::Left => "LEFT",
                        ReviewDiffSide::Right => "RIGHT",
                    },
                    "body": finding.body(),
                })
            })
            .collect::<Vec<_>>();
        let path = format!("/repos/{repository}/pulls/{}/reviews", number.get());
        let create = tokio::time::timeout(
            WRITE_TIMEOUT,
            app.write.post::<_, GithubReview>(
                path,
                Some(&json!({
                    "commit_id": submission.revision().head_sha,
                    "body": body,
                    "comments": comments,
                })),
            ),
        )
        .await;

        let created = match create {
            Ok(Ok(review)) => match owned_publication(
                review,
                reviewer,
                submission.publication_key(),
                &digest,
                submission,
            ) {
                Ok(publication) => publication,
                Err(response_error) => {
                    return self.reconcile_create(scope, response_error).await;
                }
            },
            Ok(Err(error)) => {
                let create_error = create_review_error(&error);
                return self.reconcile_create(scope, create_error).await;
            }
            Err(_) => {
                let create_error = reconciliation_error("create review timed out");
                return self.reconcile_create(scope, create_error).await;
            }
        };

        if !created.is_pending() {
            return Err(reconciliation_error(
                "GitHub create-review response was not pending",
            ));
        }
        self.submit_publication(scope, &created).await
    }

    async fn resume_review_publication(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        reviewer: &ReviewerApplication,
        key: &ReviewPublicationKey,
    ) -> Result<Option<ReviewId>> {
        let app = self.reviewer_app(reviewer)?;
        let scope = PublicationScope {
            app,
            repository,
            number,
            reviewer,
            key,
            digest: None,
            submission: None,
        };
        let Some(publication) = self.read_matching_publication(scope).await? else {
            return Ok(None);
        };
        self.finish_publication(scope, publication).await.map(Some)
    }
}

fn validate_submission_summary(summary: &str) -> Result<()> {
    if summary.contains(PUBLICATION_IDENTIFIER) {
        return Err(ProviderError::InvalidInput {
            provider: "github",
            fact: "review summary contains the reserved interprex:review-publication identifier"
                .to_owned(),
        });
    }
    Ok(())
}

#[derive(Clone)]
struct OwnedPublication {
    numeric_id: u64,
    review_id: ReviewId,
    state: String,
    record: PublicationRecord,
}

impl OwnedPublication {
    fn is_pending(&self) -> bool {
        self.state == "PENDING"
    }

    fn is_submitted(&self) -> bool {
        !self.is_pending()
    }
}

impl From<Publication<'_>> for OwnedPublication {
    fn from(publication: Publication<'_>) -> Self {
        Self {
            numeric_id: publication.review.id,
            review_id: ReviewId::new(publication.review.node_id.clone())
                .expect("validated review identifier"),
            state: publication.review.state.clone(),
            record: publication.record,
        }
    }
}

fn owned_publication(
    review: GithubReview,
    reviewer: &ReviewerApplication,
    key: &ReviewPublicationKey,
    digest: &str,
    submission: &ReviewSubmission,
) -> Result<OwnedPublication> {
    if !same_reviewer(&review, reviewer) {
        return Err(reconciliation_error(
            "GitHub create-review response has a different reviewer identity",
        ));
    }
    let mut records = publication_records(&review.body)?;
    if records.len() != 1 || records[0].key != key.as_str() {
        return Err(reconciliation_error(format!(
            "GitHub create-review response contains {} matching publication records",
            records
                .iter()
                .filter(|record| record.key == key.as_str())
                .count()
        )));
    }
    let record = records.pop().expect("one matching publication record");
    validate_review_record(&review, &record)?;
    if record.digest != digest
        || record.disposition != submission.disposition()
        || review.commit_id != submission.revision().head_sha
    {
        return Err(reconciliation_error(
            "GitHub create-review response contradicts the requested digest, revision, or disposition",
        ));
    }
    let review_id = ReviewId::new(review.node_id.clone())
        .map_err(|error| reconciliation_error(error.to_string()))?;
    Ok(OwnedPublication {
        numeric_id: review.id,
        review_id,
        state: review.state,
        record,
    })
}

fn same_reviewer(review: &GithubReview, reviewer: &ReviewerApplication) -> bool {
    // GitHub omits `performed_via_github_app` on pending reviews, so the bot
    // user is what identifies the application's actor; the app id is checked
    // only on the responses that carry it.
    review
        .user
        .as_ref()
        .is_some_and(|user| user.node_id == reviewer.bot().id.as_str())
        && review
            .performed_via_github_app
            .as_ref()
            .is_none_or(|app| app.id.to_string() == reviewer.app().id.as_str())
}

fn publication_records(body: &str) -> Result<Vec<PublicationRecord>> {
    let carrier_count = body.match_indices(PUBLICATION_CARRIER).count();
    let records = super::text_records::extract_records(body)
        .into_iter()
        .filter(|record| {
            record.namespace() == PUBLICATION_NAMESPACE && record.name() == PUBLICATION_NAME
        })
        .map(|record| {
            serde_json::from_value::<PublicationRecord>(record.value().clone()).map_err(|error| {
                reconciliation_error(format!("malformed publication record: {error}"))
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if records.len() != carrier_count {
        return Err(reconciliation_error(
            "malformed publication record carrier in exact-identity review",
        ));
    }
    for record in &records {
        validate_record(record)?;
    }
    Ok(records)
}

fn validate_record(record: &PublicationRecord) -> Result<()> {
    let digest = record.digest.strip_prefix("sha256:");
    if record.version != PUBLICATION_VERSION
        || record.key.trim().is_empty()
        || digest.is_none_or(|value| {
            value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        return Err(reconciliation_error(
            "publication record has an unsupported version, blank key, or invalid digest",
        ));
    }
    Ok(())
}

fn validate_review_record(review: &GithubReview, record: &PublicationRecord) -> Result<()> {
    if review.id == 0 || review.node_id.is_empty() || review.commit_id.trim().is_empty() {
        return Err(reconciliation_error(
            "publication review has an invalid identifier or revision",
        ));
    }
    match review.state.as_str() {
        "PENDING" if review.submitted_at.is_none() => Ok(()),
        "PENDING" => Err(reconciliation_error(
            "pending publication review has a submission time",
        )),
        state if state == github_state(record.disposition) && review.submitted_at.is_some() => {
            Ok(())
        }
        state if state == github_state(record.disposition) => Err(reconciliation_error(
            "submitted publication review has no submission time",
        )),
        state => Err(reconciliation_error(format!(
            "publication record intends {} but GitHub reports {state}",
            github_state(record.disposition)
        ))),
    }
}

fn publication_body(summary: &str, record: &PublicationRecord) -> Result<String> {
    let record = ProviderTextRecord::new(
        PUBLICATION_NAMESPACE,
        PUBLICATION_NAME,
        serde_json::to_value(record).map_err(|error| reconciliation_error(error.to_string()))?,
    )
    .map_err(|error| reconciliation_error(error.to_string()))?;
    Ok(super::text_records::embed_record(summary, &record))
}

fn submission_digest(submission: &ReviewSubmission) -> Result<String> {
    let bytes = serde_json::to_vec(submission)
        .map_err(|error| reconciliation_error(format!("serialize review submission: {error}")))?;
    Ok(format!("sha256:{:x}", Sha256::digest(bytes)))
}

fn create_review_error(error: &octocrab::Error) -> ProviderError {
    if let octocrab::Error::GitHub { source, .. } = error
        && source.status_code.as_u16() == 422
        && let Some(errors) = source.errors.as_deref()
        && !errors.is_empty()
        && let Some(mut fields) = errors
            .iter()
            .map(review_validation_field)
            .collect::<Option<Vec<_>>>()
    {
        fields.sort_unstable();
        fields.dedup();
        return ProviderError::InvalidInput {
            provider: "github",
            fact: format!("GitHub rejected these review fields: {}", fields.join(", ")),
        };
    }
    authenticated_external("create review", error)
}

fn review_validation_field(error: &serde_json::Value) -> Option<&'static str> {
    let resource = error.get("resource")?.as_str()?;
    let field = error.get("field")?.as_str()?;
    let code = error.get("code")?.as_str()?;
    if !matches!(
        code,
        "custom" | "invalid" | "missing" | "missing_field" | "unprocessable"
    ) {
        return None;
    }
    match (resource, field) {
        ("PullRequestReview", "body") => Some("body"),
        ("PullRequestReview", "comments") => Some("comments"),
        ("PullRequestReview", "commit_id") => Some("commit_id"),
        ("PullRequestReview", "event") => Some("event"),
        ("PullRequestReviewComment", "body") => Some("comments.body"),
        ("PullRequestReviewComment", "line") => Some("comments.line"),
        ("PullRequestReviewComment", "path") => Some("comments.path"),
        ("PullRequestReviewComment", "position") => Some("comments.position"),
        ("PullRequestReviewComment", "side") => Some("comments.side"),
        ("PullRequestReviewComment", "start_line") => Some("comments.start_line"),
        ("PullRequestReviewComment", "start_side") => Some("comments.start_side"),
        _ => None,
    }
}

fn github_event(disposition: ReviewSubmissionDisposition) -> &'static str {
    match disposition {
        ReviewSubmissionDisposition::Approved => "APPROVE",
        ReviewSubmissionDisposition::ChangesRequested => "REQUEST_CHANGES",
        ReviewSubmissionDisposition::Commented => "COMMENT",
    }
}

fn github_state(disposition: ReviewSubmissionDisposition) -> &'static str {
    match disposition {
        ReviewSubmissionDisposition::Approved => "APPROVED",
        ReviewSubmissionDisposition::ChangesRequested => "CHANGES_REQUESTED",
        ReviewSubmissionDisposition::Commented => "COMMENTED",
    }
}

fn reconciliation_error(message: impl Into<String>) -> ProviderError {
    ProviderError::External {
        provider: "github",
        operation: "reconcile review publication",
        message: message.into(),
    }
}
