use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{ModelError, OpenClosed, Repository, Result};

platform_number!(ChangeRequestNumber);
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

/// The exact code revision attached to a review.
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
    /// The provider address that can request this target again, when one is
    /// available. This is independent of the target's observed actor or team
    /// category.
    pub request_target: Option<ReviewRequestTarget>,
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
/// the change request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRelationship {
    ChangeAuthor,
    Other,
    Unknown,
}

/// The author of a review and the provider's knowledge of that actor's
/// relationship to the change request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewAuthor {
    ChangeAuthor,
    Other(ReviewActor),
    Unknown(ReviewActor),
}

impl ReviewAuthor {
    #[must_use]
    pub const fn relationship(&self) -> ReviewRelationship {
        match self {
            Self::ChangeAuthor => ReviewRelationship::ChangeAuthor,
            Self::Other(_) => ReviewRelationship::Other,
            Self::Unknown(_) => ReviewRelationship::Unknown,
        }
    }

    #[must_use]
    pub fn actor<'a>(&'a self, change_author: &'a ReviewActor) -> &'a ReviewActor {
        match self {
            Self::ChangeAuthor => change_author,
            Self::Other(actor) | Self::Unknown(actor) => actor,
        }
    }
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

/// The addressing user's assessment of a finding's effect on the change.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Critical,
    Major,
    Minor,
    Nit,
}

/// Why the addressing user considers a finding complete.
///
/// The variants and serialized spellings match GitHub's
/// `PullRequestReviewThreadResolutionReason` enum.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FindingResolutionReason {
    /// The review comment was addressed.
    Addressed,
    /// The review comment is invalid.
    Invalid,
    /// The review comment will not be addressed.
    WontFix,
}

/// The addressing user's recorded conclusion for one finding.
///
/// This is distinct from [`ReviewThreadStatus`]. A platform thread can have no
/// Interprex resolution because it was resolved outside this interface, and a
/// failed multi-request provider operation can leave a recorded conclusion on
/// a thread the platform still reports as open.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FindingResolution {
    pub reason: FindingResolutionReason,
    /// The severity assigned by the user addressing the finding. It need not
    /// match a severity stated by the reviewer.
    pub addressing_severity: FindingSeverity,
}

/// The nonblank visible explanation attached to a finding resolution.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct FindingResolutionReply(String);

impl FindingResolutionReply {
    pub fn new(value: impl Into<String>) -> std::result::Result<Self, ModelError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ModelError::Empty {
                field: "finding resolution reply",
            });
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for FindingResolutionReply {
    type Error = ModelError;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<FindingResolutionReply> for String {
    fn from(value: FindingResolutionReply) -> Self {
        value.0
    }
}

/// One observed finding resolution and the reply that recorded it.
///
/// The source reply identifier links to the addressing actor, explanation and
/// platform timestamps in the containing thread's replies.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "compatibility")]
pub enum FindingResolutionRecord {
    Supported {
        resolution: FindingResolution,
        source_reply_id: ReviewCommentId,
    },
    Unsupported {
        metadata_format: String,
        source_reply_id: ReviewCommentId,
    },
}

impl FindingResolutionRecord {
    #[must_use]
    pub fn supported_resolution(&self) -> Option<FindingResolution> {
        match self {
            Self::Supported { resolution, .. } => Some(*resolution),
            Self::Unsupported { .. } => None,
        }
    }

    #[must_use]
    pub fn source_reply_id(&self) -> &ReviewCommentId {
        match self {
            Self::Supported {
                source_reply_id, ..
            }
            | Self::Unsupported {
                source_reply_id, ..
            } => source_reply_id,
        }
    }
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

/// The stable source anchor within the file containing an inline review
/// thread. A line range records the location at which the thread began.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewAnchor {
    File,
    Lines {
        side: ReviewDiffSide,
        original: ReviewLineRange,
        current: Option<ReviewLineRange>,
    },
}

/// The file and anchor of an inline review thread.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewLocation {
    pub path: String,
    pub anchor: ReviewAnchor,
}

