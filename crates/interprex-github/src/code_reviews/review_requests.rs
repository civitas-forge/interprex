use std::collections::BTreeMap;

use interprex::{
    ChangeRequestNumber, ProviderError, Repository, Result, ReviewRequest, ReviewRequestId,
    ReviewRequestTarget, ReviewTarget, ReviewTeam, ReviewTeamId, ReviewTeamKind,
};
use serde::Deserialize;
use serde_json::json;

use crate::{GithubProvider, client::external};

use super::actors::actor;
use super::{PageInfo, continuation_cursor};

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

pub(super) const REQUEST_REVIEWS_BY_LOGIN: &str = r#"
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
pub(super) struct ReviewRequestsData {
    pub(super) repository: ReviewRequestsRepository,
}

#[derive(Deserialize)]
pub(super) struct ReviewRequestsRepository {
    #[serde(rename = "pullRequest")]
    pub(super) pull_request: ReviewRequestsPullRequest,
}

#[derive(Deserialize)]
pub(super) struct ReviewRequestsPullRequest {
    #[serde(rename = "reviewRequests")]
    pub(super) review_requests: ReviewRequestConnection,
}

#[derive(Deserialize)]
pub(super) struct ReviewRequestConnection {
    pub(super) nodes: Vec<ReviewRequestNode>,
    #[serde(rename = "pageInfo")]
    pub(super) page_info: PageInfo,
}

#[derive(Deserialize, PartialEq)]
pub(super) struct ReviewRequestNode {
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
pub(super) struct TimelineData {
    pub(super) repository: TimelineRepository,
}

#[derive(Deserialize)]
pub(super) struct TimelineRepository {
    #[serde(rename = "pullRequest")]
    pub(super) pull_request: TimelinePullRequest,
}

#[derive(Deserialize)]
pub(super) struct TimelinePullRequest {
    #[serde(rename = "timelineItems")]
    pub(super) timeline_items: TimelineConnection,
}

#[derive(Deserialize)]
pub(super) struct TimelineConnection {
    pub(super) nodes: Vec<TimelineItemNode>,
    #[serde(rename = "pageInfo")]
    pub(super) page_info: PageInfo,
}

/// The request and removal events selected by `REVIEW_REQUEST_TIMELINE`.
///
/// The query restricts `timelineItems` to these two types, so an item of any
/// other type fails to deserialize instead of being read as one of them.
#[derive(Deserialize)]
#[serde(tag = "__typename")]
pub(super) enum TimelineItemNode {
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
pub(super) enum TimelineReviewerNode {
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
pub(super) enum ReviewTargetKey {
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
    pub(super) fn target_key(&self) -> Option<ReviewTargetKey> {
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
pub(super) fn outstanding_request_times(
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

pub(super) fn normalize_review_request(
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

impl GithubProvider {
    pub(super) async fn github_review_requests(
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

    pub(super) async fn github_review_request_events(
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

#[cfg(test)]
mod tests {
    use super::super::change_requests::{GithubPullRequest, normalize_change_request};
    use super::{ReviewRequestNode, TimelineItemNode};

    fn requested_at(time: &str) -> Option<chrono::DateTime<chrono::Utc>> {
        Some(time.parse().expect("request timestamp"))
    }

    fn correlated_request_times(
        review_requests: serde_json::Value,
        request_events: serde_json::Value,
    ) -> Vec<Option<chrono::DateTime<chrono::Utc>>> {
        let pull_request: GithubPullRequest =
            serde_json::from_str(include_str!("../../tests/fixtures/pull_request.json"))
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
}
