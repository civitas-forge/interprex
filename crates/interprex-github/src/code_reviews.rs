//! Code-review operations implemented with GitHub pull-request APIs.
//!
//! The read combines pull-request facts, formal reviews, inline threads,
//! unanchored comments and outstanding requests into one provider-neutral
//! observation. GitHub's REST review and issue-comment records identify
//! reviews, apps and unanchored comments; GraphQL supplies thread locations,
//! resolution, complete comment sequences and outstanding requests. The
//! provider joins them here so callers never correlate GitHub entities.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use interprex::{
    ChangeRequest, ChangeRequestHead, ChangeRequestNumber, ChangeRequestState, CheckConclusion,
    CheckOutcome, CodeReviewsProvider, CommitRange, FindingResolution, FindingResolutionReason,
    FindingResolutionRecord, FindingResolutionReply, FindingSeverity, ProviderError, Repository,
    Result, Review, ReviewActor, ReviewActorId, ReviewActorKind, ReviewAnchor, ReviewApp,
    ReviewAppId, ReviewAuthor, ReviewComment, ReviewCommentId, ReviewDiffSide, ReviewDisposition,
    ReviewFinding, ReviewId, ReviewLine, ReviewLineRange, ReviewLocation, ReviewRelationship,
    ReviewRequest, ReviewRequestId, ReviewRequestTarget, ReviewState, ReviewTarget, ReviewTeam,
    ReviewTeamId, ReviewTeamKind, ReviewThread, ReviewThreadId, ReviewThreadStatus,
    ReviewedRevision,
};
use octocrab::Page;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{GithubProvider, client::external};

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

