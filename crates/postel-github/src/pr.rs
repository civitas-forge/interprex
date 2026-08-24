//! Pull-request REST and GraphQL operations owned by the pr domain.
//!
//! Ordinary pull-request facts use REST. Review threads, thread resolution,
//! reviewer requests by login, and the draft-ready transition use hand-written
//! GraphQL because GitHub exposes provider-specific capabilities on those
//! routes. The documents live here, beside their normalization, so callers
//! never assemble GitHub requests or distinguish bot logins from user logins.

use async_trait::async_trait;
use postel_contracts::{PrDomain, ProviderError, Result};
use postel_model::{
    CheckConclusion, CheckOutcome, OpenClosed, PullRequest, PullRequestNumber, Repository,
    ReviewComment, ReviewThread, ReviewThreadId,
};
use serde::Deserialize;
use serde_json::json;

use crate::{GithubProvider, api::external};

const REVIEW_THREADS: &str = r#"
query ReviewThreads($owner: String!, $name: String!, $number: Int!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      reviewThreads(first: 100, after: $cursor) {
        nodes {
          id isResolved path line
          comments(first: 100) {
            nodes { body author { login } }
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
        nodes { body author { login } }
        pageInfo { hasNextPage endCursor }
      }
    }
  }
}"#;

const RESOLVE_THREAD: &str = r#"
mutation ResolveReviewThread($threadId: ID!) {
  resolveReviewThread(input: {threadId: $threadId}) { thread { id isResolved } }
}"#;

const MARK_READY: &str = r#"
mutation MarkReady($pullRequestId: ID!) {
  markPullRequestReadyForReview(input: {pullRequestId: $pullRequestId}) {
    pullRequest { id isDraft }
  }
}"#;

const REQUEST_REVIEWS_BY_LOGIN: &str = r#"
mutation RequestReviewsByLogin(
  $pullRequestId: ID!
  $userLogins: [String!]
  $botLogins: [String!]
) {
  requestReviewsByLogin(input: {
    pullRequestId: $pullRequestId
    userLogins: $userLogins
    botLogins: $botLogins
    union: true
  }) {
    pullRequest { id }
  }
}"#;