/// The facts shared by findings and standalone inline threads.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewThread {
    pub id: ReviewThreadId,
    pub location: ReviewLocation,
    pub outdated: bool,
    pub status: ReviewThreadStatus,
    pub comment: ReviewComment,
    pub replies: Vec<ReviewComment>,
}

/// One inline thread attached to the review in which it originated.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewFinding {
    #[serde(flatten)]
    pub thread: ReviewThread,
    pub resolution: Option<FindingResolutionRecord>,
}

impl std::ops::Deref for ReviewFinding {
    type Target = ReviewThread;

    fn deref(&self) -> &Self::Target {
        &self.thread
    }
}

impl std::ops::DerefMut for ReviewFinding {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.thread
    }
}

impl ReviewFinding {
    /// Returns the reply that records this finding's resolution.
    #[must_use]
    pub fn resolution_reply(&self) -> Option<&ReviewComment> {
        let reply_id = self.resolution.as_ref()?.source_reply_id();
        self.replies.iter().find(|reply| &reply.id == reply_id)
    }
}

/// One platform review, including drafts and reviews by the change author.
///
/// Multiple reviews by the same actor remain independent. Relationship is an
/// observed fact, not a decision about whether the review counts as independent
/// evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Review {
    pub id: ReviewId,
    pub author: ReviewAuthor,
    pub via_app: Option<ReviewApp>,
    pub revision: ReviewedRevision,
    pub state: ReviewState,
    pub summary: Option<String>,
    pub findings: Vec<ReviewFinding>,
}

