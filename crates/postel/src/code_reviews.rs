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
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ReviewActor {
    pub login: String,
    pub kind: ReviewActorKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewTeam {
    pub id: String,
    pub slug: String,
    pub name: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewTarget {
    Actor(ReviewActor),
    Team(ReviewTeam),
}

/// One outstanding request for a user, bot or team to review the change.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewRequest {
    pub id: ReviewRequestId,
    pub target: ReviewTarget,
    pub as_code_owner: bool,
}

/// The GitHub App or equivalent provider application that produced a review.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewApp {
    pub id: String,
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

/// A source location as GitHub reports it after subsequent revisions.
///
/// `line` is the current line when the anchor still maps to the latest diff.
/// `original_line` retains the line selected when the finding was created.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewLocation {
    pub path: String,
    pub line: Option<u64>,
    pub original_line: Option<u64>,
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
    /// Reviewers in first-submission order. The change author is not a reviewer.
    #[must_use]
    pub fn reviewers(&self) -> Vec<&ReviewActor> {
        let mut seen = BTreeSet::new();
        self.submissions
            .iter()
            .filter_map(|submission| {
                seen.insert(submission.reviewer.clone())
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
        self.submissions
            .iter()
            .filter(|item| item.reviewer == submission.reviewer)
            .position(|item| &item.id == id)
            .map(|index| index + 1)
    }

    /// Changes since this reviewer's previous formal submission.
    ///
    /// A first submission has no prior reviewed revision and therefore no
    /// reviewer-relative range.
    #[must_use]
    pub fn changes_since_previous_review(&self, id: &ReviewSubmissionId) -> Option<CommitRange> {
        let position = self.submissions.iter().position(|item| &item.id == id)?;
        let submission = &self.submissions[position];
        let previous = self.submissions[..position]
            .iter()
            .rev()
            .find(|item| item.reviewer == submission.reviewer)?;
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
        for item in &self.submissions {
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
        reviewers: &[ReviewTarget],
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
                line: Some(10),
                original_line: Some(10),
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
}
