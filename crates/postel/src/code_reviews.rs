use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{OpenClosed, Repository, Result};

platform_number!(CodeReviewNumber);
platform_number!(ReviewLine);

macro_rules! opaque_review_id {
    ($name:ident, $field:literal, $entity:literal) => {
        #[doc = concat!("Opaque provider identifier for a ", $entity, ".")]
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

opaque_review_id!(ReviewId, "review id", "review");
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

/// The exact code revision attached to a submitted review.
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
    Organization { request_identifier: String },
    Enterprise,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewTeam {
    pub id: ReviewTeamId,
    pub slug: String,
    pub name: String,
    pub kind: ReviewTeamKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewTarget {
    Actor(ReviewActor),
    Team(ReviewTeam),
    Unavailable,
}

impl ReviewTarget {
    /// Returns the provider address that can request this observed target.
    ///
    /// Deleted identities, placeholders, organization actors and enterprise
    /// teams can remain visible even though the provider cannot request them
    /// again.
    #[must_use]
    pub fn request_target(&self) -> Option<ReviewRequestTarget> {
        match self {
            Self::Actor(actor) => match actor.kind {
                ReviewActorKind::User => Some(ReviewRequestTarget::User(actor.login.clone())),
                ReviewActorKind::Bot => Some(ReviewRequestTarget::Bot(actor.login.clone())),
                ReviewActorKind::Placeholder
                | ReviewActorKind::Organization
                | ReviewActorKind::EnterpriseUser => None,
            },
            Self::Team(team) => match &team.kind {
                ReviewTeamKind::Organization { request_identifier } => {
                    Some(ReviewRequestTarget::Team(request_identifier.clone()))
                }
                ReviewTeamKind::Enterprise => None,
            },
            Self::Unavailable => None,
        }
    }
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

/// What the provider can establish about a review author's relationship to
/// the proposed change.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRelationship {
    ChangeAuthor,
    Other,
    Unknown,
}

/// Whether a review is still a draft or has been submitted.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Draft,
    Submitted {
        disposition: ReviewDisposition,
        submitted_at: DateTime<Utc>,
    },
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
    /// The last known edit time, when the provider supplies one.
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewLineRange {
    pub start: Option<ReviewLine>,
    pub end: ReviewLine,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDiffSide {
    Left,
    Right,
}

/// The stable source anchor of an inline review thread.
///
/// A line range records the location at which the conversation began.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewLocation {
    File {
        path: String,
    },
    Lines {
        path: String,
        side: ReviewDiffSide,
        original: ReviewLineRange,
        current: Option<ReviewLineRange>,
    },
}

/// One complete inline conversation on the code review.
///
/// When nested in [`Review::findings`], this is a finding made in that review.
/// When nested in [`CodeReview::discussions`], it is an inline conversation
/// that did not originate in a review. Replies never change that placement.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewThread {
    pub id: ReviewThreadId,
    pub location: ReviewLocation,
    pub outdated: bool,
    pub status: ReviewThreadStatus,
    pub comment: ReviewComment,
    pub replies: Vec<ReviewComment>,
}

/// One platform review, including drafts and reviews by the change author.
///
/// Multiple reviews by the same actor remain independent. Relationship is an
/// observed fact, not a decision about whether the review counts as independent
/// evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Review {
    pub id: ReviewId,
    pub author: ReviewActor,
    pub relationship_to_change: ReviewRelationship,
    pub via_app: Option<ReviewApp>,
    pub revision: ReviewedRevision,
    pub state: ReviewState,
    pub summary: Option<String>,
    pub findings: Vec<ReviewThread>,
}

/// One complete observation of a proposed change and its code-review data.
///
/// The provider completely paginates every declared collection and never
/// silently drops an entity it cannot normalize. Platforms need not provide a
/// transactional snapshot across independently mutable collections.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CodeReview {
    pub number: CodeReviewNumber,
    pub title: String,
    pub state: OpenClosed,
    pub draft: bool,
    pub change: CommitRange,
    pub author: ReviewActor,
    pub updated_at: DateTime<Utc>,
    /// Platform reviews. Collection order carries no policy meaning.
    pub reviews: Vec<Review>,
    /// Inline conversations that did not originate in a review.
    pub discussions: Vec<ReviewThread>,
    /// General, non-inline conversation in chronological order.
    pub conversation: Vec<ReviewComment>,
    /// The currently outstanding reviewer requests.
    pub outstanding_requests: Vec<ReviewRequest>,
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
    /// Reads one complete observation of the code review.
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
    /// Adds each target to the outstanding reviewer set.
    ///
    /// A target already present remains one request, so repeating the same call
    /// reaches the same observable state.
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

    fn comment(id: &str, author: ReviewActor) -> ReviewComment {
        ReviewComment {
            id: ReviewCommentId::new(id).expect("comment id"),
            author,
            body: "comment".to_owned(),
            created_at: Utc.timestamp_opt(1, 0).single().expect("timestamp"),
            updated_at: Some(Utc.timestamp_opt(1, 0).single().expect("timestamp")),
        }
    }

    fn thread(id: &str, author: ReviewActor) -> ReviewThread {
        ReviewThread {
            id: ReviewThreadId::new(id).expect("thread id"),
            location: ReviewLocation::Lines {
                path: "src/lib.rs".to_owned(),
                side: ReviewDiffSide::Right,
                original: ReviewLineRange {
                    start: None,
                    end: ReviewLine::new(10).expect("line"),
                },
                current: Some(ReviewLineRange {
                    start: None,
                    end: ReviewLine::new(10).expect("line"),
                }),
            },
            outdated: false,
            status: ReviewThreadStatus::Open,
            comment: comment(&format!("comment-{id}"), author),
            replies: Vec::new(),
        }
    }

    fn review(id: &str, author: ReviewActor, findings: Vec<ReviewThread>) -> Review {
        Review {
            id: ReviewId::new(id).expect("review id"),
            author,
            relationship_to_change: ReviewRelationship::Other,
            via_app: None,
            revision: ReviewedRevision {
                head_sha: "head".to_owned(),
            },
            state: ReviewState::Submitted {
                disposition: ReviewDisposition::Commented,
                submitted_at: Utc.timestamp_opt(1, 0).single().expect("timestamp"),
            },
            summary: None,
            findings,
        }
    }

    #[test]
    fn findings_and_independent_discussions_remain_structurally_distinct() {
        let reviewer = actor("reviewer");
        let author = ReviewActor {
            id: ReviewActorId::new("actor-author").expect("actor id"),
            login: "author".to_owned(),
            kind: ReviewActorKind::User,
        };
        let code_review = CodeReview {
            number: CodeReviewNumber::new(1).expect("number"),
            title: "Author conversation".to_owned(),
            state: OpenClosed::Open,
            draft: false,
            change: CommitRange {
                base_sha: "base".to_owned(),
                head_sha: "head".to_owned(),
            },
            author: author.clone(),
            updated_at: Utc.timestamp_opt(2, 0).single().expect("timestamp"),
            reviews: vec![review(
                "review-1",
                reviewer.clone(),
                vec![thread("finding", reviewer)],
            )],
            discussions: vec![thread("discussion", author)],
            conversation: Vec::new(),
            outstanding_requests: Vec::new(),
        };

        assert_eq!(code_review.reviews[0].findings.len(), 1);
        assert_eq!(code_review.discussions.len(), 1);
    }

    #[test]
    fn observed_targets_expose_only_addresses_the_provider_can_request() {
        let user = ReviewTarget::Actor(ReviewActor {
            id: ReviewActorId::new("actor-user").expect("actor id"),
            login: "alice".to_owned(),
            kind: ReviewActorKind::User,
        });
        let enterprise_team = ReviewTarget::Team(ReviewTeam {
            id: ReviewTeamId::new("team-enterprise").expect("team id"),
            slug: "security".to_owned(),
            name: "Security".to_owned(),
            kind: ReviewTeamKind::Enterprise,
        });

        assert_eq!(
            user.request_target(),
            Some(ReviewRequestTarget::User("alice".to_owned()))
        );
        assert_eq!(enterprise_team.request_target(), None);
    }
}