/// One complete observation of a change request and its code-review data.
///
/// The provider completely paginates every declared collection and never
/// silently drops an entity it cannot normalize. Platforms need not provide a
/// transactional snapshot across independently mutable collections.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChangeRequest {
    pub number: ChangeRequestNumber,
    pub title: String,
    pub state: OpenClosed,
    pub draft: bool,
    pub commit_range: CommitRange,
    pub author: ReviewActor,
    pub updated_at: DateTime<Utc>,
    /// Platform reviews. Collection order carries no policy meaning.
    pub reviews: Vec<Review>,
    /// Inline threads that did not originate in a review.
    pub standalone_threads: Vec<ReviewThread>,
    /// Comments with no source location, in chronological order.
    pub unanchored_comments: Vec<ReviewComment>,
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
    /// Reads one complete observation of the change request.
    async fn change_request(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
    ) -> Result<ChangeRequest>;
    async fn resolve_thread(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        thread_id: &ReviewThreadId,
    ) -> Result<()>;
    /// Records why a finding is complete, records its assessed severity and
    /// marks its platform thread resolved.
    ///
    /// `reply` contains validated visible explanatory text. Providers may add
    /// their own visible or machine-readable representation around it.
    /// Providers whose platforms require more than one request can return an
    /// error after a partial write; a later observation preserves the platform
    /// thread state and any valid resolution record independently.
    ///
    /// Repeating an already recorded resolution does not add another reply. If
    /// that record exists while the platform thread is open, the repeated call
    /// only resolves the thread.
    async fn resolve_finding(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        thread_id: &ReviewThreadId,
        resolution: FindingResolution,
        reply: &FindingResolutionReply,
    ) -> Result<()>;
    /// Adds each target to the outstanding reviewer set.
    ///
    /// A target already present remains one request, so repeating the same call
    /// reaches the same observable state.
    async fn request_reviewers(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        reviewers: &[ReviewRequestTarget],
    ) -> Result<()>;
    async fn mark_ready(&self, repository: &Repository, number: ChangeRequestNumber) -> Result<()>;
    async fn publish_check(
        &self,
        repository: &Repository,
        app_name: &str,
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
            location: ReviewLocation {
                path: "src/lib.rs".to_owned(),
                anchor: ReviewAnchor::Lines {
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
            },
            outdated: false,
            status: ReviewThreadStatus::Open,
            comment: comment(&format!("comment-{id}"), author),
            replies: Vec::new(),
        }
    }

    fn review(id: &str, author: ReviewActor, findings: Vec<ReviewFinding>) -> Review {
        Review {
            id: ReviewId::new(id).expect("review id"),
            author: ReviewAuthor::Other(author),
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
    fn findings_and_standalone_threads_remain_structurally_distinct() {
        let reviewer = actor("reviewer");
        let author = ReviewActor {
            id: ReviewActorId::new("actor-author").expect("actor id"),
            login: "author".to_owned(),
            kind: ReviewActorKind::User,
        };
        let change_request = ChangeRequest {
            number: ChangeRequestNumber::new(1).expect("number"),
            title: "Author threads".to_owned(),
            state: OpenClosed::Open,
            draft: false,
            commit_range: CommitRange {
                base_sha: "base".to_owned(),
                head_sha: "head".to_owned(),
            },
            author: author.clone(),
            updated_at: Utc.timestamp_opt(2, 0).single().expect("timestamp"),
            reviews: vec![review(
                "review-1",
                reviewer.clone(),
                vec![ReviewFinding {
                    thread: thread("finding", reviewer),
                    resolution: None,
                }],
            )],
            standalone_threads: vec![thread("standalone", author)],
            unanchored_comments: Vec::new(),
            outstanding_requests: Vec::new(),
        };

        assert_eq!(change_request.reviews[0].findings.len(), 1);
        assert_eq!(change_request.standalone_threads.len(), 1);
    }

    #[test]
    fn finding_resolution_reasons_use_githubs_enum_spellings() {
        for (reason, expected) in [
            (FindingResolutionReason::Addressed, "ADDRESSED"),
            (FindingResolutionReason::Invalid, "INVALID"),
            (FindingResolutionReason::WontFix, "WONT_FIX"),
        ] {
            let resolution = FindingResolution {
                reason,
                addressing_severity: FindingSeverity::Major,
            };

            assert_eq!(
                serde_json::to_value(resolution).expect("serializes resolution"),
                serde_json::json!({
                    "reason": expected,
                    "addressing_severity": "major"
                })
            );
        }
    }

    #[test]
    fn finding_resolution_replies_require_visible_explanatory_text() {
        assert!(FindingResolutionReply::new("\n\t").is_err());
        let reply = FindingResolutionReply::new("Addressed in the current revision.")
            .expect("visible explanation");
        assert_eq!(reply.as_str(), "Addressed in the current revision.");
    }

    #[test]
    fn observed_target_kind_and_request_address_are_independent() {
        let organization_team = ReviewRequest {
            id: ReviewRequestId::new("request-organization").expect("request id"),
            target: ReviewTarget::Team(ReviewTeam {
                id: ReviewTeamId::new("team-organization").expect("team id"),
                slug: "maintainers".to_owned(),
                name: "Maintainers".to_owned(),
                kind: ReviewTeamKind::Organization,
            }),
            request_target: None,
            as_code_owner: false,
        };
        let enterprise_team = ReviewRequest {
            id: ReviewRequestId::new("request-enterprise").expect("request id"),
            target: ReviewTarget::Team(ReviewTeam {
                id: ReviewTeamId::new("team-enterprise").expect("team id"),
                slug: "security".to_owned(),
                name: "Security".to_owned(),
                kind: ReviewTeamKind::Enterprise,
            }),
            request_target: Some(ReviewRequestTarget::Team("security".to_owned())),
            as_code_owner: false,
        };

        assert_eq!(organization_team.request_target, None);
        assert_eq!(
            enterprise_team.request_target,
            Some(ReviewRequestTarget::Team("security".to_owned()))
        );
    }
}
