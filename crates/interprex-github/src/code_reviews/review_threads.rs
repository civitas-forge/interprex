use interprex::{
    ChangeRequestNumber, ProviderError, Repository, Result, ReviewAnchor, ReviewComment,
    ReviewCommentId, ReviewDiffSide, ReviewLine, ReviewLineRange, ReviewLocation,
};
use serde::Deserialize;
use serde_json::json;

use crate::{GithubProvider, client::external};

use super::actors::{actor, ghost_actor};
use super::{PageInfo, continuation_cursor};

const REVIEW_THREADS: &str = r#"
query ReviewThreads($owner: String!, $name: String!, $number: Int!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      reviewThreads(first: 100, after: $cursor) {
        nodes {
          id isResolved isOutdated path subjectType diffSide
          line startLine originalLine originalStartLine
          comments(first: 100) {
            nodes {
              id body createdAt updatedAt
              author {
                login __typename
                ... on Bot { id }
                ... on EnterpriseUserAccount { id }
                ... on Mannequin { id }
                ... on Organization { id }
                ... on User { id }
              }
              pullRequestReview { id }
            }
            pageInfo { hasNextPage endCursor }
          }
        }
        pageInfo { hasNextPage endCursor }
      }
    }
  }
}"#;

const REVIEW_THREAD_COMMENTS: &str = r#"
query ReviewThreadComments($threadId: ID!, $cursor: String) {
  node(id: $threadId) {
    ... on PullRequestReviewThread {
      comments(first: 100, after: $cursor) {
        nodes {
          id body createdAt updatedAt
          author {
            login __typename
            ... on Bot { id }
            ... on EnterpriseUserAccount { id }
            ... on Mannequin { id }
            ... on Organization { id }
            ... on User { id }
          }
          pullRequestReview { id }
        }
        pageInfo { hasNextPage endCursor }
      }
    }
  }
}"#;

#[derive(Deserialize)]
pub(super) struct ThreadsData {
    pub(super) repository: ThreadsRepository,
}

#[derive(Deserialize)]
pub(super) struct ThreadsRepository {
    #[serde(rename = "pullRequest")]
    pub(super) pull_request: ThreadsPullRequest,
}

#[derive(Deserialize)]
pub(super) struct ThreadsPullRequest {
    #[serde(rename = "reviewThreads")]
    pub(super) review_threads: ThreadConnection,
}

#[derive(Deserialize)]
pub(super) struct ThreadConnection {
    pub(super) nodes: Vec<ThreadNode>,
    #[serde(rename = "pageInfo")]
    pub(super) page_info: PageInfo,
}

#[derive(Deserialize, PartialEq)]
pub(super) struct ThreadNode {
    pub(super) id: String,
    #[serde(rename = "isResolved")]
    pub(super) resolved: bool,
    #[serde(rename = "isOutdated")]
    pub(super) outdated: bool,
    path: String,
    #[serde(rename = "subjectType")]
    subject_type: ThreadSubjectType,
    #[serde(rename = "diffSide")]
    diff_side: Option<GithubDiffSide>,
    line: Option<u64>,
    #[serde(rename = "startLine")]
    start_line: Option<u64>,
    #[serde(rename = "originalLine")]
    original_line: Option<u64>,
    #[serde(rename = "originalStartLine")]
    original_start_line: Option<u64>,
    pub(super) comments: CommentConnection,
}

#[derive(Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ThreadSubjectType {
    File,
    Line,
}

#[derive(Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum GithubDiffSide {
    Left,
    Right,
}

#[derive(Deserialize, PartialEq)]
pub(super) struct CommentConnection {
    pub(super) nodes: Vec<CommentNode>,
    #[serde(rename = "pageInfo")]
    pub(super) page_info: PageInfo,
}

#[derive(Clone, Deserialize, PartialEq)]
pub(super) struct CommentNode {
    pub(super) id: String,
    pub(super) body: String,
    #[serde(rename = "createdAt")]
    created_at: chrono::DateTime<chrono::Utc>,
    #[serde(rename = "updatedAt")]
    updated_at: chrono::DateTime<chrono::Utc>,
    author: Option<GraphqlActor>,
    #[serde(rename = "pullRequestReview")]
    pub(super) pull_request_review: Option<CommentReview>,
}

#[derive(Clone, Deserialize, PartialEq)]
struct GraphqlActor {
    id: String,
    login: String,
    #[serde(rename = "__typename")]
    kind: String,
}

#[derive(Clone, Deserialize, PartialEq)]
pub(super) struct CommentReview {
    pub(super) id: String,
}

#[derive(Deserialize)]
struct ThreadCommentsData {
    node: Option<ThreadCommentsNode>,
}

#[derive(Deserialize)]
struct ThreadCommentsNode {
    comments: CommentConnection,
}

fn normalize_line(value: u64) -> Result<ReviewLine> {
    ReviewLine::new(value).map_err(|error| ProviderError::Unrepresentable {
        provider: "github",
        fact: error.to_string(),
    })
}

fn normalize_diff_side(value: GithubDiffSide) -> ReviewDiffSide {
    match value {
        GithubDiffSide::Left => ReviewDiffSide::Left,
        GithubDiffSide::Right => ReviewDiffSide::Right,
    }
}

