use std::collections::{BTreeMap, BTreeSet};

use interprex::{
    ChangeRequest, ChangeRequestHead, ChangeRequestNumber, ChangeRequestState, CommitRange,
    Mergeability, ProviderError, Repository, Result, Review, ReviewAuthor, ReviewComment,
    ReviewCommentId, ReviewDisposition, ReviewFinding, ReviewId, ReviewRelationship, ReviewState,
    ReviewThread, ReviewThreadId, ReviewThreadStatus, ReviewedRevision,
};
use octocrab::Page;
use serde::Deserialize;

use crate::{
    GithubProvider,
    client::{authenticated_external, external, is_not_found},
};

use super::actors::{GithubApp, GithubUser, ghost_actor, normalize_app, rest_actor};
use super::finding_resolutions::latest_finding_resolution;
use super::review_requests::{
    ReviewRequestNode, TimelineItemNode, normalize_review_request, outstanding_request_times,
};
use super::review_threads::{ThreadNode, normalize_comment, normalize_review_location};

#[derive(Deserialize)]
pub(super) struct GithubPullRequest {
    number: u64,
    pub(super) node_id: String,
    title: String,
    state: String,
    merged: bool,
    merged_at: Option<chrono::DateTime<chrono::Utc>>,
    draft: bool,
    /// GitHub computes the merge after the read arrives and reports `null`
    /// until that finishes. A response that carries no field at all states as
    /// little as `null` does, and reads the same way.
    mergeable: Option<bool>,
    head: GitRef,
    base: GitRef,
    user: Option<GithubUser>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
pub(super) struct GitRef {
    /// The branch name, which GitHub returns unqualified.
    #[serde(rename = "ref")]
    pub(super) branch: String,
    sha: String,
    /// The repository holding the branch, absent once GitHub stops
    /// identifying it, as for a change request whose fork was deleted.
    repo: Option<GithubRepositoryRef>,
}

#[derive(Deserialize)]
struct GithubRepositoryRef {
    full_name: String,
}

/// What a head listing reads from each pull request.
///
/// GitHub's `head` filter addresses an owner and a branch, so the repository
/// name comes back on each result and is compared here rather than assumed.
#[derive(Deserialize)]
pub(super) struct GithubPullRequestNumber {
    pub(super) number: u64,
    pub(super) head: GitRef,
}

#[derive(Deserialize, PartialEq)]
pub(super) struct GithubReview {
    pub(super) id: u64,
    pub(super) node_id: String,
    pub(super) user: Option<GithubUser>,
    pub(super) body: String,
    pub(super) state: String,
    pub(super) commit_id: String,
    pub(super) submitted_at: Option<chrono::DateTime<chrono::Utc>>,
    pub(super) performed_via_github_app: Option<GithubApp>,
}

#[derive(Deserialize, PartialEq)]
pub(super) struct GithubUnanchoredComment {
    id: u64,
    node_id: String,
    user: Option<GithubUser>,
    body: String,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

fn normalize_disposition(value: &str) -> Result<ReviewDisposition> {
    match value {
        "APPROVED" => Ok(ReviewDisposition::Approved),
        "CHANGES_REQUESTED" => Ok(ReviewDisposition::ChangesRequested),
        "COMMENTED" => Ok(ReviewDisposition::Commented),
        "DISMISSED" => Ok(ReviewDisposition::Dismissed),
        other => Err(ProviderError::Unrepresentable {
            provider: "github",
            fact: format!("unknown review state {other}"),
        }),
    }
}

fn normalize_unanchored_comment(value: GithubUnanchoredComment) -> Result<ReviewComment> {
    let comment_id = value.node_id;
    Ok(ReviewComment {
        id: ReviewCommentId::new(comment_id.clone()).map_err(|error| {
            ProviderError::Unrepresentable {
                provider: "github",
                fact: error.to_string(),
            }
        })?,
        author: match value.user {
            Some(author) => rest_actor(author)?,
            None => ghost_actor(format!(
                "unavailable-unanchored-comment-author:{comment_id}"
            ))?,
        },
        body: value.body,
        created_at: value.created_at,
        updated_at: Some(value.updated_at),
    })
}

/// Reads GitHub's `state`, `merged` and `merged_at` fields as one state.
///
/// GitHub reports a merge as a closed pull request carrying `merged` and a
/// merge time. Every other combination of the three fields contradicts itself,
/// and the provider refuses it rather than deciding which field to believe.
fn normalize_change_request_state(
    number: u64,
    state: &str,
    merged: bool,
    merged_at: Option<chrono::DateTime<chrono::Utc>>,
) -> Result<ChangeRequestState> {
    match (state, merged, merged_at) {
        ("open", false, None) => Ok(ChangeRequestState::Open),
        ("closed", false, None) => Ok(ChangeRequestState::Closed),
        ("closed", true, Some(merged_at)) => Ok(ChangeRequestState::Merged { merged_at }),
        ("closed", true, None) => Err(ProviderError::Unrepresentable {
            provider: "github",
            fact: format!("merged change request {number} has no merge time"),
        }),
        ("open" | "closed", false, Some(_)) => Err(ProviderError::Unrepresentable {
            provider: "github",
            fact: format!("change request {number} has a merge time but is not merged"),
        }),
        ("open", true, _) => Err(ProviderError::Unrepresentable {
            provider: "github",
            fact: format!("change request {number} is open and merged"),
        }),
        (other, _, _) => Err(ProviderError::Unrepresentable {
            provider: "github",
            fact: format!("unknown change request state {other}"),
        }),
    }
}

pub(super) fn normalize_change_request(
    value: GithubPullRequest,
    mut reviews: Vec<GithubReview>,
    threads: Vec<ThreadNode>,
    review_requests: Vec<ReviewRequestNode>,
    request_events: Vec<TimelineItemNode>,
    unanchored_comments: Vec<GithubUnanchoredComment>,
) -> Result<ChangeRequest> {
    let author_provider_id = value.user.as_ref().map(|user| user.node_id.clone());
    let author = match value.user {
        Some(user) => rest_actor(user)?,
        None => ghost_actor(format!("unavailable-change-author:{}", value.node_id))?,
    };
    let base_sha = value.base.sha;
    let base_branch = value.base.branch;
    let head = observed_head(&value.head)?;
    let mut review_positions = BTreeMap::new();
    let mut normalized_reviews = Vec::new();

    reviews.sort_by(|left, right| {
        left.submitted_at
            .is_none()
            .cmp(&right.submitted_at.is_none())
            .then_with(|| left.submitted_at.cmp(&right.submitted_at))
            .then_with(|| left.node_id.cmp(&right.node_id))
    });
    for review in reviews {
        let relationship = match (
            author_provider_id.as_deref(),
            review.user.as_ref().map(|user| user.node_id.as_str()),
        ) {
            (Some(author), Some(review_author)) if author == review_author => {
                ReviewRelationship::ChangeAuthor
            }
            (Some(_), Some(_)) => ReviewRelationship::Other,
            _ => ReviewRelationship::Unknown,
        };
        let review_author = match review.user {
            Some(user) => rest_actor(user)?,
            None => ghost_actor(format!("unavailable-review-author:{}", review.node_id))?,
        };
        let review_author = match relationship {
            ReviewRelationship::ChangeAuthor => ReviewAuthor::ChangeAuthor,
            ReviewRelationship::Other => ReviewAuthor::Other(review_author),
            ReviewRelationship::Unknown => ReviewAuthor::Unknown(review_author),
        };
        let state = if review.state == "PENDING" {
            if review.submitted_at.is_some() {
                return Err(ProviderError::Unrepresentable {
                    provider: "github",
                    fact: format!("draft review {} has a submission time", review.node_id),
                });
            }
            ReviewState::Draft
        } else {
            let submitted_at =
                review
                    .submitted_at
                    .ok_or_else(|| ProviderError::Unrepresentable {
                        provider: "github",
                        fact: format!("submitted review {} has no submission time", review.node_id),
                    })?;
            ReviewState::Submitted {
                disposition: normalize_disposition(&review.state)?,
                submitted_at,
            }
        };
        let id = ReviewId::new(review.node_id.clone()).map_err(|error| {
            ProviderError::Unrepresentable {
                provider: "github",
                fact: error.to_string(),
            }
        })?;
        review_positions.insert(review.node_id, normalized_reviews.len());
        normalized_reviews.push(Review {
            id,
            author: review_author,
            via_app: review
                .performed_via_github_app
                .map(normalize_app)
                .transpose()?,
            revision: ReviewedRevision {
                head_sha: review.commit_id,
            },
            state,
            summary: (!review.body.trim().is_empty()).then_some(review.body),
            findings: Vec::new(),
        });
    }

    let mut standalone_threads = Vec::new();
    for thread in threads {
        let location = normalize_review_location(&thread)?;
        let mut comments = thread.comments.nodes.into_iter();
        let initial = comments
            .next()
            .ok_or_else(|| ProviderError::Unrepresentable {
                provider: "github",
                fact: format!("review thread {} has no comments", thread.id),
            })?;
        let review_position = match initial.pull_request_review.as_ref() {
            None => None,
            Some(review) => match review_positions.get(&review.id) {
                Some(position) => Some(*position),
                None => {
                    return Err(ProviderError::Unrepresentable {
                        provider: "github",
                        fact: format!(
                            "review thread {} references missing review {}",
                            thread.id, review.id
                        ),
                    });
                }
            },
        };
        let replies = comments
            .map(normalize_comment)
            .collect::<Result<Vec<_>>>()?;
        let resolution = review_position
            .is_some()
            .then(|| latest_finding_resolution(&replies))
            .flatten();
        let normalized = ReviewThread {
            id: ReviewThreadId::new(thread.id).map_err(|error| ProviderError::Unrepresentable {
                provider: "github",
                fact: error.to_string(),
            })?,
            location,
            outdated: thread.outdated,
            status: if thread.resolved {
                ReviewThreadStatus::Resolved
            } else {
                ReviewThreadStatus::Open
            },
            comment: normalize_comment(initial)?,
            replies,
        };
        if let Some(position) = review_position {
            normalized_reviews[position].findings.push(ReviewFinding {
                thread: normalized,
                resolution,
            });
        } else {
            standalone_threads.push(normalized);
        }
    }

    let request_times = outstanding_request_times(&request_events);
    let outstanding_requests = review_requests
        .into_iter()
        .map(|request| normalize_review_request(request, &request_times))
        .collect::<Result<Vec<_>>>()?;
    let mut unanchored_comments = unanchored_comments;
    unanchored_comments.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let unanchored_comments = unanchored_comments
        .into_iter()
        .map(normalize_unanchored_comment)
        .collect::<Result<Vec<_>>>()?;

    Ok(ChangeRequest {
        number: ChangeRequestNumber::new(value.number).map_err(|error| {
            ProviderError::Unrepresentable {
                provider: "github",
                fact: error.to_string(),
            }
        })?,
        title: value.title,
        state: normalize_change_request_state(
            value.number,
            &value.state,
            value.merged,
            value.merged_at,
        )?,
        draft: value.draft,
        commit_range: CommitRange {
            base_sha,
            head_sha: value.head.sha,
        },
        base_branch,
        head,
        mergeability: match value.mergeable {
            Some(true) => Mergeability::Mergeable,
            Some(false) => Mergeability::Conflicted,
            None => Mergeability::Unknown,
        },
        author,
        updated_at: value.updated_at,
        reviews: normalized_reviews,
        standalone_threads,
        unanchored_comments,
        outstanding_requests,
    })
}

pub(super) fn thread_references_missing_review(
    reviews: &[GithubReview],
    threads: &[ThreadNode],
) -> bool {
    let review_ids = reviews
        .iter()
        .map(|review| review.node_id.as_str())
        .collect::<BTreeSet<_>>();
    threads.iter().any(|thread| {
        thread
            .comments
            .nodes
            .first()
            .and_then(|comment| comment.pull_request_review.as_ref())
            .is_some_and(|review| !review_ids.contains(review.id.as_str()))
    })
}

/// Reads the head GitHub reports for one pull request.
///
/// GitHub returns the branch unqualified and drops the repository once the
/// fork holding it is deleted. A branch without its repository is not a head,
/// so that observation is absent rather than paired with the targeted
/// repository, which did not hold the branch.
pub(super) fn observed_head(head: &GitRef) -> Result<Option<ChangeRequestHead>> {
    let Some(repository) = &head.repo else {
        return Ok(None);
    };
    let unrepresentable = |fact: String| ProviderError::Unrepresentable {
        provider: "github",
        fact,
    };
    let repository = repository
        .full_name
        .parse::<Repository>()
        .map_err(|error| {
            unrepresentable(format!("head repository {}: {error}", repository.full_name))
        })?;
    ChangeRequestHead::new(repository, &format!("refs/heads/{}", head.branch))
        .map(Some)
        .map_err(|error| unrepresentable(format!("head branch {}: {error}", head.branch)))
}

/// Writes a change request's head as GitHub's `head` pull-request filter.
///
/// The filter is `owner:branch`, naming where the branch lives rather than
/// which repository the change request targets. The two differ for a change
/// request proposed from a fork, so the owner comes from the head's own
/// repository.
pub(super) fn head_filter(head: &ChangeRequestHead) -> String {
    format!("{}:{}", head.repository().owner(), head.branch())
}

pub(super) fn number(value: u64) -> Result<ChangeRequestNumber> {
    ChangeRequestNumber::new(value).map_err(|error| ProviderError::Unrepresentable {
        provider: "github",
        fact: error.to_string(),
    })
}

impl GithubProvider {
    pub(super) async fn github_pull_request(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
    ) -> Result<GithubPullRequest> {
        self.user()?
            .get(
                format!("/repos/{repository}/pulls/{}", number.get()),
                None::<&()>,
            )
            .await
            .map_err(|error| {
                crate::client::read_error(
                    "read change request",
                    format!("change request {} in {repository}", number.get()),
                    error,
                )
            })
    }