#[derive(Deserialize)]
struct GithubPullRequest {
    number: u64,
    node_id: String,
    title: String,
    state: String,
    draft: bool,
    head: GitRef,
    base: GitRef,
    user: GithubUser,
    updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
struct GitRef {
    sha: String,
}

#[derive(Deserialize)]
struct GithubUser {
    login: String,
}

fn normalize_pull_request(value: GithubPullRequest) -> Result<PullRequest> {
    Ok(PullRequest {
        number: PullRequestNumber::new(value.number).map_err(|error| ProviderError::External {
            provider: "github",
            operation: "normalize pull request",
            message: error.to_string(),
        })?,
        title: value.title,
        state: if value.state == "open" {
            OpenClosed::Open
        } else {
            OpenClosed::Closed
        },
        draft: value.draft,
        head_sha: value.head.sha,
        base_sha: value.base.sha,
        author: value.user.login,
        updated_at: value.updated_at,
    })
}

#[derive(Deserialize)]
struct ThreadsData {
    repository: ThreadsRepository,
}

#[derive(Deserialize)]
struct ThreadsRepository {
    #[serde(rename = "pullRequest")]
    pull_request: ThreadsPullRequest,
}

#[derive(Deserialize)]
struct ThreadsPullRequest {
    #[serde(rename = "reviewThreads")]
    review_threads: ThreadConnection,
}

#[derive(Deserialize)]
struct ThreadConnection {
    nodes: Vec<ThreadNode>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

#[derive(Default, Deserialize)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Deserialize)]
struct ThreadNode {
    id: String,
    #[serde(rename = "isResolved")]
    resolved: bool,
    path: Option<String>,
    line: Option<u64>,
    comments: CommentConnection,
}

#[derive(Deserialize)]
struct CommentConnection {
    nodes: Vec<CommentNode>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

#[derive(Deserialize)]
struct CommentNode {
    body: String,
    author: Option<GithubUser>,
}

#[derive(Deserialize)]
struct ThreadCommentsData {
    node: Option<ThreadCommentsNode>,
}

#[derive(Deserialize)]
struct ThreadCommentsNode {
    comments: CommentConnection,
}

fn continuation_cursor(
    page_info: &PageInfo,
    operation: &'static str,
    collection: &str,
) -> Result<Option<String>> {
    if !page_info.has_next_page {
        return Ok(None);
    }
    page_info
        .end_cursor
        .clone()
        .map(Some)
        .ok_or_else(|| ProviderError::External {
            provider: "github",
            operation,
            message: format!("GitHub reported another {collection} page without an end cursor"),
        })
}

fn normalize_threads(nodes: Vec<ThreadNode>) -> Result<Vec<ReviewThread>> {
    nodes
        .into_iter()
        .map(|thread| {
            Ok(ReviewThread {
                id: ReviewThreadId::new(thread.id).map_err(|error| ProviderError::External {
                    provider: "github",
                    operation: "normalize review thread",
                    message: error.to_string(),
                })?,
                resolved: thread.resolved,
                path: thread.path,
                line: thread.line,
                comments: thread
                    .comments
                    .nodes
                    .into_iter()
                    .map(|comment| ReviewComment {
                        body: comment.body,
                        author: comment
                            .author
                            .map_or_else(|| "ghost".to_owned(), |user| user.login),
                    })
                    .collect(),
            })
        })
        .collect()
}

fn conclusion(value: &CheckConclusion) -> &'static str {
    match value {
        CheckConclusion::Success => "success",
        CheckConclusion::Failure => "failure",
        CheckConclusion::Neutral => "neutral",
        CheckConclusion::Cancelled => "cancelled",
        CheckConclusion::TimedOut => "timed_out",
        CheckConclusion::ActionRequired => "action_required",
    }
}

impl GithubProvider {
    async fn github_pull_request(
        &self,
        repository: &Repository,
        number: PullRequestNumber,
    ) -> Result<GithubPullRequest> {
        self.user()?
            .get(
                format!("/repos/{repository}/pulls/{}", number.get()),
                None::<&()>,
            )
            .await
            .map_err(|error| {
                crate::api::read_error(
                    "read pull request",
                    format!("pull request {} in {repository}", number.get()),
                    error,
                )
            })
    }

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
}

#[async_trait]
impl PrDomain for GithubProvider {
    async fn pull_request(
        &self,
        repository: &Repository,
        number: PullRequestNumber,
    ) -> Result<PullRequest> {
        let response = self.github_pull_request(repository, number).await?;
        normalize_pull_request(response)
    }

    async fn review_threads(
        &self,
        repository: &Repository,
        number: PullRequestNumber,
    ) -> Result<Vec<ReviewThread>> {
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
            threads.extend(normalize_threads(page)?);
            let Some(next_cursor) = next_cursor else {
                return Ok(threads);
            };
            cursor = Some(next_cursor);
        }
    }

    async fn resolve_thread(
        &self,
        _repository: &Repository,
        _number: PullRequestNumber,
        thread_id: &ReviewThreadId,
    ) -> Result<()> {
        let _: serde_json::Value = self
            .user()?
            .graphql(&json!({
                "query": RESOLVE_THREAD,
                "variables": { "threadId": thread_id.as_str() }
            }))
            .await
            .map_err(|error| external("resolve review thread", error))?;
        Ok(())
    }

    async fn request_reviewers(
        &self,
        repository: &Repository,
        number: PullRequestNumber,
        reviewers: &[String],
    ) -> Result<()> {
        let pull_request = self.github_pull_request(repository, number).await?;
        let (bot_logins, user_logins): (Vec<&str>, Vec<&str>) = reviewers
            .iter()
            .map(String::as_str)
            .partition(|login| login.ends_with("[bot]"));
        let _: serde_json::Value = self
            .user()?
            .graphql(&json!({
                "query": REQUEST_REVIEWS_BY_LOGIN,
                "variables": {
                    "pullRequestId": pull_request.node_id,
                    "userLogins": user_logins,
                    "botLogins": bot_logins,
                }
            }))
            .await
            .map_err(|error| external("request pull request reviewers", error))?;
        Ok(())
    }