/// A range GitHub reports without an end line places nothing.
///
/// GitHub keeps a multi-line thread's `startLine` and drops its `line` once a
/// commit deletes the lines the thread ended at — the ordinary outcome of
/// acting on a finding by removing the code it named. The surviving start
/// names where the range began in a file that no longer has those lines, and
/// [`ReviewLineRange`] has no shape for a range with no end, so the thread
/// reads as one this side of the diff does not place. That is what a null
/// start and end already report for the same thread one commit later.
fn normalize_line_range(end: Option<u64>, start: Option<u64>) -> Result<Option<ReviewLineRange>> {
    let Some(end) = end else {
        return Ok(None);
    };
    Ok(Some(ReviewLineRange {
        start: start.map(normalize_line).transpose()?,
        end: normalize_line(end)?,
    }))
}

pub(super) fn normalize_review_location(thread: &ThreadNode) -> Result<ReviewLocation> {
    let anchor = match thread.subject_type {
        ThreadSubjectType::File => ReviewAnchor::File,
        ThreadSubjectType::Line => {
            let side = thread
                .diff_side
                .ok_or_else(|| ProviderError::Unrepresentable {
                    provider: "github",
                    fact: format!("line thread {} has no diff side", thread.id),
                })?;
            let original = normalize_line_range(thread.original_line, thread.original_start_line)?
                .ok_or_else(|| ProviderError::Unrepresentable {
                    provider: "github",
                    fact: format!("line thread {} has no original line", thread.id),
                })?;
            ReviewAnchor::Lines {
                side: normalize_diff_side(side),
                original,
                current: normalize_line_range(thread.line, thread.start_line)?,
            }
        }
    };
    Ok(ReviewLocation {
        path: thread.path.clone(),
        anchor,
    })
}

pub(super) fn normalize_comment(value: CommentNode) -> Result<ReviewComment> {
    let comment_id = value.id;
    Ok(ReviewComment {
        id: ReviewCommentId::new(comment_id.clone()).map_err(|error| {
            ProviderError::Unrepresentable {
                provider: "github",
                fact: error.to_string(),
            }
        })?,
        author: match value.author {
            Some(author) => actor(author.id, author.login, &author.kind)?,
            None => ghost_actor(format!("unavailable-comment-author:{comment_id}"))?,
        },
        body: value.body,
        created_at: value.created_at,
        updated_at: Some(value.updated_at),
    })
}

impl GithubProvider {
    async fn complete_thread_comments(&self, thread: &mut ThreadNode) -> Result<()> {
        while let Some(cursor) = continuation_cursor(
            &thread.comments.page_info,
            "read review thread comments",
            "review comments",
        )? {
            let data: ThreadCommentsData = self
                .user()?
                .graphql(&json!({
                    "query": REVIEW_THREAD_COMMENTS,
                    "variables": { "threadId": thread.id, "cursor": cursor }
                }))
                .await
                .map_err(|error| external("read review thread comments", error))?;
            let mut connection = data
                .node
                .ok_or_else(|| ProviderError::NotFound {
                    entity: format!("review thread {}", thread.id),
                })?
                .comments;
            thread.comments.nodes.append(&mut connection.nodes);
            thread.comments.page_info = connection.page_info;
        }
        Ok(())
    }

    pub(super) async fn github_review_threads(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
    ) -> Result<Vec<ThreadNode>> {
        let mut cursor: Option<String> = None;
        let mut threads = Vec::new();
        loop {
            let data: ThreadsData = self
                .user()?
                .graphql(&json!({
                    "query": REVIEW_THREADS,
                    "variables": {
                        "owner": repository.owner(),
                        "name": repository.name(),
                        "number": number.get(),
                        "cursor": cursor,
                    }
                }))
                .await
                .map_err(|error| external("read review threads", error))?;
            let connection = data.repository.pull_request.review_threads;
            let next_cursor = continuation_cursor(
                &connection.page_info,
                "read review threads",
                "review threads",
            )?;
            let mut page = connection.nodes;
            for thread in &mut page {
                self.complete_thread_comments(thread).await?;
            }
            threads.append(&mut page);
            let Some(next_cursor) = next_cursor else {
                return Ok(threads);
            };
            cursor = Some(next_cursor);
        }
    }
}

#[cfg(test)]
mod tests {
    use interprex::{ReviewAnchor, ReviewLineRange};

    use super::super::change_requests::{
        GithubPullRequest, GithubReview, normalize_change_request,
    };
    use super::*;

    #[test]
    fn a_range_whose_end_a_commit_deleted_places_the_thread_nowhere() {
        let mut threads = review_threads();
        threads[0].line = None;

        let change_request = normalize_change_request(
            pull_request(),
            reviews(),
            threads,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("a thread GitHub cannot place is still a thread");

        assert!(matches!(
            &change_request.reviews[0].findings[0].thread.location.anchor,
            ReviewAnchor::Lines {
                original: ReviewLineRange { start: Some(_), .. },
                current: None,
                ..
            }
        ));
    }

    fn pull_request() -> GithubPullRequest {
        serde_json::from_str(include_str!("../../tests/fixtures/pull_request.json"))
            .expect("pull request fixture")
    }

    fn reviews() -> Vec<GithubReview> {
        serde_json::from_str(include_str!(
            "../../tests/fixtures/code_review_reviews.json"
        ))
        .expect("review fixture")
    }

    fn review_threads() -> Vec<ThreadNode> {
        let threads: ThreadsData =
            serde_json::from_str(include_str!("../../tests/fixtures/review_threads.json"))
                .expect("thread fixture");
        threads.repository.pull_request.review_threads.nodes
    }
}