const REVIEW_REQUEST_TIMELINE: &str = r#"
query ReviewRequestTimeline($owner: String!, $name: String!, $number: Int!, $cursor: String) {
  repository(owner: $owner, name: $name) {
    pullRequest(number: $number) {
      timelineItems(
        first: 100
        after: $cursor
        itemTypes: [REVIEW_REQUESTED_EVENT, REVIEW_REQUEST_REMOVED_EVENT]
      ) {
        nodes {
          __typename
          ... on ReviewRequestedEvent {
            createdAt
            requestedReviewer {
              __typename
              ... on User { id }
              ... on Bot { id }
              ... on Mannequin { id }
              ... on Team { id }
              ... on EnterpriseTeam { id }
            }
          }
          ... on ReviewRequestRemovedEvent {
            requestedReviewer {
              __typename
              ... on User { id }
              ... on Bot { id }
              ... on Mannequin { id }
              ... on Team { id }
              ... on EnterpriseTeam { id }
            }
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

const ADD_THREAD_REPLY: &str = r#"
mutation AddPullRequestReviewThreadReply($threadId: ID!, $body: String!) {
  addPullRequestReviewThreadReply(
    input: {pullRequestReviewThreadId: $threadId, body: $body}
  ) { comment { id } }
}"#;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveThreadData {
    resolve_review_thread: ResolveThreadPayload,
}

#[derive(Deserialize)]
struct ResolveThreadPayload {
    thread: ResolvedThread,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolvedThread {
    id: String,
    is_resolved: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddThreadReplyData {
    add_pull_request_review_thread_reply: AddThreadReplyPayload,
}

#[derive(Deserialize)]
struct AddThreadReplyPayload {
    comment: AddedThreadReply,
}

#[derive(Deserialize)]
struct AddedThreadReply {
    id: String,
}

const FINDING_RESOLUTION_META_START: &str = "<!-- interprex:finding-resolution\n";
const FINDING_RESOLUTION_META_END: &str = "\n-->";
const FINDING_RESOLUTION_META_VERSION: u8 = 1;

#[derive(Deserialize, Serialize)]
struct GithubFindingResolution {
    version: u8,
    resolution_reason: FindingResolutionReason,
    addressing_severity: FindingSeverity,
}

fn severity_badge(severity: FindingSeverity) -> (&'static str, &'static str, &'static str) {
    match severity {
        FindingSeverity::Critical => ("critical", "Critical", "b60205"),
        FindingSeverity::Major => ("major", "Major", "d93f0b"),
        FindingSeverity::Minor => ("minor", "Minor", "fbca04"),
        FindingSeverity::Nit => ("nit", "Nit", "c5def5"),
    }
}

fn resolution_label(reason: FindingResolutionReason) -> &'static str {
    match reason {
        FindingResolutionReason::Addressed => "Addressed",
        FindingResolutionReason::Invalid => "Invalid",
        FindingResolutionReason::WontFix => "Won't fix",
    }
}

fn github_resolution_reply(resolution: FindingResolution, reply: &str) -> String {
    let (severity, severity_label, color) = severity_badge(resolution.addressing_severity);
    let resolution_label = resolution_label(resolution.reason);
    let visible =
        format!("**Resolution:** {resolution_label}  \n**Addressing severity:** {severity_label}");
    let badge = format!(
        "![Interprex severity: {severity}](https://img.shields.io/badge/severity-{severity}-{color})"
    );
    let metadata = GithubFindingResolution {
        version: FINDING_RESOLUTION_META_VERSION,
        resolution_reason: resolution.reason,
        addressing_severity: resolution.addressing_severity,
    };
    let metadata = serde_json::to_string(&metadata)
        .expect("the fixed finding-resolution metadata shape serializes");
    let marker = format!("{FINDING_RESOLUTION_META_START}{metadata}{FINDING_RESOLUTION_META_END}");
    [visible, badge, reply.to_owned(), marker]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[derive(Debug, Eq, PartialEq)]
enum ParsedFindingResolution {
    Absent,
    Supported(FindingResolution),
    UnsupportedVersion(u64),
}

fn finding_resolution(body: &str) -> ParsedFindingResolution {
    let body = body.trim_end();
    let Some(marker_start) = body.rfind(FINDING_RESOLUTION_META_START) else {
        return ParsedFindingResolution::Absent;
    };
    if !body.ends_with(FINDING_RESOLUTION_META_END) {
        return ParsedFindingResolution::Absent;
    }
    let metadata_start = marker_start + FINDING_RESOLUTION_META_START.len();
    let metadata_end = body.len() - FINDING_RESOLUTION_META_END.len();
    let Some(metadata) = body.get(metadata_start..metadata_end) else {
        return ParsedFindingResolution::Absent;
    };
    let Ok(metadata) = serde_json::from_str::<serde_json::Value>(metadata) else {
        return ParsedFindingResolution::Absent;
    };
    let Some(version) = metadata.get("version").and_then(serde_json::Value::as_u64) else {
        return ParsedFindingResolution::Absent;
    };
    if version != u64::from(FINDING_RESOLUTION_META_VERSION) {
        return ParsedFindingResolution::UnsupportedVersion(version);
    }
    let Ok(metadata) = serde_json::from_value::<GithubFindingResolution>(metadata) else {
        return ParsedFindingResolution::Absent;
    };
    ParsedFindingResolution::Supported(FindingResolution {
        reason: metadata.resolution_reason,
        addressing_severity: metadata.addressing_severity,
    })
}

fn latest_finding_resolution(replies: &[ReviewComment]) -> Option<FindingResolutionRecord> {
    for comment in replies.iter().rev() {
        match finding_resolution(&comment.body) {
            ParsedFindingResolution::Absent => {}
            ParsedFindingResolution::Supported(resolution) => {
                return Some(FindingResolutionRecord::Supported {
                    resolution,
                    source_reply_id: comment.id.clone(),
                });
            }
            ParsedFindingResolution::UnsupportedVersion(metadata_version) => {
                return Some(FindingResolutionRecord::Unsupported {
                    metadata_format: format!(
                        "github:interprex-finding-resolution:v{metadata_version}"
                    ),
                    source_reply_id: comment.id.clone(),
                });
            }
        }
    }
    None
}

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
    merged: bool,
    merged_at: Option<chrono::DateTime<chrono::Utc>>,
    draft: bool,
    head: GitRef,
    base: GitRef,
    user: Option<GithubUser>,
    updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize)]
struct GitRef {
    /// The branch name, which GitHub returns unqualified.
    #[serde(rename = "ref")]
    branch: String,
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
struct GithubPullRequestNumber {
    number: u64,
    head: GitRef,
}

#[derive(Deserialize, PartialEq)]
struct GithubUser {
    node_id: String,
    login: String,
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(Deserialize, PartialEq)]
struct GithubApp {
    id: u64,
    slug: String,
    name: String,
}

#[derive(Deserialize, PartialEq)]
struct GithubReview {
    node_id: String,
    user: Option<GithubUser>,
    body: String,
    state: String,
    commit_id: String,
    submitted_at: Option<chrono::DateTime<chrono::Utc>>,
    performed_via_github_app: Option<GithubApp>,
}

#[derive(Deserialize, PartialEq)]
struct GithubUnanchoredComment {
    node_id: String,
    user: Option<GithubUser>,
    body: String,
    created_at: chrono::DateTime<chrono::Utc>,
    updated_at: chrono::DateTime<chrono::Utc>,
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

#[derive(Default, Deserialize, PartialEq)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

#[derive(Deserialize, PartialEq)]
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
    diff_side: Option<GithubDiffSide>,
    line: Option<u64>,
    #[serde(rename = "startLine")]
    start_line: Option<u64>,
    #[serde(rename = "originalLine")]
    original_line: Option<u64>,
    #[serde(rename = "originalStartLine")]
    original_start_line: Option<u64>,
    comments: CommentConnection,
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
struct CommentConnection {
    nodes: Vec<CommentNode>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

#[derive(Clone, Deserialize, PartialEq)]
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

#[derive(Clone, Deserialize, PartialEq)]
struct GraphqlActor {
    id: String,
    login: String,
    #[serde(rename = "__typename")]
    kind: String,
}

#[derive(Clone, Deserialize, PartialEq)]
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

#[derive(Deserialize, PartialEq)]
struct ReviewRequestNode {
    id: String,
    #[serde(rename = "asCodeOwner")]
    as_code_owner: bool,
    #[serde(rename = "requestedReviewer")]
    requested_reviewer: Option<RequestedReviewerNode>,
}

#[derive(Deserialize, PartialEq)]
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

#[derive(Deserialize, PartialEq)]
struct RequestedReviewerOrganization {
    login: String,
}

#[derive(Deserialize)]
struct TimelineData {
    repository: TimelineRepository,
}

#[derive(Deserialize)]
struct TimelineRepository {
    #[serde(rename = "pullRequest")]
    pull_request: TimelinePullRequest,
}

#[derive(Deserialize)]
struct TimelinePullRequest {
    #[serde(rename = "timelineItems")]
    timeline_items: TimelineConnection,
}

#[derive(Deserialize)]
struct TimelineConnection {
    nodes: Vec<TimelineItemNode>,
    #[serde(rename = "pageInfo")]
    page_info: PageInfo,
}

/// The request and removal events selected by `REVIEW_REQUEST_TIMELINE`.
///
/// The query restricts `timelineItems` to these two types, so an item of any
/// other type fails to deserialize instead of being read as one of them.
#[derive(Deserialize)]
#[serde(tag = "__typename")]
enum TimelineItemNode {
    ReviewRequestedEvent {
        #[serde(rename = "createdAt")]
        created_at: chrono::DateTime<chrono::Utc>,
        #[serde(rename = "requestedReviewer")]
        requested_reviewer: Option<TimelineReviewerNode>,
    },
    /// A removal discards the request it superseded, so only the reviewer it
    /// names is read; where it sits in the sequence says when it happened.
    ReviewRequestRemovedEvent {
        #[serde(rename = "requestedReviewer")]
        requested_reviewer: Option<TimelineReviewerNode>,
    },
}

#[derive(Deserialize)]
#[serde(tag = "__typename")]
enum TimelineReviewerNode {
    User { id: String },
    Bot { id: String },
    Mannequin { id: String },
    Team { id: String },
    EnterpriseTeam { id: String },
}

/// The reviewer identity that joins an outstanding request to its events.
///
/// Actor and team identifiers are compared separately, so a user and a team
/// never match each other whatever their login and slug say.
#[derive(Eq, Ord, PartialEq, PartialOrd)]
enum ReviewTargetKey {
    Actor(String),
    Team(String),
}

impl TimelineReviewerNode {
    fn key(&self) -> ReviewTargetKey {
        match self {
            Self::User { id } | Self::Bot { id } | Self::Mannequin { id } => {
                ReviewTargetKey::Actor(id.clone())
            }
            Self::Team { id } | Self::EnterpriseTeam { id } => ReviewTargetKey::Team(id.clone()),
        }
    }
}

impl ReviewRequestNode {
    /// The identity to look up in the timeline, absent for a target GitHub no
    /// longer names.
    fn target_key(&self) -> Option<ReviewTargetKey> {
        match self.requested_reviewer.as_ref()? {
            RequestedReviewerNode::User { id, .. }
            | RequestedReviewerNode::Bot { id, .. }
            | RequestedReviewerNode::Mannequin { id, .. } => {
                Some(ReviewTargetKey::Actor(id.clone()))
            }
            RequestedReviewerNode::Team { id, .. }
            | RequestedReviewerNode::EnterpriseTeam { id, .. } => {
                Some(ReviewTargetKey::Team(id.clone()))
            }
        }
    }
}

/// The request time still in force for each reviewer identity.
///
/// GitHub returns timeline items in ascending creation order, so replaying
/// them leaves each identity holding its most recent request, and a removal
/// discards the request it superseded. A reviewer requested, removed and
/// requested again therefore reports the latest request.
fn outstanding_request_times(
    events: &[TimelineItemNode],
) -> BTreeMap<ReviewTargetKey, chrono::DateTime<chrono::Utc>> {
    let mut times = BTreeMap::new();
    for event in events {
        match event {
            TimelineItemNode::ReviewRequestedEvent {
                created_at,
                requested_reviewer: Some(reviewer),
            } => {
                times.insert(reviewer.key(), *created_at);
            }
            TimelineItemNode::ReviewRequestRemovedEvent {
                requested_reviewer: Some(reviewer),
            } => {
                times.remove(&reviewer.key());
            }
            TimelineItemNode::ReviewRequestedEvent {
                requested_reviewer: None,
                ..
            }
            | TimelineItemNode::ReviewRequestRemovedEvent {
                requested_reviewer: None,
            } => {}
        }
    }
    times
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
            return Err(ProviderError::Unrepresentable {
                provider: "github",
                fact: format!("unknown review actor kind {other}"),
            });
        }
    };
    Ok(ReviewActor {
        id: ReviewActorId::new(id).map_err(|error| ProviderError::Unrepresentable {
            provider: "github",
            fact: error.to_string(),
        })?,
        login,
        kind,
    })
}

fn rest_actor(user: GithubUser) -> Result<ReviewActor> {
    let kind = user.kind.ok_or_else(|| ProviderError::Unrepresentable {
        provider: "github",
        fact: format!("actor {} has no type", user.login),
    })?;
    actor(user.node_id, user.login, &kind)
}

fn ghost_actor(id: String) -> Result<ReviewActor> {
    Ok(ReviewActor {
        id: ReviewActorId::new(id).map_err(|error| ProviderError::Unrepresentable {
            provider: "github",
            fact: error.to_string(),
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
        other => Err(ProviderError::Unrepresentable {
            provider: "github",
            fact: format!("unknown review state {other}"),
        }),
    }
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

fn normalize_line_range(end: Option<u64>, start: Option<u64>) -> Result<Option<ReviewLineRange>> {
    let Some(end) = end else {
        if start.is_some() {
            return Err(ProviderError::Unrepresentable {
                provider: "github",
                fact: "review range has a start line without an end line".to_owned(),
            });
        }
        return Ok(None);
    };
    Ok(Some(ReviewLineRange {
        start: start.map(normalize_line).transpose()?,
        end: normalize_line(end)?,
    }))
}

fn normalize_review_location(thread: &ThreadNode) -> Result<ReviewLocation> {
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

fn normalize_comment(value: CommentNode) -> Result<ReviewComment> {
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

fn normalize_review_request(
    value: ReviewRequestNode,
    request_times: &BTreeMap<ReviewTargetKey, chrono::DateTime<chrono::Utc>>,
) -> Result<ReviewRequest> {
    let requested_at = value
        .target_key()
        .and_then(|key| request_times.get(&key).copied());
    let (target, request_target) = match value.requested_reviewer {
        Some(RequestedReviewerNode::User { id, login }) => (
            ReviewTarget::Actor(actor(id, login.clone(), "User")?),
            Some(ReviewRequestTarget::User(login)),
        ),
        Some(RequestedReviewerNode::Bot { id, login }) => (
            ReviewTarget::Actor(actor(id, login.clone(), "Bot")?),
            Some(ReviewRequestTarget::Bot(login)),
        ),
        Some(RequestedReviewerNode::Mannequin { id, login }) => {
            (ReviewTarget::Actor(actor(id, login, "Mannequin")?), None)
        }
        Some(RequestedReviewerNode::Team {
            id,
            slug,
            name,
            organization,
        }) => {
            let request_identifier = format!("{}/{}", organization.login, slug);
            (
                ReviewTarget::Team(ReviewTeam {
                    id: ReviewTeamId::new(id).map_err(|error| ProviderError::Unrepresentable {
                        provider: "github",
                        fact: error.to_string(),
                    })?,
                    slug,
                    name,
                    kind: ReviewTeamKind::Organization,
                }),
                Some(ReviewRequestTarget::Team(request_identifier)),
            )
        }
        Some(RequestedReviewerNode::EnterpriseTeam { id, slug, name }) => (
            ReviewTarget::Team(ReviewTeam {
                id: ReviewTeamId::new(id).map_err(|error| ProviderError::Unrepresentable {
                    provider: "github",
                    fact: error.to_string(),
                })?,
                slug,
                name,
                kind: ReviewTeamKind::Enterprise,
            }),
            None,
        ),
        None => (ReviewTarget::Unavailable, None),
    };
    Ok(ReviewRequest {
        id: ReviewRequestId::new(value.id).map_err(|error| ProviderError::Unrepresentable {
            provider: "github",
            fact: error.to_string(),
        })?,
        target,
        request_target,
        requested_at,
        as_code_owner: value.as_code_owner,
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

fn normalize_change_request(
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
                .map(|app| {
                    Ok(ReviewApp {
                        id: ReviewAppId::new(app.id.to_string()).map_err(|error| {
                            ProviderError::Unrepresentable {
                                provider: "github",
                                fact: error.to_string(),
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
    let mut unanchored_comments = unanchored_comments
        .into_iter()
        .map(normalize_unanchored_comment)
        .collect::<Result<Vec<_>>>()?;
    unanchored_comments.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });

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
        author,
        updated_at: value.updated_at,
        reviews: normalized_reviews,
        standalone_threads,
        unanchored_comments,
        outstanding_requests,
    })
}

fn thread_references_missing_review(reviews: &[GithubReview], threads: &[ThreadNode]) -> bool {
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
fn observed_head(head: &GitRef) -> Result<Option<ChangeRequestHead>> {
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
fn head_filter(head: &ChangeRequestHead) -> String {
    format!("{}:{}", head.repository().owner(), head.branch())
}

fn number(value: u64) -> Result<ChangeRequestNumber> {
    ChangeRequestNumber::new(value).map_err(|error| ProviderError::Unrepresentable {
        provider: "github",
        fact: error.to_string(),
    })
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

    async fn github_reviews(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
    ) -> Result<Vec<GithubReview>> {
        let page: Page<GithubReview> = self
            .user()?
            .get(
                format!("/repos/{repository}/pulls/{}/reviews", number.get()),
                Some(&[("per_page", 100)]),
            )
            .await
            .map_err(|error| external("read reviews", error))?;
        self.user()?
            .all_pages(page)
            .await
            .map_err(|error| external("read reviews", error))
    }

    async fn github_unanchored_comments(
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

    async fn github_review_requests(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
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

    async fn github_review_request_events(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
    ) -> Result<Vec<TimelineItemNode>> {
        let mut cursor: Option<String> = None;
        let mut events = Vec::new();
        loop {
            let data: TimelineData = self
                .user()?
                .graphql(&json!({
                    "query": REVIEW_REQUEST_TIMELINE,
                    "variables": {
                        "owner": repository.owner(),
                        "name": repository.name(),
                        "number": number.get(),
                        "cursor": cursor,
                    }
                }))
                .await
                .map_err(|error| external("read review request events", error))?;
            let connection = data.repository.pull_request.timeline_items;
            let next_cursor = continuation_cursor(
                &connection.page_info,
                "read review request events",
                "review request events",
            )?;
            events.extend(connection.nodes);
            let Some(next_cursor) = next_cursor else {
                return Ok(events);
            };
            cursor = Some(next_cursor);
        }
    }
}

#[async_trait]
impl CodeReviewsProvider for GithubProvider {
    async fn change_request(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
    ) -> Result<ChangeRequest> {
        let pull_request = self.github_pull_request(repository, number).await?;
        let mut reviews = self.github_reviews(repository, number).await?;
        let mut threads = self.github_review_threads(repository, number).await?;
        if thread_references_missing_review(&reviews, &threads) {
            reviews = self.github_reviews(repository, number).await?;
            threads = self.github_review_threads(repository, number).await?;
        }
        let requests = self.github_review_requests(repository, number).await?;
        // The timeline is a whole paginated read on its own and the request
        // times it carries describe outstanding requests only, so it is read
        // when at least one outstanding request names a reviewer to match.
        let request_events = if requests
            .iter()
            .any(|request| request.target_key().is_some())
        {
            self.github_review_request_events(repository, number)
                .await?
        } else {
            Vec::new()
        };
        let unanchored_comments = self.github_unanchored_comments(repository, number).await?;
        normalize_change_request(
            pull_request,
            reviews,
            threads,
            requests,
            request_events,
            unanchored_comments,
        )
    }

    async fn open_change_requests(
        &self,
        repository: &Repository,
        head: &ChangeRequestHead,
    ) -> Result<Vec<ChangeRequestNumber>> {
        let filter = head_filter(head);
        let page: Page<GithubPullRequestNumber> = self
            .user()?
            .get(
                format!("/repos/{repository}/pulls"),
                Some(&[
                    ("head", filter.as_str()),
                    ("state", "open"),
                    ("per_page", "100"),
                ]),
            )
            .await
            .map_err(|error| external("list open change requests", error))?;
        let listed = self
            .user()?
            .all_pages(page)
            .await
            .map_err(|error| external("list open change requests", error))?;
        let mut numbers = Vec::new();
        for pull_request in listed {
            match observed_head(&pull_request.head)? {
                Some(observed) if &observed == head => numbers.push(number(pull_request.number)?),
                // The filter addresses an owner and a branch, so another
                // repository of the same owner can answer it.
                Some(_) => {}
                None => {
                    return Err(ProviderError::Unrepresentable {
                        provider: "github",
                        fact: format!(
                            "change request {} proposes branch {} from a repository GitHub no longer identifies, so whether it proposes {} cannot be established",
                            pull_request.number,
                            pull_request.head.branch,
                            head.repository()
                        ),
                    });
                }
            }
        }
        Ok(numbers)
    }

    async fn resolve_thread(
        &self,
        _repository: &Repository,
        _number: ChangeRequestNumber,
        thread_id: &ReviewThreadId,
    ) -> Result<()> {
        let response: ResolveThreadData = self
            .user()?
            .graphql(&json!({
                "query": RESOLVE_THREAD,
                "variables": { "threadId": thread_id.as_str() }
            }))
            .await
            .map_err(|error| external("resolve review thread", error))?;
        let resolved = response.resolve_review_thread.thread;
        if resolved.id == thread_id.as_str() && resolved.is_resolved {
            Ok(())
        } else {
            Err(ProviderError::External {
                provider: "github",
                operation: "resolve review thread",
                message: format!(
                    "GitHub returned thread {} with isResolved={} for requested thread {}",
                    resolved.id,
                    resolved.is_resolved,
                    thread_id.as_str()
                ),
            })
        }
    }

    async fn resolve_finding(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        thread_id: &ReviewThreadId,
        resolution: FindingResolution,
        reply: &FindingResolutionReply,
    ) -> Result<()> {
        let change_request = self.change_request(repository, number).await?;
        let finding = change_request
            .reviews
            .iter()
            .flat_map(|review| &review.findings)
            .find(|finding| &finding.id == thread_id)
            .ok_or_else(|| ProviderError::NotFound {
                entity: format!("finding thread {}", thread_id.as_str()),
            })?;
        if let Some(FindingResolutionRecord::Unsupported {
            metadata_format, ..
        }) = &finding.resolution
        {
            return Err(ProviderError::Unrepresentable {
                provider: "github",
                fact: format!(
                    "finding thread {} contains unsupported resolution metadata format {metadata_format}",
                    thread_id.as_str()
                ),
            });
        }
        if matches!(
            &finding.resolution,
            Some(FindingResolutionRecord::Supported {
                resolution: recorded,
                ..
            }) if *recorded == resolution
        ) {
            return if finding.status == ReviewThreadStatus::Resolved {
                Ok(())
            } else {
                self.resolve_thread(repository, number, thread_id).await
            };
        }
        let already_resolved = finding.status == ReviewThreadStatus::Resolved;
        let body = github_resolution_reply(resolution, reply.as_str());
        let response: AddThreadReplyData = self
            .user()?
            .graphql(&json!({
                "query": ADD_THREAD_REPLY,
                "variables": { "threadId": thread_id.as_str(), "body": body }
            }))
            .await
            .map_err(|error| external("record finding resolution", error))?;
        if response
            .add_pull_request_review_thread_reply
            .comment
            .id
            .is_empty()
        {
            return Err(ProviderError::External {
                provider: "github",
                operation: "record finding resolution",
                message: "GitHub returned an empty reply identifier".to_owned(),
            });
        }
        if already_resolved {
            Ok(())
        } else {
            self.resolve_thread(repository, number, thread_id).await
        }
    }

    async fn request_reviewers(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        reviewers: &[ReviewRequestTarget],
    ) -> Result<()> {
        let pull_request = self.github_pull_request(repository, number).await?;
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
                    "pullRequestId": pull_request.node_id,
                    "userLogins": user_logins,
                    "botLogins": bot_logins,
                    "teamSlugs": team_slugs,
                }
            }))
            .await
            .map_err(|error| external("request code reviewers", error))?;
        Ok(())
    }

    async fn mark_ready(&self, repository: &Repository, number: ChangeRequestNumber) -> Result<()> {
        let pull_request = self.github_pull_request(repository, number).await?;
        let _: serde_json::Value = self
            .user()?
            .graphql(&json!({
                "query": MARK_READY,
                "variables": { "pullRequestId": pull_request.node_id }
            }))
            .await
            .map_err(|error| external("mark change request ready", error))?;
        Ok(())
    }

    async fn publish_check(
        &self,
        repository: &Repository,
        app_name: &str,
        outcome: &CheckOutcome,
    ) -> Result<()> {
        let _: serde_json::Value = self
            .app(app_name)?
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
            .map_err(|error| external("publish change request check", error))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use interprex::{
        ChangeRequestHead, ChangeRequestState, CheckConclusion, CheckOutcome, CodeReviewsProvider,
        FindingResolution, FindingResolutionReason, FindingResolutionRecord, FindingSeverity,
        ProviderError, Repository, ReviewActorKind, ReviewAnchor, ReviewAuthor, ReviewLocation,
        ReviewRequestTarget, ReviewTarget, ReviewTeamKind, ReviewThreadStatus,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
    };

    use crate::GithubProvider;

    use super::{
        GithubPullRequest, GithubReview, GithubUnanchoredComment, ParsedFindingResolution,
        ReviewRequestNode, ReviewRequestsData, ThreadsData, TimelineData, TimelineItemNode,
        finding_resolution, github_resolution_reply, head_filter, latest_finding_resolution,
        normalize_change_request,
    };

    fn review_request_timeline() -> TimelineData {
        serde_json::from_str(include_str!(
            "../tests/fixtures/review_request_timeline.json"
        ))
        .expect("review request timeline fixture")
    }

    fn requested_at(time: &str) -> Option<chrono::DateTime<chrono::Utc>> {
        Some(time.parse().expect("request timestamp"))
    }

    #[test]
    fn a_head_whose_repository_github_dropped_is_absent_rather_than_guessed() {
        let mut pull_request: GithubPullRequest =
            serde_json::from_str(include_str!("../tests/fixtures/pull_request.json"))
                .expect("pull request fixture");
        pull_request.head.repo = None;
        let change_request = normalize_change_request(
            pull_request,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("normalizes");

        assert_eq!(change_request.head, None);
        assert_eq!(
            change_request.base_branch, "main",
            "the targeted branch survives a head whose repository is gone"
        );
    }

    #[test]
    fn head_filter_names_the_repository_holding_the_branch() {
        let upstream = Repository::new("civitas-forge", "interprex").expect("repository");
        let fork = Repository::new("contributor", "interprex").expect("repository");
        assert_eq!(
            head_filter(
                &ChangeRequestHead::new(upstream, "refs/heads/feat/open-request").expect("head")
            ),
            "civitas-forge:feat/open-request"
        );
        assert_eq!(
            head_filter(
                &ChangeRequestHead::new(fork, "refs/heads/feat/open-request").expect("head")
            ),
            "contributor:feat/open-request",
            "a fork head keeps its own owner rather than the targeted repository's"
        );
    }

    #[test]
    fn github_reply_keeps_visible_labels_and_hidden_canonical_metadata() {
        for (reason, label) in [
            (FindingResolutionReason::Addressed, "Addressed"),
            (FindingResolutionReason::Invalid, "Invalid"),
            (FindingResolutionReason::WontFix, "Won't fix"),
        ] {
            let expected = FindingResolution {
                reason,
                addressing_severity: FindingSeverity::Minor,
            };

            let body = github_resolution_reply(expected, "The addressing explanation.");

            assert!(body.contains(&format!("**Resolution:** {label}")));
            assert!(body.contains("**Addressing severity:** Minor"));
            assert!(body.contains("https://img.shields.io/badge/severity-minor-fbca04"));
            assert!(body.contains("<!-- interprex:finding-resolution"));
            assert_eq!(
                finding_resolution(&body),
                ParsedFindingResolution::Supported(expected)
            );
        }
    }

    #[test]
    fn parser_distinguishes_malformed_and_unsupported_resolution_metadata() {
        assert_eq!(
            finding_resolution(
                "<!-- interprex:finding-resolution\n{\"version\":2,\"resolution_reason\":\"ADDRESSED\",\"addressing_severity\":\"major\"}\n-->"
            ),
            ParsedFindingResolution::UnsupportedVersion(2)
        );
        assert_eq!(
            finding_resolution("<!-- interprex:finding-resolution\nnot json\n-->"),
            ParsedFindingResolution::Absent
        );

        let valid = github_resolution_reply(
            FindingResolution {
                reason: FindingResolutionReason::Invalid,
                addressing_severity: FindingSeverity::Nit,
            },
            "Valid record before malformed trailing metadata.",
        );
        let body = format!("{valid}\n\n<!-- interprex:finding-resolution\nnot json\n-->");
        assert_eq!(finding_resolution(&body), ParsedFindingResolution::Absent);
        assert_eq!(
            finding_resolution(&format!("{valid}\n\nordinary trailing text")),
            ParsedFindingResolution::Absent
        );
    }

    #[test]
    fn github_fixtures_preserve_reviews_findings_standalone_threads_and_unanchored_comments() {
        let pull_request: GithubPullRequest =
            serde_json::from_str(include_str!("../tests/fixtures/pull_request.json"))
                .expect("pull request fixture");
        let reviews: Vec<GithubReview> =
            serde_json::from_str(include_str!("../tests/fixtures/code_review_reviews.json"))
                .expect("review fixture");
        let mut threads: ThreadsData =
            serde_json::from_str(include_str!("../tests/fixtures/review_threads.json"))
                .expect("thread fixture");
        let expected_resolution = FindingResolution {
            reason: FindingResolutionReason::Addressed,
            addressing_severity: FindingSeverity::Major,
        };
        threads.repository.pull_request.review_threads.nodes[0]
            .comments
            .nodes[1]
            .body =
            github_resolution_reply(expected_resolution, "Addressed in the current revision.");
        let requests: ReviewRequestsData =
            serde_json::from_str(include_str!("../tests/fixtures/review_requests.json"))
                .expect("review request fixture");
        let unanchored_comments: Vec<GithubUnanchoredComment> =
            serde_json::from_str(include_str!("../tests/fixtures/unanchored_comments.json"))
                .expect("unanchored comments fixture");
        let timeline = review_request_timeline();
        let change_request = normalize_change_request(
            pull_request,
            reviews,
            threads.repository.pull_request.review_threads.nodes,
            requests.repository.pull_request.review_requests.nodes,
            timeline.repository.pull_request.timeline_items.nodes,
            unanchored_comments,
        )
        .expect("normalizes");

        assert_eq!(
            change_request.base_branch, "main",
            "the targeted branch is a named fact, not inferred from base_sha"
        );
        assert_eq!(
            change_request.head,
            Some(
                ChangeRequestHead::new(
                    Repository::new("contributor", "interprex-sandbox").expect("repository"),
                    "refs/heads/feature"
                )
                .expect("head")
            ),
            "a fork head is observed as the fork's branch, not the targeted repository's"
        );
        assert_eq!(change_request.reviews.len(), 11);
        assert_eq!(
            change_request.reviews[1].revision,
            change_request.reviews[3].revision
        );
        assert_ne!(change_request.reviews[1].id, change_request.reviews[3].id);
        assert!(change_request.reviews[0].id.as_str().starts_with("PRR_"));
        let finding = &change_request.reviews[0].findings[0];
        assert_eq!(
            finding.location,
            ReviewLocation {
                path: "docs/dev/architecture.lex".to_owned(),
                anchor: ReviewAnchor::Lines {
                    side: interprex::ReviewDiffSide::Right,
                    original: interprex::ReviewLineRange {
                        start: Some(interprex::ReviewLine::new(177).expect("line")),
                        end: interprex::ReviewLine::new(181).expect("line"),
                    },
                    current: Some(interprex::ReviewLineRange {
                        start: Some(interprex::ReviewLine::new(184).expect("line")),
                        end: interprex::ReviewLine::new(188).expect("line"),
                    }),
                },
            }
        );
        assert!(finding.comment.id.as_str().starts_with("PRRC_"));
        assert_eq!(finding.replies.len(), 1);
        assert_eq!(finding.replies[0].author.login, "arthur-debert");
        assert_eq!(finding.status, ReviewThreadStatus::Resolved);
        let record = finding.resolution.as_ref().expect("resolution record");
        assert_eq!(record.supported_resolution(), Some(expected_resolution));
        assert_eq!(record.source_reply_id(), &finding.replies[0].id);
        assert_eq!(
            finding
                .resolution_reply()
                .expect("linked resolution reply")
                .author
                .login,
            "arthur-debert"
        );
        assert_eq!(
            change_request.reviews[0]
                .via_app
                .as_ref()
                .map(|app| app.slug.as_str()),
            Some("adr-review")
        );
        assert!(
            change_request
                .reviews
                .last()
                .expect("last review")
                .findings
                .is_empty()
        );
        let author_review = change_request
            .reviews
            .iter()
            .find(|item| item.author == ReviewAuthor::ChangeAuthor)
            .expect("author review");
        assert_eq!(
            author_review.author.relationship(),
            interprex::ReviewRelationship::ChangeAuthor
        );
        assert_eq!(
            author_review.author.actor(&change_request.author).login,
            "arthur-debert"
        );
        assert!(matches!(
            author_review.state,
            interprex::ReviewState::Submitted { .. }
        ));
        assert_eq!(author_review.findings.len(), 1);
        let draft_review = change_request
            .reviews
            .iter()
            .find(|item| item.author.actor(&change_request.author).login == "draft-reviewer")
            .expect("draft review");
        assert_eq!(draft_review.state, interprex::ReviewState::Draft);
        assert_eq!(
            draft_review.summary.as_deref(),
            Some("This draft was never submitted.")
        );
        let unavailable = change_request
            .reviews
            .iter()
            .filter(|item| item.author.relationship() == interprex::ReviewRelationship::Unknown)
            .collect::<Vec<_>>();
        assert_eq!(unavailable.len(), 2);
        assert_ne!(
            unavailable[0].author.actor(&change_request.author).id,
            unavailable[1].author.actor(&change_request.author).id
        );
        assert_eq!(
            change_request
                .reviews
                .iter()
                .map(|submitted| submitted.findings.len())
                .sum::<usize>()
                + change_request.standalone_threads.len(),
            4
        );
        let author_thread = author_review.findings.first().expect("author finding");
        assert_eq!(author_thread.comment.author.login, "arthur-debert");
        assert_eq!(author_thread.replies[0].author.login, "adr-agy-review");
        assert_eq!(
            author_thread.location,
            ReviewLocation {
                path: "src/lib.rs".to_owned(),
                anchor: ReviewAnchor::File,
            }
        );
        assert_eq!(change_request.outstanding_requests.len(), 6);
        assert!(matches!(
            &change_request.outstanding_requests[0].target,
            ReviewTarget::Actor(actor)
                if actor.kind == ReviewActorKind::Bot
                    && actor.login == "copilot-pull-request-reviewer"
        ));
        assert!(change_request.outstanding_requests[1].as_code_owner);
        assert!(matches!(
            &change_request.outstanding_requests[2].target,
            ReviewTarget::Team(team)
                if team.slug == "maintainers"
                    && team.kind == ReviewTeamKind::Organization
        ));
        assert_eq!(
            change_request.outstanding_requests[2].request_target,
            Some(ReviewRequestTarget::Team(
                "civitas-forge/maintainers".to_owned()
            ))
        );
        assert!(matches!(
            &change_request.outstanding_requests[3].target,
            ReviewTarget::Actor(actor) if actor.kind == ReviewActorKind::Placeholder
        ));
        assert!(matches!(
            &change_request.outstanding_requests[4].target,
            ReviewTarget::Team(team) if team.kind == interprex::ReviewTeamKind::Enterprise
        ));
        assert_eq!(
            change_request.outstanding_requests[5].target,
            ReviewTarget::Unavailable
        );
        assert_eq!(
            change_request
                .outstanding_requests
                .iter()
                .map(|request| request.requested_at)
                .collect::<Vec<_>>(),
            [
                requested_at("2026-06-23T09:00:00Z"),
                requested_at("2026-06-23T09:35:00Z"),
                requested_at("2026-06-23T09:15:00Z"),
                requested_at("2026-06-23T09:20:00Z"),
                requested_at("2026-06-23T09:25:00Z"),
                None,
            ]
        );
        assert_eq!(change_request.unanchored_comments.len(), 1);
        assert!(change_request.unanchored_comments[0].updated_at.is_some());
    }

    fn correlated_request_times(
        review_requests: serde_json::Value,
        request_events: serde_json::Value,
    ) -> Vec<Option<chrono::DateTime<chrono::Utc>>> {
        let pull_request: GithubPullRequest =
            serde_json::from_str(include_str!("../tests/fixtures/pull_request.json"))
                .expect("pull request fixture");
        let review_requests: Vec<ReviewRequestNode> =
            serde_json::from_value(review_requests).expect("review request nodes");
        let request_events: Vec<TimelineItemNode> =
            serde_json::from_value(request_events).expect("review request events");

        normalize_change_request(
            pull_request,
            Vec::new(),
            Vec::new(),
            review_requests,
            request_events,
            Vec::new(),
        )
        .expect("normalizes")
        .outstanding_requests
        .into_iter()
        .map(|request| request.requested_at)
        .collect()
    }

    #[test]
    fn a_re_requested_reviewer_reports_the_latest_request() {
        let times = correlated_request_times(
            serde_json::json!([{
                "id": "PRR_kwDORequestUser",
                "asCodeOwner": false,
                "requestedReviewer": {
                    "__typename": "User",
                    "id": "U_kwDOReviewer",
                    "login": "alice"
                }
            }]),
            serde_json::json!([
                {
                    "__typename": "ReviewRequestedEvent",
                    "createdAt": "2026-06-23T09:00:00Z",
                    "requestedReviewer": { "__typename": "User", "id": "U_kwDOReviewer" }
                },
                {
                    "__typename": "ReviewRequestRemovedEvent",
                    "requestedReviewer": { "__typename": "User", "id": "U_kwDOReviewer" }
                },
                {
                    "__typename": "ReviewRequestedEvent",
                    "createdAt": "2026-06-23T11:00:00Z",
                    "requestedReviewer": { "__typename": "User", "id": "U_kwDOReviewer" }
                }
            ]),
        );

        assert_eq!(times, [requested_at("2026-06-23T11:00:00Z")]);
    }

    #[test]
    fn a_removed_request_leaves_a_later_reviewer_uncorrelated() {
        let times = correlated_request_times(
            serde_json::json!([{
                "id": "PRR_kwDORequestUser",
                "asCodeOwner": false,
                "requestedReviewer": {
                    "__typename": "User",
                    "id": "U_kwDOReviewer",
                    "login": "alice"
                }
            }]),
            serde_json::json!([
                {
                    "__typename": "ReviewRequestedEvent",
                    "createdAt": "2026-06-23T09:00:00Z",
                    "requestedReviewer": { "__typename": "User", "id": "U_kwDOReviewer" }
                },
                {
                    "__typename": "ReviewRequestRemovedEvent",
                    "requestedReviewer": { "__typename": "User", "id": "U_kwDOReviewer" }
                }
            ]),
        );

        assert_eq!(times, [None]);
    }

    #[test]
    fn a_team_and_a_user_sharing_a_name_do_not_take_each_others_request_times() {
        let times = correlated_request_times(
            serde_json::json!([
                {
                    "id": "PRR_kwDORequestUser",
                    "asCodeOwner": false,
                    "requestedReviewer": {
                        "__typename": "User",
                        "id": "U_kwDOReviewers",
                        "login": "reviewers"
                    }
                },
                {
                    "id": "PRR_kwDORequestTeam",
                    "asCodeOwner": false,
                    "requestedReviewer": {
                        "__typename": "Team",
                        "id": "T_kwDOReviewers",
                        "slug": "reviewers",
                        "name": "reviewers",
                        "organization": { "login": "civitas-forge" }
                    }
                }
            ]),
            serde_json::json!([
                {
                    "__typename": "ReviewRequestedEvent",
                    "createdAt": "2026-06-23T09:00:00Z",
                    "requestedReviewer": { "__typename": "Team", "id": "T_kwDOReviewers" }
                },
                {
                    "__typename": "ReviewRequestedEvent",
                    "createdAt": "2026-06-23T10:00:00Z",
                    "requestedReviewer": { "__typename": "User", "id": "U_kwDOReviewers" }
                }
            ]),
        );

        assert_eq!(
            times,
            [
                requested_at("2026-06-23T10:00:00Z"),
                requested_at("2026-06-23T09:00:00Z")
            ]
        );
    }

    #[test]
    fn an_unavailable_target_reports_no_request_time() {
        let times = correlated_request_times(
            serde_json::json!([{
                "id": "PRR_kwDORequestUnavailable",
                "asCodeOwner": false,
                "requestedReviewer": null
            }]),
            serde_json::json!([
                {
                    "__typename": "ReviewRequestedEvent",
                    "createdAt": "2026-06-23T09:00:00Z",
                    "requestedReviewer": null
                },
                {
                    "__typename": "ReviewRequestedEvent",
                    "createdAt": "2026-06-23T10:00:00Z",
                    "requestedReviewer": { "__typename": "User", "id": "U_kwDOReviewer" }
                }
            ]),
        );

        assert_eq!(times, [None]);
    }

    #[test]
    fn unsupported_newer_resolution_does_not_resurrect_an_older_record() {
        let pull_request: GithubPullRequest =
            serde_json::from_str(include_str!("../tests/fixtures/pull_request.json"))
                .expect("pull request fixture");
        let reviews: Vec<GithubReview> =
            serde_json::from_str(include_str!("../tests/fixtures/code_review_reviews.json"))
                .expect("review fixture");
        let mut threads: ThreadsData =
            serde_json::from_str(include_str!("../tests/fixtures/review_threads.json"))
                .expect("thread fixture");
        let resolution = FindingResolution {
            reason: FindingResolutionReason::Addressed,
            addressing_severity: FindingSeverity::Major,
        };
        let comments = &mut threads.repository.pull_request.review_threads.nodes[0]
            .comments
            .nodes;
        comments[1].body = github_resolution_reply(resolution, "Version one resolution.");
        let mut future = comments[1].clone();
        future.id = "PRRC_future_resolution".to_owned();
        future.body = github_resolution_reply(resolution, "Future resolution.")
            .replace("\"version\":1", "\"version\":2")
            .replace(
                "\"resolution_reason\":\"ADDRESSED\"",
                "\"resolution_reason\":\"SUPERSEDED\"",
            );
        comments.push(future);

        let change_request = normalize_change_request(
            pull_request,
            reviews,
            threads.repository.pull_request.review_threads.nodes,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("unsupported future resolution metadata remains observable as replies");

        let finding = &change_request.reviews[0].findings[0];
        assert!(matches!(
            &finding.resolution,
            Some(FindingResolutionRecord::Unsupported {
                metadata_format,
                source_reply_id,
            }) if metadata_format == "github:interprex-finding-resolution:v2"
                && source_reply_id.as_str() == "PRRC_future_resolution"
        ));
        assert_eq!(
            finding.resolution,
            latest_finding_resolution(&finding.replies)
        );
    }

    #[test]
    fn unknown_actor_kinds_are_unrepresentable() {
        let pull_request: GithubPullRequest =
            serde_json::from_str(include_str!("../tests/fixtures/pull_request.json"))
                .expect("pull request fixture");
        let mut reviews: Vec<GithubReview> =
            serde_json::from_str(include_str!("../tests/fixtures/code_review_reviews.json"))
                .expect("review fixture");
        reviews[0].user.as_mut().expect("reviewer").kind = Some("Repository".to_owned());

        let error = normalize_change_request(
            pull_request,
            reviews,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect_err("unknown actor kind must be unrepresentable");
        assert!(matches!(
            error,
            ProviderError::Unrepresentable { fact, .. } if fact.contains("unknown review actor kind")
        ));
    }

    #[test]
    fn actors_without_a_type_are_unrepresentable() {
        let pull_request: GithubPullRequest =
            serde_json::from_str(include_str!("../tests/fixtures/pull_request.json"))
                .expect("pull request fixture");
        let mut reviews: Vec<GithubReview> =
            serde_json::from_str(include_str!("../tests/fixtures/code_review_reviews.json"))
                .expect("review fixture");
        reviews[0].user.as_mut().expect("reviewer").kind = None;

        let error = normalize_change_request(
            pull_request,
            reviews,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect_err("missing actor type must be unrepresentable");
        assert!(matches!(
            error,
            ProviderError::Unrepresentable { fact, .. } if fact.contains("has no type")
        ));
    }

    #[test]
    fn unknown_change_request_states_are_unrepresentable() {
        let mut pull_request: GithubPullRequest =
            serde_json::from_str(include_str!("../tests/fixtures/pull_request.json"))
                .expect("pull request fixture");
        pull_request.state = "reopening".to_owned();

        let error = normalize_change_request(
            pull_request,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect_err("unknown state must be unrepresentable");
        assert!(matches!(
            error,
            ProviderError::Unrepresentable { fact, .. } if fact.contains("unknown change request state")
        ));
    }

    fn pull_request_with_merge_facts(
        state: &str,
        merged: bool,
        merged_at: Option<&str>,
    ) -> GithubPullRequest {
        let mut pull_request: GithubPullRequest =
            serde_json::from_str(include_str!("../tests/fixtures/pull_request.json"))
                .expect("pull request fixture");
        pull_request.state = state.to_owned();
        pull_request.merged = merged;
        pull_request.merged_at = merged_at.map(|value| value.parse().expect("merge time"));
        pull_request
    }

    #[test]
    fn a_merged_change_request_is_distinct_from_one_closed_without_merging() {
        let closed = normalize_change_request(
            pull_request_with_merge_facts("closed", false, None),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("normalizes a change request closed without merging");
        assert_eq!(closed.state, ChangeRequestState::Closed);

        let merged = normalize_change_request(
            pull_request_with_merge_facts("closed", true, Some("2026-08-24T11:00:00Z")),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("normalizes a merged change request");
        assert_eq!(
            merged.state,
            ChangeRequestState::Merged {
                merged_at: "2026-08-24T11:00:00Z".parse().expect("merge time"),
            }
        );
    }

    #[test]
    fn contradictory_merge_facts_are_unrepresentable() {
        for (state, merged, merged_at, expected) in [
            (
                "open",
                true,
                Some("2026-08-24T11:00:00Z"),
                "change request 5 is open and merged",
            ),
            ("open", true, None, "change request 5 is open and merged"),
            (
                "closed",
                true,
                None,
                "merged change request 5 has no merge time",
            ),
            (
                "closed",
                false,
                Some("2026-08-24T11:00:00Z"),
                "change request 5 has a merge time but is not merged",
            ),
            (
                "open",
                false,
                Some("2026-08-24T11:00:00Z"),
                "change request 5 has a merge time but is not merged",
            ),
        ] {
            let error = normalize_change_request(
                pull_request_with_merge_facts(state, merged, merged_at),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .expect_err("contradictory merge facts must be unrepresentable");
            assert!(
                matches!(&error, ProviderError::Unrepresentable { fact, .. } if fact == expected),
                "{error}"
            );
        }
    }

    #[test]
    fn submitted_reviews_require_a_submission_time() {
        let pull_request: GithubPullRequest =
            serde_json::from_str(include_str!("../tests/fixtures/pull_request.json"))
                .expect("pull request fixture");
        let mut reviews: Vec<GithubReview> =
            serde_json::from_str(include_str!("../tests/fixtures/code_review_reviews.json"))
                .expect("review fixture");
        reviews[0].submitted_at = None;

        let error = normalize_change_request(
            pull_request,
            reviews,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect_err("submitted review without time must be unrepresentable");
        assert!(matches!(
            error,
            ProviderError::Unrepresentable { fact, .. } if fact.contains("has no submission time")
        ));
    }

    #[test]
    fn review_threads_require_an_initial_comment() {
        let pull_request: GithubPullRequest =
            serde_json::from_str(include_str!("../tests/fixtures/pull_request.json"))
                .expect("pull request fixture");
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

        let error = normalize_change_request(
            pull_request,
            reviews,
            threads.repository.pull_request.review_threads.nodes,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect_err("thread without an initial comment must be unrepresentable");
        assert!(matches!(
            error,
            ProviderError::Unrepresentable { fact, .. } if fact.contains("has no comments")
        ));
    }

    #[test]
    fn missing_thread_review_is_not_misclassified_as_a_standalone_thread() {
        let pull_request: GithubPullRequest =
            serde_json::from_str(include_str!("../tests/fixtures/pull_request.json"))
                .expect("pull request fixture");
        let reviews: Vec<GithubReview> =
            serde_json::from_str(include_str!("../tests/fixtures/code_review_reviews.json"))
                .expect("review fixture");
        let mut threads: ThreadsData =
            serde_json::from_str(include_str!("../tests/fixtures/review_threads.json"))
                .expect("thread fixture");
        threads.repository.pull_request.review_threads.nodes[0]
            .comments
            .nodes[0]
            .pull_request_review = Some(super::CommentReview {
            id: "PRR_missing".to_owned(),
        });

        let error = normalize_change_request(
            pull_request,
            reviews,
            threads.repository.pull_request.review_threads.nodes,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect_err("missing originating submission must be unrepresentable");
        assert!(matches!(
            error,
            ProviderError::Unrepresentable { fact, .. } if fact.contains("references missing review")
        ));
    }

    #[test]
    fn thread_without_an_originating_review_becomes_a_standalone_thread() {
        let pull_request: GithubPullRequest =
            serde_json::from_str(include_str!("../tests/fixtures/pull_request.json"))
                .expect("pull request fixture");
        let reviews: Vec<GithubReview> =
            serde_json::from_str(include_str!("../tests/fixtures/code_review_reviews.json"))
                .expect("review fixture");
        let mut threads: ThreadsData =
            serde_json::from_str(include_str!("../tests/fixtures/review_threads.json"))
                .expect("thread fixture");
        let thread = threads
            .repository
            .pull_request
            .review_threads
            .nodes
            .first_mut()
            .expect("captured thread");
        let expected_id = thread.id.clone();
        thread
            .comments
            .nodes
            .first_mut()
            .expect("initial comment")
            .pull_request_review = None;
        thread.comments.nodes[1].body = github_resolution_reply(
            FindingResolution {
                reason: FindingResolutionReason::Addressed,
                addressing_severity: FindingSeverity::Minor,
            },
            "Marker text on a standalone thread.",
        );

        let change_request = normalize_change_request(
            pull_request,
            reviews,
            threads.repository.pull_request.review_threads.nodes,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("normalizes standalone thread");

        assert_eq!(change_request.standalone_threads.len(), 1);
        assert_eq!(
            change_request.standalone_threads[0].id.as_str(),
            expected_id
        );
        assert_eq!(
            change_request
                .reviews
                .iter()
                .map(|item| item.findings.len())
                .sum::<usize>()
                + change_request.standalone_threads.len(),
            4
        );
    }

    #[test]
    fn deleted_change_author_remains_an_unavailable_actor() {
        let mut pull_request: GithubPullRequest =
            serde_json::from_str(include_str!("../tests/fixtures/pull_request.json"))
                .expect("pull request fixture");
        pull_request.user = None;
        let reviews: Vec<GithubReview> =
            serde_json::from_str(include_str!("../tests/fixtures/code_review_reviews.json"))
                .expect("review fixture");

        let change_request = normalize_change_request(
            pull_request,
            reviews,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("deleted author remains readable");
        assert_eq!(change_request.author.kind, ReviewActorKind::Placeholder);
        assert_eq!(change_request.author.login, "ghost");
        assert!(
            change_request.reviews.iter().all(|item| {
                item.author.relationship() == interprex::ReviewRelationship::Unknown
            })
        );
    }

    #[test]
    fn draft_reviews_with_a_submission_time_are_unrepresentable() {
        let pull_request: GithubPullRequest =
            serde_json::from_str(include_str!("../tests/fixtures/pull_request.json"))
                .expect("pull request fixture");
        let mut reviews: Vec<GithubReview> =
            serde_json::from_str(include_str!("../tests/fixtures/code_review_reviews.json"))
                .expect("review fixture");
        reviews[10].submitted_at = Some("2026-06-23T22:10:00Z".parse().expect("submission time"));

        let error = normalize_change_request(
            pull_request,
            reviews,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect_err("draft review with a submission time must be unrepresentable");
        assert!(matches!(
            error,
            ProviderError::Unrepresentable { fact, .. } if fact.contains("has a submission time")
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
        let repository = Repository::new("civitas-forge", "interprex-sandbox").expect("repository");
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
        assert!(request.starts_with("POST /repos/civitas-forge/interprex-sandbox/check-runs "));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer app-installation-token")
        );
        assert!(request.contains("\"conclusion\":\"success\""));
    }
}
