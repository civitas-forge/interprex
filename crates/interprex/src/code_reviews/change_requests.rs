use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{ChangeRequestHead, Review, ReviewActor, ReviewComment, ReviewRequest, ReviewThread};

platform_number!(ChangeRequestNumber);

/// Two commit endpoints whose relationship is meaningful to the caller.
///
/// The endpoints do not assert ancestry; a force push can make them siblings.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct CommitRange {
    pub base_sha: String,
    pub head_sha: String,
}

/// Whether a change request is open, closed without merging, or merged.
///
/// Platforms report merging separately from closing, so `Closed` states that
/// the change did not land and `Merged` carries the merge time the platform
/// recorded. A state Interprex does not model, such as a locked merge request,
/// is unrepresentable rather than reported as the nearest variant.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeRequestState {
    Open,
    Closed,
    Merged { merged_at: DateTime<Utc> },
}

/// Whether the platform can currently merge the change request's source into
/// its target branch.
///
/// This reports the platform's merge computation and nothing else. Required
/// checks, approvals and branch rules are separate facts, so a mergeable
/// change request can still be one the platform refuses to merge.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mergeability {
    /// The platform reports no conflict between the source and the target.
    Mergeable,
    /// The platform reports a conflict that a person must resolve.
    Conflicted,
    /// The platform published no answer. GitHub computes the merge after the
    /// read arrives and reports nothing until that finishes, so this is an
    /// observed platform state rather than a failure to read the fact.
    Unknown,
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
    pub state: ChangeRequestState,
    pub draft: bool,
    pub commit_range: CommitRange,
    /// The branch this change request targets, whose tip at observation time
    /// is `commit_range.base_sha`.
    ///
    /// The branch is named because a sha cannot identify it: branches share
    /// tips and advance between observations. Two open change requests
    /// proposing the same head differ by this branch, so a caller choosing
    /// among them reads a fact rather than inferring one.
    pub base_branch: String,
    /// The branch this change request proposes and the repository holding it,
    /// which is this repository or a fork of it.
    ///
    /// `None` when the provider no longer identifies where the branch lived,
    /// as GitHub reports for a change request whose fork was deleted. A branch
    /// name alone is not a head, so it is absent rather than paired with a
    /// guessed repository.
    pub head: Option<ChangeRequestHead>,
    pub mergeability: Mergeability,
    pub author: ReviewActor,
    pub updated_at: DateTime<Utc>,
    /// Platform reviews. Collection order carries no policy meaning.
    pub reviews: Vec<Review>,
    /// Inline threads that did not originate in a review.
    pub standalone_threads: Vec<ReviewThread>,
    /// Comments with no source location in stable, total provider order from
    /// earliest to latest. The provider resolves equal creation times using its
    /// native ordering value; consumers preserve this order instead of sorting
    /// opaque comment identifiers.
    pub unanchored_comments: Vec<ReviewComment>,
    /// The currently outstanding reviewer requests.
    pub outstanding_requests: Vec<ReviewRequest>,
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;
    use crate::{
        Repository, ReviewActorId, ReviewActorKind, ReviewAnchor, ReviewAuthor, ReviewCommentId,
        ReviewDiffSide, ReviewDisposition, ReviewFinding, ReviewId, ReviewLine, ReviewLineRange,
        ReviewLocation, ReviewState, ReviewThreadId, ReviewThreadStatus, ReviewedRevision,
    };

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
            state: ChangeRequestState::Open,
            draft: false,
            commit_range: CommitRange {
                base_sha: "base".to_owned(),
                head_sha: "head".to_owned(),
            },
            base_branch: "main".to_owned(),
            head: Some(
                ChangeRequestHead::new(
                    Repository::new("civitas-forge", "sandbox").expect("repository"),
                    "refs/heads/author-threads",
                )
                .expect("head"),
            ),
            mergeability: Mergeability::Mergeable,
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
    fn only_merged_change_requests_carry_a_merge_time() {
        let merged_at = Utc.timestamp_opt(3, 0).single().expect("timestamp");

        assert_eq!(
            serde_json::to_value(ChangeRequestState::Open).expect("serializes open state"),
            serde_json::json!("open")
        );
        assert_eq!(
            serde_json::to_value(ChangeRequestState::Closed).expect("serializes closed state"),
            serde_json::json!("closed")
        );
        assert_eq!(
            serde_json::to_value(ChangeRequestState::Merged { merged_at })
                .expect("serializes merged state"),
            serde_json::json!({ "merged": { "merged_at": "1970-01-01T00:00:03Z" } })
        );
        assert_ne!(
            ChangeRequestState::Closed,
            ChangeRequestState::Merged { merged_at }
        );
    }

    #[test]
    fn mergeability_keeps_an_uncomputed_merge_distinct_from_a_conflicted_one() {
        for (mergeability, expected) in [
            (Mergeability::Mergeable, "mergeable"),
            (Mergeability::Conflicted, "conflicted"),
            (Mergeability::Unknown, "unknown"),
        ] {
            assert_eq!(
                serde_json::to_value(mergeability).expect("serializes mergeability"),
                serde_json::json!(expected)
            );
        }
        assert_ne!(Mergeability::Unknown, Mergeability::Conflicted);
    }
}