    async fn mark_ready(&self, repository: &Repository, number: PullRequestNumber) -> Result<()> {
        let pull_request = self.github_pull_request(repository, number).await?;
        let _: serde_json::Value = self
            .user()?
            .graphql(&json!({
                "query": MARK_READY,
                "variables": { "pullRequestId": pull_request.node_id }
            }))
            .await
            .map_err(|error| external("mark pull request ready", error))?;
        Ok(())
    }

    async fn publish_check(
        &self,
        repository: &Repository,
        app_identity: &str,
        outcome: &CheckOutcome,
    ) -> Result<()> {
        let _: serde_json::Value = self
            .app(app_identity)?
            .post(
                format!("/repos/{repository}/check-runs"),
                Some(&json!({
                    "name": outcome.name,
                    "head_sha": outcome.head_sha,
                    "status": "completed",
                    "conclusion": conclusion(&outcome.conclusion),
                    "output": { "title": outcome.name, "summary": outcome.summary }
                })),
            )
            .await
            .map_err(|error| external("publish pull request check", error))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use postel_contracts::PrDomain;
    use postel_model::{CheckConclusion, CheckOutcome, Repository};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
    };

    use crate::GithubProvider;

    use super::{GithubPullRequest, ThreadsData, normalize_pull_request, normalize_threads};

    #[test]
    fn pull_request_fixture_preserves_review_revision_facts() {
        let response: GithubPullRequest =
            serde_json::from_str(include_str!("../tests/fixtures/pull_request.json"))
                .expect("fixture");
        let pull_request = normalize_pull_request(response).expect("normalizes");
        assert_eq!(
            pull_request.head_sha,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert!(pull_request.draft);
    }

    #[test]
    fn graphql_fixture_normalizes_review_threads() {
        let response: ThreadsData =
            serde_json::from_str(include_str!("../tests/fixtures/review_threads.json"))
                .expect("fixture");
        let threads = normalize_threads(response.repository.pull_request.review_threads.nodes)
            .expect("normalize threads");
        assert_eq!(threads.len(), 2);
        assert_eq!(threads[0].id.as_str(), "PRRT_kwDOExample");
        assert_eq!(threads[0].comments.len(), 2);
        assert_eq!(threads[0].comments[0].author, "reviewer-bot");
        assert_eq!(threads[0].comments[1].author, "author");
        assert!(threads[1].comments.is_empty());
    }

    #[tokio::test]
    async fn app_only_check_uses_the_named_app_client() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("address");
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let count = stream.read(&mut buffer).await.expect("read request");
                request.extend_from_slice(&buffer[..count]);
                if count == 0 || String::from_utf8_lossy(&request).contains("\r\n\r\n{") {
                    break;
                }
            }
            stream
                .write_all(b"HTTP/1.1 201 Created\r\ncontent-type: application/json\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}")
                .await
                .expect("write response");
            sender.send(String::from_utf8(request).expect("UTF-8")).ok();
        });
        let client = octocrab::Octocrab::builder()
            .base_uri(format!("http://{address}"))
            .expect("base URI")
            .personal_token("app-installation-token")
            .build()
            .expect("client");
        let provider = GithubProvider {
            user: None,
            streaming_user: None,
            apps: BTreeMap::from([("automation".to_owned(), Arc::new(client))]),
        };
        let repository = Repository::new("faictor", "postel-sandbox").expect("repository");
        provider
            .publish_check(
                &repository,
                "automation",
                &CheckOutcome {
                    name: "reviewer".to_owned(),
                    head_sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
                    conclusion: CheckConclusion::Success,
                    summary: "settled".to_owned(),
                },
            )
            .await
            .expect("publish check");
        let request = receiver.await.expect("captured request");
        assert!(request.starts_with("POST /repos/faictor/postel-sandbox/check-runs "));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer app-installation-token")
        );
        assert!(request.contains("\"conclusion\":\"success\""));
    }
}
