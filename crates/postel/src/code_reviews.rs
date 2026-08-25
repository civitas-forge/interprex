use std::collections::BTreeSet;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{OpenClosed, Repository, Result};

platform_number!(CodeReviewNumber);

macro_rules! opaque_review_id {
    ($name:ident, $field:literal, $entity:literal) => {
        #[doc = concat!("Opaque provider identity for a ", $entity, ".")]
        ///
        /// Consumers retain this value only to address the same entity
        /// through the provider that returned it. Its representation has no
        /// provider-neutral meaning.
        #[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> std::result::Result<Self, crate::ModelError> {
                let value = value.into();
                if value.is_empty() {
                    return Err(crate::ModelError::Empty { field: $field });
                }
                Ok(Self(value))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

opaque_review_id!(
    ReviewSubmissionId,
    "review submission id",
    "review submission"
);
opaque_review_id!(ReviewThreadId, "review thread id", "review thread");
opaque_review_id!(ReviewCommentId, "review comment id", "review comment");
opaque_review_id!(ReviewRequestId, "review request id", "review request");
opaque_review_id!(ReviewActorId, "review actor id", "review actor");
opaque_review_id!(ReviewTeamId, "review team id", "review team");
opaque_review_id!(ReviewAppId, "review app id", "review app");

/// Two commit endpoints whose relationship is meaningful to the caller.
///
/// The endpoints do not assert ancestry; a force push can make them siblings.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct CommitRange {
    pub base_sha: String,
    pub head_sha: String,
}

/// The exact code revision attached to a formal review submission.
///
/// Some providers, including GitHub, retain the reviewed head commit but not
/// the base commit as it existed when a historical review was submitted.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ReviewedRevision {
    pub head_sha: String,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewActorKind {
    User,
    Bot,
    Placeholder,
    Organization,
    EnterpriseUser,
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ReviewActor {
    pub id: ReviewActorId,
    pub login: String,
    pub kind: ReviewActorKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewTeamKind {
    Organization,
    Enterprise,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewTeam {
    pub id: ReviewTeamId,
    pub slug: String,
    pub name: String,
    pub kind: ReviewTeamKind,
    pub request_identifier: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewTarget {
    Actor(ReviewActor),
    Team(ReviewTeam),
    Unavailable,
}

/// One provider address to add to the outstanding reviewer set.
///
/// User and bot values are logins. A team value is its canonical provider
/// identifier, such as `organization/team-slug` on GitHub. Targets observed
/// in a read can contain richer facts or unavailable identities, so writes use
/// this deliberately narrower shape.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRequestTarget {
    User(String),
    Bot(String),
    Team(String),
}

/// One outstanding request, including one whose target became unavailable.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewRequest {
    pub id: ReviewRequestId,
    pub target: ReviewTarget,
    pub as_code_owner: bool,
}

/// The GitHub App or equivalent provider application that produced a review.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewApp {
    pub id: ReviewAppId,
    pub slug: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDisposition {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewThreadStatus {
    Open,
    Resolved,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewComment {
    pub id: ReviewCommentId,
    pub author: ReviewActor,
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDiffSide {
    Left,
    Right,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewAnchor {
    File,
    DiffRange {
        side: ReviewDiffSide,
        start_side: Option<ReviewDiffSide>,
        line: Option<u64>,
        start_line: Option<u64>,
        original_line: Option<u64>,
        original_start_line: Option<u64>,
    },
}

/// The source anchor of an inline review thread.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewLocation {
    pub path: String,
    pub outdated: bool,
    pub anchor: ReviewAnchor,
}

/// One complete inline conversation on the code review.
///
/// A thread associated with a formal reviewer submission is a finding from
/// that submission. A thread without that association remains visible, which
/// preserves conversations initiated by the change author. Later comments are
/// replies and do not change the thread's origin.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewThread {
    pub id: ReviewThreadId,
    pub originating_submission: Option<ReviewSubmissionId>,
    pub location: ReviewLocation,
    pub status: ReviewThreadStatus,
    pub comment: ReviewComment,
    pub replies: Vec<ReviewComment>,
}

/// One formal review submission by one reviewer against one code revision.
///
/// Multiple submissions by the same reviewer are retained independently,
/// including multiple submissions against the same revision and submissions
/// without inline findings. Use [`CodeReview::findings_for`] to read the
/// threads that originated in a submission.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewSubmission {
    pub id: ReviewSubmissionId,
    pub reviewer: ReviewActor,
    pub app: Option<ReviewApp>,
    pub revision: ReviewedRevision,
    pub disposition: ReviewDisposition,
    pub submitted_at: DateTime<Utc>,
    pub summary: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CodeReview {
    pub number: CodeReviewNumber,
    pub title: String,
    pub state: OpenClosed,
    pub draft: bool,
    pub current_range: CommitRange,
    pub author: ReviewActor,
    pub updated_at: DateTime<Utc>,
    pub submissions: Vec<ReviewSubmission>,
    pub threads: Vec<ReviewThread>,
    pub outstanding_review_requests: Vec<ReviewRequest>,
}

impl CodeReview {
    fn submissions_by_time(&self) -> Vec<&ReviewSubmission> {
        let mut submissions = self.submissions.iter().enumerate().collect::<Vec<_>>();
        submissions.sort_by_key(|(index, submission)| (submission.submitted_at, *index));
        submissions
            .into_iter()
            .map(|(_, submission)| submission)
            .collect()
    }

    /// Reviewers in first-submission order. The change author is not a reviewer.
    #[must_use]
    pub fn reviewers(&self) -> Vec<&ReviewActor> {
        let mut seen = BTreeSet::new();
        self.submissions_by_time()
            .into_iter()
            .filter_map(|submission| {
                seen.insert(submission.reviewer.id.clone())
                    .then_some(&submission.reviewer)
            })
            .collect()
    }

    /// Findings created by one formal review submission, in provider order.
    pub fn findings_for<'a>(
        &'a self,
        id: &'a ReviewSubmissionId,
    ) -> impl Iterator<Item = &'a ReviewThread> + 'a {
        self.threads
            .iter()
            .filter(move |thread| thread.originating_submission.as_ref() == Some(id))
    }

    /// The one-based submission number for this reviewer.
    #[must_use]
    pub fn reviewer_round(&self, id: &ReviewSubmissionId) -> Option<usize> {
        let submission = self.submissions.iter().find(|item| &item.id == id)?;
        self.submissions_by_time()
            .into_iter()
            .filter(|item| item.reviewer.id == submission.reviewer.id)
            .position(|item| &item.id == id)
            .map(|index| index + 1)
    }

    /// Changes since this reviewer's previous formal submission.
    ///
    /// A first submission has no prior reviewed revision and therefore no
    /// reviewer-relative range.
    #[must_use]
    pub fn changes_since_previous_review(&self, id: &ReviewSubmissionId) -> Option<CommitRange> {
        let submissions = self.submissions_by_time();
        let position = submissions.iter().position(|item| &item.id == id)?;
        let submission = submissions[position];
        let previous = submissions[..position]
            .iter()
            .rev()
            .find(|item| item.reviewer.id == submission.reviewer.id)?;
        Some(CommitRange {
            base_sha: previous.revision.head_sha.clone(),
            head_sha: submission.revision.head_sha.clone(),
        })
    }

    /// The one-based code revision number, ordered by first reviewed revision.
    /// All submissions against the same reviewed head commit share this
    /// number.
    #[must_use]
    pub fn revision_round(&self, id: &ReviewSubmissionId) -> Option<usize> {
        let submission = self.submissions.iter().find(|item| &item.id == id)?;
        let mut revisions = Vec::new();
        for item in self.submissions_by_time() {
            if !revisions.contains(&item.revision) {
                revisions.push(item.revision.clone());
            }
            if &item.id == id {
                return revisions
                    .iter()
                    .position(|revision| revision == &submission.revision)
                    .map(|index| index + 1);
            }
        }
        None
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckConclusion {
    Success,
    Failure,
    Neutral,
    Cancelled,
    TimedOut,
    ActionRequired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckOutcome {
    pub name: String,
    pub head_sha: String,
    pub conclusion: CheckConclusion,
    pub summary: String,
}

#[async_trait]
pub trait CodeReviewsProvider: Send + Sync {
    /// Reads the code review and its complete submitted-review history.
    async fn code_review(
        &self,
        repository: &Repository,
        number: CodeReviewNumber,
    ) -> Result<CodeReview>;
    async fn resolve_thread(
        &self,
        repository: &Repository,
        number: CodeReviewNumber,
        thread_id: &ReviewThreadId,
    ) -> Result<()>;
    async fn request_reviewers(
        &self,
        repository: &Repository,
        number: CodeReviewNumber,
        reviewers: &[ReviewRequestTarget],
    ) -> Result<()>;
    async fn mark_ready(&self, repository: &Repository, number: CodeReviewNumber) -> Result<()>;
    async fn publish_check(
        &self,
        repository: &Repository,
        app_identity: &str,
        outcome: &CheckOutcome,
    ) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn actor(login: &str) -> ReviewActor {
        ReviewActor {
            id: ReviewActorId::new(format!("actor-{login}")).expect("actor id"),
            login: login.to_owned(),
            kind: ReviewActorKind::Bot,
        }
    }

    fn submission(id: &str, reviewer: &str, head: &str) -> ReviewSubmission {
        ReviewSubmission {
            id: ReviewSubmissionId::new(id).expect("submission id"),
            reviewer: actor(reviewer),
            app: None,
            revision: ReviewedRevision {
                head_sha: head.to_owned(),
            },
            disposition: ReviewDisposition::Commented,
            submitted_at: Utc.timestamp_opt(1, 0).single().expect("timestamp"),
            summary: None,
        }
    }

    #[test]
    fn rounds_are_derived_without_collapsing_review_submissions() {
        let review = CodeReview {
            number: CodeReviewNumber::new(355).expect("number"),
            title: "Review history".to_owned(),
            state: OpenClosed::Open,
            draft: false,
            current_range: CommitRange {
                base_sha: "base".to_owned(),
                head_sha: "revision-b".to_owned(),
            },
            author: ReviewActor {
                id: ReviewActorId::new("actor-author").expect("actor id"),
                login: "author".to_owned(),
                kind: ReviewActorKind::User,
            },
            updated_at: Utc.timestamp_opt(2, 0).single().expect("timestamp"),
            submissions: vec![
                submission("review-1", "codex", "revision-a"),
                submission("review-2", "agy", "revision-a"),
                submission("review-3", "agy", "revision-a"),
                submission("review-4", "codex", "revision-b"),
                submission("review-5", "agy", "revision-b"),
            ],
            threads: Vec::new(),
            outstanding_review_requests: Vec::new(),
        };

        assert_eq!(
            review
                .reviewers()
                .into_iter()
                .map(|reviewer| reviewer.login.as_str())
                .collect::<Vec<_>>(),
            ["codex", "agy"]
        );
        assert_eq!(
            review.reviewer_round(&ReviewSubmissionId::new("review-3").expect("id")),
            Some(2)
        );
        assert_eq!(
            review.revision_round(&ReviewSubmissionId::new("review-3").expect("id")),
            Some(1)
        );
        assert_eq!(
            review.revision_round(&ReviewSubmissionId::new("review-5").expect("id")),
            Some(2)
        );
        assert_eq!(
            review.changes_since_previous_review(&ReviewSubmissionId::new("review-4").expect("id")),
            Some(CommitRange {
                base_sha: "revision-a".to_owned(),
                head_sha: "revision-b".to_owned(),
            })
        );
    }

    #[test]
    fn author_threads_remain_visible_without_becoming_review_findings() {
        let reviewer = actor("reviewer");
        let author = ReviewActor {
            id: ReviewActorId::new("actor-author").expect("actor id"),
            login: "author".to_owned(),
            kind: ReviewActorKind::User,
        };
        let review_id = ReviewSubmissionId::new("review-1").expect("review id");
        let comment = |id: &str, actor: ReviewActor| ReviewComment {
            id: ReviewCommentId::new(id).expect("comment id"),
            author: actor,
            body: "comment".to_owned(),
            created_at: Utc.timestamp_opt(1, 0).single().expect("timestamp"),
            updated_at: Utc.timestamp_opt(1, 0).single().expect("timestamp"),
        };
        let thread = |id: &str, origin, initial| ReviewThread {
            id: ReviewThreadId::new(id).expect("thread id"),
            originating_submission: origin,
            location: ReviewLocation {
                path: "src/lib.rs".to_owned(),
                outdated: false,
                anchor: ReviewAnchor::DiffRange {
                    side: ReviewDiffSide::Right,
                    start_side: None,
                    line: Some(10),
                    start_line: None,
                    original_line: Some(10),
                    original_start_line: None,
                },
            },
            status: ReviewThreadStatus::Open,
            comment: initial,
            replies: Vec::new(),
        };
        let review = CodeReview {
            number: CodeReviewNumber::new(1).expect("number"),
            title: "Author conversation".to_owned(),
            state: OpenClosed::Open,
            draft: false,
            current_range: CommitRange {
                base_sha: "base".to_owned(),
                head_sha: "head".to_owned(),
            },
            author: author.clone(),
            updated_at: Utc.timestamp_opt(2, 0).single().expect("timestamp"),
            submissions: vec![submission("review-1", "reviewer", "head")],
            threads: vec![
                thread(
                    "reviewer-thread",
                    Some(review_id.clone()),
                    comment("comment-1", reviewer),
                ),
                thread("author-thread", None, comment("comment-2", author)),
            ],
            outstanding_review_requests: Vec::new(),
        };

        assert_eq!(review.threads.len(), 2);
        assert_eq!(review.findings_for(&review_id).count(), 1);
        assert!(review.threads[1].originating_submission.is_none());
    }

    #[test]
    fn reviewer_identity_and_time_determine_rounds() {
        let mut first = submission("review-1", "old-login", "revision-a");
        first.submitted_at = Utc.timestamp_opt(1, 0).single().expect("timestamp");
        let mut second = submission("review-2", "new-login", "revision-b");
        second.reviewer.id = first.reviewer.id.clone();
        second.submitted_at = Utc.timestamp_opt(2, 0).single().expect("timestamp");
        let review = CodeReview {
            number: CodeReviewNumber::new(1).expect("number"),
            title: "Renamed reviewer".to_owned(),
            state: OpenClosed::Open,
            draft: false,
            current_range: CommitRange {
                base_sha: "base".to_owned(),
                head_sha: "revision-b".to_owned(),
            },
            author: ReviewActor {
                id: ReviewActorId::new("actor-author").expect("actor id"),
                login: "author".to_owned(),
                kind: ReviewActorKind::User,
            },
            updated_at: Utc.timestamp_opt(3, 0).single().expect("timestamp"),
            submissions: vec![second.clone(), first],
            threads: Vec::new(),
            outstanding_review_requests: Vec::new(),
        };

        assert_eq!(review.reviewers().len(), 1);
        assert_eq!(review.reviewers()[0].login, "old-login");
        assert_eq!(review.reviewer_round(&second.id), Some(2));
        assert_eq!(
            review.changes_since_previous_review(&second.id),
            Some(CommitRange {
                base_sha: "revision-a".to_owned(),
                head_sha: "revision-b".to_owned(),
            })
        );
        assert_eq!(review.revision_round(&second.id), Some(2));
    }
}