    pub(super) async fn github_reviews(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
    ) -> Result<Vec<GithubReview>> {
        self.github_reviews_with(self.user()?, repository, number)
            .await
    }

    pub(super) async fn github_reviews_with(
        &self,
        client: &octocrab::Octocrab,
        repository: &Repository,
        number: ChangeRequestNumber,
    ) -> Result<Vec<GithubReview>> {
        let page: Page<GithubReview> = self.read_reviews_page(client, repository, number).await?;
        client
            .all_pages(page)
            .await
            .map_err(|error| authenticated_external("read reviews", &error))
    }

    async fn read_reviews_page(
        &self,
        client: &octocrab::Octocrab,
        repository: &Repository,
        number: ChangeRequestNumber,
    ) -> Result<Page<GithubReview>> {
        client
            .get(
                format!("/repos/{repository}/pulls/{}/reviews", number.get()),
                Some(&[("per_page", 100)]),
            )
            .await
            .map_err(|error| {
                if is_not_found(&error) {
                    ProviderError::NotFound {
                        entity: format!("change request {} in {repository}", number.get()),
                    }
                } else {
                    authenticated_external("read reviews", &error)
                }
            })
    }

    pub(super) async fn github_unanchored_comments(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
    ) -> Result<Vec<GithubUnanchoredComment>> {
        let page: Page<GithubUnanchoredComment> = self
            .user()?
            .get(
                format!("/repos/{repository}/issues/{}/comments", number.get()),
                Some(&[("per_page", 100)]),
            )
            .await
            .map_err(|error| external("read unanchored comments", error))?;
        self.user()?
            .all_pages(page)
            .await
            .map_err(|error| external("read unanchored comments", error))
    }
}

#[cfg(test)]
mod tests;
