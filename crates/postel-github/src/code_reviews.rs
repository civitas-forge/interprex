//! Code-review operations implemented with GitHub pull-request APIs.
//!
//! The read combines pull-request facts, formal review submissions and review
//! threads into one provider-neutral result. GitHub's REST review records
//! identify submissions and apps; GraphQL supplies thread resolution and the
//! complete comment sequence. The adapter joins them here so callers never
//! need to correlate GitHub review IDs with thread comments.

use std::collections::BTreeMap;

use async_trait::async_trait;
use octocrab::Page;
use postel::{
    CheckConclusion, CheckOutcome, CodeReview, CodeReviewNumber, CodeReviewsProvider, CommitRange,
    OpenClosed, ProviderError, Repository, Result, ReviewActor, ReviewActorId, ReviewActorKind,
    ReviewAnchor, ReviewApp, ReviewAppId, ReviewComment, ReviewCommentId, ReviewDiffSide,
    ReviewDisposition, ReviewLocation, ReviewRequest, ReviewRequestId, ReviewRequestTarget,
    ReviewSubmission, ReviewSubmissionId, ReviewTarget, ReviewTeam, ReviewTeamId, ReviewTeamKind,
    ReviewThread, ReviewThreadId, ReviewThreadStatus, ReviewedRevision,
};
use serde::Deserialize;
use serde_json::json;

use crate::{GithubProvider, client::external};

const REVIEW_THREADS: &str = r#"
query ReviewThreads($owner: String!, $name: String!, $number: Int!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      reviewThreads(first: 100, after: $cursor) {
        nodes {
          id isResolved isOutdated path subjectType diffSide startDiffSide
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

const REVIEW_REQUESTS: &str = r#"
query ReviewRequests($owner: String!, $name: String!, $number: Int!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      reviewRequests(first: 100, after: $cursor) {
        nodes {
          id asCodeOwner
          requestedReviewer {
            __typename
            ... on User { id login }
            ... on Bot { id login }
            ... on Mannequin { id login }
            ... on Team { id slug name organization { login } }
            ... on EnterpriseTeam { id slug name }
          }
        }
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
  $teamSlugs: [String!]
) {
  requestReviewsByLogin(input: {
    pullRequestId: $pullRequestId
    userLogins: $userLogins
    botLogins: $botLogins
    teamSlugs: $teamSlugs
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
    node_id: String,
    login: String,
    #[serde(rename = "type", default = "default_user_kind")]
    kind: String,
}

fn default_user_kind() -> String {
    "User".to_owned()
}

#[derive(Deserialize)]
struct GithubApp {
    id: u64,
    slug: String,
    name: String,
}

#[derive(Deserialize)]
struct GithubReview {
    node_id: String,
    user: Option<GithubUser>,
    body: String,
    state: String,
    commit_id: String,
    submitted_at: Option<chrono::DateTime<chrono::Utc>>,
    performed_via_github_app: Option<GithubApp>,
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
    #[serde(rename = "isOutdated")]
    outdated: bool,
    path: String,
    #[serde(rename = "subjectType")]
    subject_type: ThreadSubjectType,
    #[serde(rename = "diffSide")]
    diff_side: GithubDiffSide,
    #[serde(rename = "startDiffSide")]
    start_diff_side: Option<GithubDiffSide>,
    line: Option<u64>,
    #[serde(rename = "startLine")]
    start_line: Option<u64>,
    #[serde(rename = "originalLine")]
    original_line: Option<u64>,
    #[serde(rename = "originalStartLine")]
    original_start_line: Option<u64>,
    comments: CommentConnection,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum ThreadSubjectType {
    File,
    Line,
}

#[derive(Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum GithubDiffSide {
    Left,
    Right,
}

#[derive(Deserialize)]
struct CommentConnection {
    nodes: Vec<CommentNode>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

#[derive(Deserialize)]
struct CommentNode {
    id: String,
    body: String,
    #[serde(rename = "createdAt")]
    created_at: chrono::DateTime<chrono::Utc>,
    #[serde(rename = "updatedAt")]
    updated_at: chrono::DateTime<chrono::Utc>,
    author: Option<GraphqlActor>,
    #[serde(rename = "pullRequestReview")]
    pull_request_review: Option<CommentReview>,
}

#[derive(Deserialize)]
struct GraphqlActor {
    id: String,
    login: String,
    #[serde(rename = "__typename")]
    kind: String,
}

#[derive(Deserialize)]
struct CommentReview {
    id: String,
}

#[derive(Deserialize)]
struct ThreadCommentsData {
    node: Option<ThreadCommentsNode>,
}

#[derive(Deserialize)]
struct ThreadCommentsNode {
    comments: CommentConnection,
}

#[derive(Deserialize)]
struct ReviewRequestsData {
    repository: ReviewRequestsRepository,
}

#[derive(Deserialize)]
struct ReviewRequestsRepository {
    #[serde(rename = "pullRequest")]
    pull_request: ReviewRequestsPullRequest,
}

#[derive(Deserialize)]
struct ReviewRequestsPullRequest {
    #[serde(rename = "reviewRequests")]
    review_requests: ReviewRequestConnection,
}

#[derive(Deserialize)]
struct ReviewRequestConnection {
    nodes: Vec<ReviewRequestNode>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

#[derive(Deserialize)]
struct ReviewRequestNode {
    id: String,
    #[serde(rename = "asCodeOwner")]
    as_code_owner: bool,
    #[serde(rename = "requestedReviewer")]
    requested_reviewer: Option<RequestedReviewerNode>,
}

#[derive(Deserialize)]
#[serde(tag = "__typename")]
enum RequestedReviewerNode {
    User {
        id: String,
        login: String,
    },
    Bot {
        id: String,
        login: String,
    },
    Mannequin {
        id: String,
        login: String,
    },
    Team {
        id: String,
        slug: String,
        name: String,
        organization: RequestedReviewerOrganization,
    },
    EnterpriseTeam {
        id: String,
        slug: String,
        name: String,
    },
}

#[derive(Deserialize)]
struct RequestedReviewerOrganization {
    login: String,
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

fn actor(id: String, login: String, kind: &str) -> Result<ReviewActor> {
    let kind = match kind {
        "User" => ReviewActorKind::User,
        "Bot" => ReviewActorKind::Bot,
        "Mannequin" => ReviewActorKind::Placeholder,
        "Organization" => ReviewActorKind::Organization,
        "EnterpriseUserAccount" => ReviewActorKind::EnterpriseUser,
        other => {
            return Err(ProviderError::External {
                provider: "github",
                operation: "normalize review actor",
                message: format!("unknown review actor kind {other}"),
            });
        }
    };
    Ok(ReviewActor {
        id: ReviewActorId::new(id).map_err(|error| ProviderError::External {
            provider: "github",
            operation: "normalize review actor",
            message: error.to_string(),
        })?,
        login,
        kind,
    })
}

fn ghost_actor(id: String) -> Result<ReviewActor> {
    Ok(ReviewActor {
        id: ReviewActorId::new(id).map_err(|error| ProviderError::External {
            provider: "github",
            operation: "normalize unavailable review actor",
            message: error.to_string(),
        })?,
        login: "ghost".to_owned(),
        kind: ReviewActorKind::Placeholder,
    })
}

fn normalize_disposition(value: &str) -> Result<ReviewDisposition> {
    match value {
        "APPROVED" => Ok(ReviewDisposition::Approved),
        "CHANGES_REQUESTED" => Ok(ReviewDisposition::ChangesRequested),
        "COMMENTED" => Ok(ReviewDisposition::Commented),
        "DISMISSED" => Ok(ReviewDisposition::Dismissed),
        other => Err(ProviderError::External {
            provider: "github",
            operation: "normalize review submission",
            message: format!("unknown review state {other}"),
        }),
    }
}

fn normalize_diff_side(value: GithubDiffSide) -> ReviewDiffSide {
    match value {
        GithubDiffSide::Left => ReviewDiffSide::Left,
        GithubDiffSide::Right => ReviewDiffSide::Right,
    }
}

fn normalize_comment(value: CommentNode) -> Result<ReviewComment> {
    let comment_id = value.id;
    Ok(ReviewComment {
        id: ReviewCommentId::new(comment_id.clone()).map_err(|error| ProviderError::External {
            provider: "github",
            operation: "normalize review comment",
            message: error.to_string(),
        })?,
        author: match value.author {
            Some(author) => actor(author.id, author.login, &author.kind)?,
            None => ghost_actor(format!("unavailable-comment-author:{comment_id}"))?,
        },
        body: value.body,
        created_at: value.created_at,
        updated_at: value.updated_at,
    })
}

fn normalize_review_request(value: ReviewRequestNode) -> Result<ReviewRequest> {
    let target = match value.requested_reviewer {
        Some(RequestedReviewerNode::User { id, login }) => {
            ReviewTarget::Actor(actor(id, login, "User")?)
        }
        Some(RequestedReviewerNode::Bot { id, login }) => {
            ReviewTarget::Actor(actor(id, login, "Bot")?)
        }
        Some(RequestedReviewerNode::Mannequin { id, login }) => {
            ReviewTarget::Actor(actor(id, login, "Mannequin")?)
        }
        Some(RequestedReviewerNode::Team {
            id,
            slug,
            name,
            organization,
        }) => ReviewTarget::Team(ReviewTeam {
            id: ReviewTeamId::new(id).map_err(|error| ProviderError::External {
                provider: "github",
                operation: "normalize review request",
                message: error.to_string(),
            })?,
            request_identifier: Some(format!("{}/{}", organization.login, slug)),
            slug,
            name,
            kind: ReviewTeamKind::Organization,
        }),
        Some(RequestedReviewerNode::EnterpriseTeam { id, slug, name }) => {
            ReviewTarget::Team(ReviewTeam {
                id: ReviewTeamId::new(id).map_err(|error| ProviderError::External {
                    provider: "github",
                    operation: "normalize review request",
                    message: error.to_string(),
                })?,
                slug,
                name,
                kind: ReviewTeamKind::Enterprise,
                request_identifier: None,
            })
        }
        None => ReviewTarget::Unavailable,
    };
    Ok(ReviewRequest {
        id: ReviewRequestId::new(value.id).map_err(|error| ProviderError::External {
            provider: "github",
            operation: "normalize review request",
            message: error.to_string(),
        })?,
        target,
        as_code_owner: value.as_code_owner,
    })
}

fn normalize_code_review(
    value: GithubPullRequest,
    mut reviews: Vec<GithubReview>,
    threads: Vec<ThreadNode>,
    review_requests: Vec<ReviewRequestNode>,
) -> Result<CodeReview> {
    let author = actor(value.user.node_id, value.user.login, &value.user.kind)?;
    let base_sha = value.base.sha;
    let mut review_positions = BTreeMap::new();
    let mut submissions = Vec::new();

    reviews.sort_by_key(|review| review.submitted_at);
    for review in reviews {
        if review.state == "PENDING" {
            continue;
        }
        let reviewer = match review.user {
            Some(user) => actor(user.node_id, user.login, &user.kind)?,
            None => ghost_actor(format!("unavailable-reviewer:{}", review.node_id))?,
        };
        if reviewer.id == author.id {
            continue;
        }
        let submitted_at = review.submitted_at.ok_or_else(|| ProviderError::External {
            provider: "github",
            operation: "normalize review submission",
            message: format!("submitted review {} has no submission time", review.node_id),
        })?;
        let id = ReviewSubmissionId::new(review.node_id.clone()).map_err(|error| {
            ProviderError::External {
                provider: "github",
                operation: "normalize review submission",
                message: error.to_string(),
            }
        })?;
        review_positions.insert(review.node_id, submissions.len());
        submissions.push(ReviewSubmission {
            id,
            reviewer,
            app: review
                .performed_via_github_app
                .map(|app| {
                    Ok(ReviewApp {
                        id: ReviewAppId::new(app.id.to_string()).map_err(|error| {
                            ProviderError::External {
                                provider: "github",
                                operation: "normalize review app",
                                message: error.to_string(),
                            }
                        })?,
                        slug: app.slug,
                        name: app.name,
                    })
                })
                .transpose()?,
            revision: ReviewedRevision {
                head_sha: review.commit_id,
            },
            disposition: normalize_disposition(&review.state)?,
            submitted_at,
            summary: (!review.body.trim().is_empty()).then_some(review.body),
        });
    }

    let mut normalized_threads = Vec::with_capacity(threads.len());
    for thread in threads {
        let mut comments = thread.comments.nodes.into_iter();
        let initial = comments.next().ok_or_else(|| ProviderError::External {
            provider: "github",
            operation: "normalize review thread",
            message: format!("review thread {} has no comments", thread.id),
        })?;
        let originating_submission = initial
            .pull_request_review
            .as_ref()
            .map(|review| &review.id)
            .and_then(|review_id| review_positions.get(review_id))
            .map(|position| submissions[*position].id.clone());
        normalized_threads.push(ReviewThread {
            id: ReviewThreadId::new(thread.id).map_err(|error| ProviderError::External {
                provider: "github",
                operation: "normalize review thread",
                message: error.to_string(),
            })?,
            originating_submission,
            location: ReviewLocation {
                path: thread.path,
                outdated: thread.outdated,
                anchor: match thread.subject_type {
                    ThreadSubjectType::File => ReviewAnchor::File,
                    ThreadSubjectType::Line => ReviewAnchor::DiffRange {
                        side: normalize_diff_side(thread.diff_side),
                        start_side: thread.start_diff_side.map(normalize_diff_side),
                        line: thread.line,
                        start_line: thread.start_line,
                        original_line: thread.original_line,
                        original_start_line: thread.original_start_line,
                    },
                },
            },
            status: if thread.resolved {
                ReviewThreadStatus::Resolved
            } else {
                ReviewThreadStatus::Open
            },
            comment: normalize_comment(initial)?,
            replies: comments
                .map(normalize_comment)
                .collect::<Result<Vec<_>>>()?,
        });
    }

    let outstanding_review_requests = review_requests
        .into_iter()
        .map(normalize_review_request)
        .collect::<Result<Vec<_>>>()?;

    Ok(CodeReview {
        number: CodeReviewNumber::new(value.number).map_err(|error| ProviderError::External {
            provider: "github",
            operation: "normalize code review",
            message: error.to_string(),
        })?,
        title: value.title,
        state: if value.state == "open" {
            OpenClosed::Open
        } else {
            OpenClosed::Closed
        },
        draft: value.draft,
        current_range: CommitRange {
            base_sha,
            head_sha: value.head.sha,
        },
        author,
        updated_at: value.updated_at,
        submissions,
        threads: normalized_threads,
        outstanding_review_requests,
    })
}

fn same_code_review_version(left: &GithubPullRequest, right: &GithubPullRequest) -> bool {
    left.base.sha == right.base.sha
        && left.head.sha == right.head.sha
        && left.updated_at == right.updated_at
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
    async fn github_code_review(
        &self,
        repository: &Repository,
        number: CodeReviewNumber,
    ) -> Result<GithubPullRequest> {
        self.user()?
            .get(
                format!("/repos/{repository}/pulls/{}", number.get()),
                None::<&()>,
            )
            .await
            .map_err(|error| {
                crate::client::read_error(
                    "read code review",
                    format!("code review {} in {repository}", number.get()),
                    error,
                )
            })
    }

    async fn github_reviews(
        &self,
        repository: &Repository,
        number: CodeReviewNumber,
    ) -> Result<Vec<GithubReview>> {
        let page: Page<GithubReview> = self
            .user()?
            .get(
                format!("/repos/{repository}/pulls/{}/reviews", number.get()),
                Some(&[("per_page", 100)]),
            )
            .await
            .map_err(|error| external("read review submissions", error))?;
        self.user()?
            .all_pages(page)
            .await
            .map_err(|error| external("read review submissions", error))
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

    async fn github_review_threads(
        &self,
        repository: &Repository,
        number: CodeReviewNumber,
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

    async fn github_review_requests(
        &self,
        repository: &Repository,
        number: CodeReviewNumber,
    ) -> Result<Vec<ReviewRequestNode>> {
        let mut cursor: Option<String> = None;
        let mut requests = Vec::new();
        loop {
            let data: ReviewRequestsData = self
                .user()?
                .graphql(&json!({
                    "query": REVIEW_REQUESTS,
                    "variables": {
                        "owner": repository.owner(),
                        "name": repository.name(),
                        "number": number.get(),
                        "cursor": cursor,
                    }
                }))
                .await
                .map_err(|error| external("read review requests", error))?;
            let connection = data.repository.pull_request.review_requests;
            let next_cursor = continuation_cursor(
                &connection.page_info,
                "read review requests",
                "review requests",
            )?;
            requests.extend(connection.nodes);
            let Some(next_cursor) = next_cursor else {
                return Ok(requests);
            };
            cursor = Some(next_cursor);
        }
    }
}

#[async_trait]
impl CodeReviewsProvider for GithubProvider {
    async fn code_review(
        &self,
        repository: &Repository,
        number: CodeReviewNumber,
    ) -> Result<CodeReview> {
        let mut before = self.github_code_review(repository, number).await?;
        for _ in 0..2 {
            let reviews = self.github_reviews(repository, number).await?;
            let threads = self.github_review_threads(repository, number).await?;
            let requests = self.github_review_requests(repository, number).await?;
            let after = self.github_code_review(repository, number).await?;
            if same_code_review_version(&before, &after) {
                return normalize_code_review(after, reviews, threads, requests);
            }
            before = after;
        }
        Err(ProviderError::External {
            provider: "github",
            operation: "read code review",
            message: format!(
                "code review {} in {repository} changed during every read attempt",
                number.get()
            ),
        })
    }

    async fn resolve_thread(
        &self,
        _repository: &Repository,
        _number: CodeReviewNumber,
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
        number: CodeReviewNumber,
        reviewers: &[ReviewRequestTarget],
    ) -> Result<()> {
        let code_review = self.github_code_review(repository, number).await?;
        let mut user_logins = Vec::new();
        let mut bot_logins = Vec::new();
        let mut team_slugs = Vec::new();
        for reviewer in reviewers {
            match reviewer {
                ReviewRequestTarget::User(login) => user_logins.push(login.as_str()),
                ReviewRequestTarget::Bot(login) => bot_logins.push(if login.ends_with("[bot]") {
                    login.clone()
                } else {
                    format!("{login}[bot]")
                }),
                ReviewRequestTarget::Team(identifier) => team_slugs.push(identifier.as_str()),
            }
        }
        let _: serde_json::Value = self
            .user()?
            .graphql(&json!({
                "query": REQUEST_REVIEWS_BY_LOGIN,
                "variables": {
                    "pullRequestId": code_review.node_id,
                    "userLogins": user_logins,
                    "botLogins": bot_logins,
                    "teamSlugs": team_slugs,
                }
            }))
            .await
            .map_err(|error| external("request code reviewers", error))?;
        Ok(())
    }

    async fn mark_ready(&self, repository: &Repository, number: CodeReviewNumber) -> Result<()> {
        let code_review = self.github_code_review(repository, number).await?;
        let _: serde_json::Value = self
            .user()?
            .graphql(&json!({
                "query": MARK_READY,
                "variables": { "pullRequestId": code_review.node_id }
            }))
            .await
            .map_err(|error| external("mark code review ready", error))?;
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
            .map_err(|error| external("publish code review check", error))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use postel::{
        CheckConclusion, CheckOutcome, CodeReviewsProvider, ProviderError, Repository,
        ReviewActorKind, ReviewTarget, ReviewThreadStatus,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
    };

    use crate::GithubProvider;

    use super::{
        GithubPullRequest, GithubReview, ReviewRequestsData, ThreadsData, normalize_code_review,
    };

    #[test]
    fn github_fixtures_preserve_reviewers_rounds_findings_and_replies() {
        let code_review: GithubPullRequest =
            serde_json::from_str(include_str!("../tests/fixtures/pull_request.json"))
                .expect("code review fixture");
        let reviews: Vec<GithubReview> =
            serde_json::from_str(include_str!("../tests/fixtures/code_review_reviews.json"))
                .expect("review fixture");
        let threads: ThreadsData =
            serde_json::from_str(include_str!("../tests/fixtures/review_threads.json"))
                .expect("thread fixture");
        let requests: ReviewRequestsData =
            serde_json::from_str(include_str!("../tests/fixtures/review_requests.json"))
                .expect("review request fixture");
        let review = normalize_code_review(
            code_review,
            reviews,
            threads.repository.pull_request.review_threads.nodes,
            requests.repository.pull_request.review_requests.nodes,
        )
        .expect("normalizes");

        assert_eq!(review.submissions.len(), 9);
        assert_eq!(
            review
                .reviewers()
                .into_iter()
                .map(|actor| actor.login.as_str())
                .collect::<Vec<_>>(),
            [
                "adr-codex-review",
                "adr-agy-review",
                "copilot-pull-request-reviewer",
                "ghost",
                "ghost"
            ]
        );
        assert_eq!(
            review.submissions[1].revision,
            review.submissions[3].revision
        );
        assert_ne!(review.submissions[1].id, review.submissions[3].id);
        assert!(review.submissions[0].id.as_str().starts_with("PRR_"));
        let finding = review
            .findings_for(&review.submissions[0].id)
            .next()
            .expect("first submission finding");
        assert_eq!(finding.location.path, "docs/dev/architecture.lex");
        assert_eq!(
            finding.location.anchor,
            postel::ReviewAnchor::DiffRange {
                side: postel::ReviewDiffSide::Right,
                start_side: Some(postel::ReviewDiffSide::Right),
                line: Some(188),
                start_line: Some(184),
                original_line: Some(181),
                original_start_line: Some(177),
            }
        );
        assert!(finding.comment.id.as_str().starts_with("PRRC_"));
        assert_eq!(finding.replies.len(), 1);
        assert_eq!(finding.replies[0].author.login, "arthur-debert");
        assert_eq!(finding.status, ReviewThreadStatus::Resolved);
        assert_eq!(
            review.submissions[0]
                .app
                .as_ref()
                .map(|app| app.slug.as_str()),
            Some("adr-review")
        );
        let last_submission = review.submissions.last().expect("last review");
        assert!(review.findings_for(&last_submission.id).next().is_none());
        let unavailable = &review.submissions[7..9];
        assert_ne!(unavailable[0].reviewer.id, unavailable[1].reviewer.id);
        assert_eq!(review.reviewer_round(&unavailable[0].id), Some(1));
        assert_eq!(review.reviewer_round(&unavailable[1].id), Some(1));
        assert_eq!(review.threads.len(), 4);
        let author_thread = review
            .threads
            .iter()
            .find(|thread| thread.id.as_str() == "PRRT_kwDOSCkZoc6Author")
            .expect("author-started thread");
        assert!(author_thread.originating_submission.is_none());
        assert_eq!(author_thread.comment.author.login, "arthur-debert");
        assert_eq!(author_thread.replies[0].author.login, "adr-agy-review");
        assert_eq!(author_thread.location.anchor, postel::ReviewAnchor::File);
        assert_eq!(review.outstanding_review_requests.len(), 6);
        assert!(matches!(
            &review.outstanding_review_requests[0].target,
            ReviewTarget::Actor(actor)
                if actor.kind == ReviewActorKind::Bot
                    && actor.login == "copilot-pull-request-reviewer"
        ));
        assert!(review.outstanding_review_requests[1].as_code_owner);
        assert!(matches!(
            &review.outstanding_review_requests[2].target,
            ReviewTarget::Team(team)
                if team.slug == "maintainers"
                    && team.request_identifier.as_deref() == Some("faictor/maintainers")
        ));
        assert!(matches!(
            &review.outstanding_review_requests[3].target,
            ReviewTarget::Actor(actor) if actor.kind == ReviewActorKind::Placeholder
        ));
        assert!(matches!(
            &review.outstanding_review_requests[4].target,
            ReviewTarget::Team(team) if team.kind == postel::ReviewTeamKind::Enterprise
        ));
        assert_eq!(
            review.outstanding_review_requests[5].target,
            ReviewTarget::Unavailable
        );
    }

    #[test]
    fn submitted_reviews_refuse_unknown_actor_kinds() {
        let code_review: GithubPullRequest =
            serde_json::from_str(include_str!("../tests/fixtures/pull_request.json"))
                .expect("code review fixture");
        let mut reviews: Vec<GithubReview> =
            serde_json::from_str(include_str!("../tests/fixtures/code_review_reviews.json"))
                .expect("review fixture");
        reviews[0].user.as_mut().expect("reviewer").kind = "Repository".to_owned();

        let error = normalize_code_review(code_review, reviews, Vec::new(), Vec::new())
            .expect_err("unknown actor kind must be refused");
        assert!(matches!(
            error,
            ProviderError::External {
                operation: "normalize review actor",
                ..
            }
        ));
    }

    #[test]
    fn submitted_reviews_require_a_submission_time() {
        let code_review: GithubPullRequest =
            serde_json::from_str(include_str!("../tests/fixtures/pull_request.json"))
                .expect("code review fixture");
        let mut reviews: Vec<GithubReview> =
            serde_json::from_str(include_str!("../tests/fixtures/code_review_reviews.json"))
                .expect("review fixture");
        reviews[0].submitted_at = None;

        let error = normalize_code_review(code_review, reviews, Vec::new(), Vec::new())
            .expect_err("submitted review without time must be refused");
        assert!(matches!(
            error,
            ProviderError::External {
                operation: "normalize review submission",
                ..
            }
        ));
    }

    #[test]
    fn review_threads_require_an_initial_comment() {
        let code_review: GithubPullRequest =
            serde_json::from_str(include_str!("../tests/fixtures/pull_request.json"))
                .expect("code review fixture");
        let reviews: Vec<GithubReview> =
            serde_json::from_str(include_str!("../tests/fixtures/code_review_reviews.json"))
                .expect("review fixture");
        let mut threads: ThreadsData =
            serde_json::from_str(include_str!("../tests/fixtures/review_threads.json"))
                .expect("thread fixture");
        threads.repository.pull_request.review_threads.nodes[0]
            .comments
            .nodes
            .clear();

        let error = normalize_code_review(
            code_review,
            reviews,
            threads.repository.pull_request.review_threads.nodes,
            Vec::new(),
        )
        .expect_err("thread without an initial comment must be refused");
        assert!(matches!(
            error,
            ProviderError::External {
                operation: "normalize review thread",
                ..
            }
        ));
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
