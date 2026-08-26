use std::time::Duration;

use interprex::{
    ChangeRequestHead, ChangeRequestNumber, ChangeRequestState, CheckConclusion, CheckStatus,
    CodeReviewsProvider, FindingResolution, FindingResolutionReason, FindingResolutionReply,
    FindingSeverity, Mergeability, Repository, ReviewRequestTarget, ReviewThreadId,
};
use tokio::time::timeout;

use super::http_fixture::{
    assert_user_request, json_responses, provider, repository, rest_filtered_pages, server,
};

#[tokio::test]
async fn code_review_domain_lists_open_change_requests_for_a_fork_head_across_pages() {
    let route = "/repos/civitas-forge/interprex-sandbox/pulls";
    let (uri, requests) = rest_filtered_pages(
        route,
        "head=contributor%3Afeature&state=open&",
        vec![
            r#"[{"number":5,"head":{"ref":"feature","sha":"aaa","repo":{"full_name":"contributor/interprex-sandbox"}}},
                 {"number":6,"head":{"ref":"feature","sha":"bbb","repo":{"full_name":"contributor/another-sandbox"}}}]"#,
            r#"[{"number":9,"head":{"ref":"feature","sha":"ccc","repo":{"full_name":"contributor/interprex-sandbox"}}}]"#,
        ],
    )
    .await;
    let fork = Repository::new("contributor", "interprex-sandbox").expect("repository");
    let numbers = provider(uri)
        .open_change_requests(
            &repository(),
            &ChangeRequestHead::new(fork, "refs/heads/feature").expect("head"),
        )
        .await
        .expect("open change requests");
    assert_eq!(
        numbers
            .iter()
            .map(|number| number.get())
            .collect::<Vec<_>>(),
        [5, 9],
        "6 proposes the same owner's branch in another repository, which is another head"
    );
    let requests = requests.await.expect("captured requests");
    assert_user_request(
        &requests[0],
        "GET /repos/civitas-forge/interprex-sandbox/pulls?head=contributor%3Afeature&state=open&per_page=100 ",
    );
    assert!(requests[1].starts_with(
        "GET /repos/civitas-forge/interprex-sandbox/pulls?head=contributor%3Afeature&state=open&per_page=100&page=2 "
    ));
}

#[tokio::test]
async fn code_review_domain_requests_users_bots_and_teams_through_the_login_mutation() {
    let (uri, requests) = json_responses(vec![
        include_str!("../fixtures/pull_request.json"),
        r#"{"data":{"requestReviewsByLogin":{"pullRequest":{"id":"PR_kwDOExample"}}}}"#,
    ])
    .await;
    provider(uri)
        .request_reviewers(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &[
                ReviewRequestTarget::User("alice".to_owned()),
                ReviewRequestTarget::Bot("copilot-pull-request-reviewer".to_owned()),
                ReviewRequestTarget::Team("civitas-forge/maintainers".to_owned()),
            ],
        )
        .await
        .expect("review request");
    let requests = timeout(Duration::from_secs(1), requests)
        .await
        .expect("provider sent both requests")
        .expect("captured requests");
    assert_user_request(
        &requests[0],
        "GET /repos/civitas-forge/interprex-sandbox/pulls/5 ",
    );
    assert_user_request(&requests[1], "POST /graphql ");
    let (_, body) = requests[1].split_once("\r\n\r\n").expect("request body");
    let body: serde_json::Value = serde_json::from_str(body).expect("JSON request body");
    assert!(
        body["query"]
            .as_str()
            .expect("GraphQL document")
            .contains("requestReviewsByLogin")
    );
    assert_eq!(body["variables"]["pullRequestId"], "PR_kwDOExample");
    assert_eq!(
        body["variables"]["userLogins"],
        serde_json::json!(["alice"])
    );
    assert_eq!(
        body["variables"]["botLogins"],
        serde_json::json!(["copilot-pull-request-reviewer[bot]"])
    );
    assert_eq!(
        body["variables"]["teamSlugs"],
        serde_json::json!(["civitas-forge/maintainers"])
    );
}

#[tokio::test]
async fn code_review_domain_resolves_the_review_handle_before_marking_ready() {
    let (uri, requests) = json_responses(vec![
        include_str!("../fixtures/pull_request.json"),
        r#"{"data":{"markPullRequestReadyForReview":{"pullRequest":{"id":"PR_kwDOExample","isDraft":false}}}}"#,
    ])
    .await;
    provider(uri)
        .mark_ready(&repository(), ChangeRequestNumber::new(5).expect("number"))
        .await
        .expect("mark ready");
    let requests = requests.await.expect("captured requests");
    assert_user_request(
        &requests[0],
        "GET /repos/civitas-forge/interprex-sandbox/pulls/5 ",
    );
    assert_user_request(&requests[1], "POST /graphql ");
    let (_, body) = requests[1].split_once("\r\n\r\n").expect("request body");
    let body: serde_json::Value = serde_json::from_str(body).expect("JSON request body");
    assert_eq!(body["variables"]["pullRequestId"], "PR_kwDOExample");
}

#[tokio::test]
async fn code_review_domain_resolves_a_scoped_review_thread_handle() {
    let (uri, request) = server(
        "200 OK",
        "application/json",
        r#"{"data":{"resolveReviewThread":{"thread":{"id":"PRRT_kwDOExample","isResolved":true}}}}"#,
    )
    .await;
    provider(uri)
        .resolve_thread(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &ReviewThreadId::new("PRRT_kwDOExample").expect("thread id"),
        )
        .await
        .expect("resolve thread");
    let request = request.await.expect("captured request");
    assert_user_request(&request, "POST /graphql ");
    let (_, body) = request.split_once("\r\n\r\n").expect("request body");
    let body: serde_json::Value = serde_json::from_str(body).expect("JSON request body");
    assert_eq!(body["variables"]["threadId"], "PRRT_kwDOExample");
}

#[tokio::test]
async fn code_review_domain_rejects_an_unconfirmed_thread_resolution() {
    let (uri, _request) = server(
        "200 OK",
        "application/json",
        r#"{"data":{"resolveReviewThread":{"thread":{"id":"PRRT_kwDOOther","isResolved":false}}}}"#,
    )
    .await;
    let error = provider(uri)
        .resolve_thread(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &ReviewThreadId::new("PRRT_kwDOExample").expect("thread id"),
        )
        .await
        .expect_err("mismatched unresolved response must fail");

    assert!(error.to_string().contains("isResolved=false"));
    assert!(error.to_string().contains("PRRT_kwDOOther"));
}

#[tokio::test]
async fn code_review_domain_records_a_finding_resolution_before_resolving_the_thread() {
    let (uri, requests) = json_responses(vec![
        include_str!("../fixtures/pull_request.json").to_owned(),
        include_str!("../fixtures/code_review_reviews.json").to_owned(),
        include_str!("../fixtures/review_threads_response.json").replace(
            "\"isResolved\": true",
            "\"isResolved\": false",
        ),
        include_str!("../fixtures/review_requests_response.json").to_owned(),
        include_str!("../fixtures/review_request_timeline_second_page.json").to_owned(),
        include_str!("../fixtures/unanchored_comments.json").to_owned(),
        r#"{"data":{"addPullRequestReviewThreadReply":{"comment":{"id":"PRRC_resolution"}}}}"#
            .to_owned(),
        r#"{"data":{"resolveReviewThread":{"thread":{"id":"PRRT_kwDOSCkZoc6LuYFt","isResolved":true}}}}"#
            .to_owned(),
    ])
    .await;
    provider(uri)
        .resolve_finding(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &ReviewThreadId::new("PRRT_kwDOSCkZoc6LuYFt").expect("thread id"),
            FindingResolution {
                reason: FindingResolutionReason::Invalid,
                addressing_severity: FindingSeverity::Minor,
            },
            &FindingResolutionReply::new(
                "The reported state cannot be reached through the public interface.",
            )
            .expect("resolution explanation"),
        )
        .await
        .expect("resolve finding");

    let requests = requests.await.expect("captured requests");
    assert_eq!(requests.len(), 8);
    assert_user_request(&requests[6], "POST /graphql ");
    assert_user_request(&requests[7], "POST /graphql ");
    let (_, reply_body) = requests[6]
        .split_once("\r\n\r\n")
        .expect("reply request body");
    let reply_body: serde_json::Value =
        serde_json::from_str(reply_body).expect("JSON reply request body");
    assert!(
        reply_body["query"]
            .as_str()
            .expect("GraphQL document")
            .contains("addPullRequestReviewThreadReply")
    );
    assert_eq!(reply_body["variables"]["threadId"], "PRRT_kwDOSCkZoc6LuYFt");
    let posted = reply_body["variables"]["body"]
        .as_str()
        .expect("reply Markdown");
    assert!(posted.contains("**Resolution:** Invalid"));
    assert!(posted.contains("**Addressing severity:** Minor"));
    assert!(posted.contains("<!-- interprex:finding-resolution"));
    assert!(posted.contains("\"resolution_reason\":\"INVALID\""));
    assert!(posted.contains("\"addressing_severity\":\"minor\""));

    let (_, resolve_body) = requests[7]
        .split_once("\r\n\r\n")
        .expect("resolve request body");
    let resolve_body: serde_json::Value =
        serde_json::from_str(resolve_body).expect("JSON resolve request body");
    assert_eq!(
        resolve_body["variables"]["threadId"],
        "PRRT_kwDOSCkZoc6LuYFt"
    );
}

#[tokio::test]
async fn code_review_domain_reads_one_complete_observation() {
    let (uri, requests) = json_responses(vec![
        include_str!("../fixtures/pull_request.json"),
        include_str!("../fixtures/code_review_reviews.json"),
        include_str!("../fixtures/review_threads_response.json"),
        include_str!("../fixtures/review_requests_response.json"),
        include_str!("../fixtures/review_request_timeline_first_page.json"),
        include_str!("../fixtures/review_request_timeline_second_page.json"),
        include_str!("../fixtures/unanchored_comments.json"),
    ])
    .await;
    let change_request = provider(uri)
        .change_request(&repository(), ChangeRequestNumber::new(5).expect("number"))
        .await
        .expect("change request");

    assert_eq!(change_request.state, ChangeRequestState::Open);
    assert_eq!(change_request.reviews.len(), 11);
    assert_eq!(
        change_request
            .reviews
            .iter()
            .filter(|item| item.state == interprex::ReviewState::Draft)
            .count(),
        1
    );
    assert_eq!(
        change_request
            .reviews
            .iter()
            .filter(|item| {
                item.author.relationship() == interprex::ReviewRelationship::ChangeAuthor
            })
            .count(),
        1
    );
    assert_eq!(
        change_request
            .reviews
            .iter()
            .filter(|item| item.author.relationship() == interprex::ReviewRelationship::Unknown)
            .count(),
        2
    );
    assert_eq!(change_request.reviews[0].findings[0].replies.len(), 1);
    assert!(
        change_request
            .reviews
            .last()
            .expect("last review")
            .findings
            .is_empty()
    );
    assert_eq!(change_request.outstanding_requests.len(), 2);
    assert_eq!(
        change_request
            .outstanding_requests
            .iter()
            .map(|request| request.requested_at)
            .collect::<Vec<_>>(),
        [
            Some(
                "2026-06-23T09:00:00Z"
                    .parse::<chrono::DateTime<chrono::Utc>>()
                    .expect("request timestamp")
            ),
            Some(
                "2026-06-23T09:15:00Z"
                    .parse::<chrono::DateTime<chrono::Utc>>()
                    .expect("request timestamp")
            ),
        ],
        "the re-request on the second timeline page supersedes the removal on the first"
    );
    assert_eq!(change_request.unanchored_comments.len(), 1);
    assert_eq!(change_request.mergeability, Mergeability::Mergeable);
    let requests = requests.await.expect("captured requests");
    assert_user_request(
        &requests[0],
        "GET /repos/civitas-forge/interprex-sandbox/pulls/5 ",
    );
    assert_user_request(
        &requests[1],
        "GET /repos/civitas-forge/interprex-sandbox/pulls/5/reviews?per_page=100 ",
    );
    assert_user_request(&requests[2], "POST /graphql ");
    assert_user_request(&requests[3], "POST /graphql ");
    assert_user_request(&requests[4], "POST /graphql ");
    assert_user_request(&requests[5], "POST /graphql ");
    assert_user_request(
        &requests[6],
        "GET /repos/civitas-forge/interprex-sandbox/issues/5/comments?per_page=100 ",
    );
    let (_, body) = requests[2].split_once("\r\n\r\n").expect("request body");
    let body: serde_json::Value = serde_json::from_str(body).expect("JSON request body");
    assert!(
        body["query"]
            .as_str()
            .expect("GraphQL document")
            .contains("pullRequestReview { id }")
    );
    assert!(
        !body["query"]
            .as_str()
            .expect("GraphQL document")
            .contains("databaseId")
    );
    assert!(
        body["query"]
            .as_str()
            .expect("GraphQL document")
            .contains("subjectType")
    );
    let (_, body) = requests[3].split_once("\r\n\r\n").expect("request body");
    let body: serde_json::Value = serde_json::from_str(body).expect("JSON request body");
    assert!(
        body["query"]
            .as_str()
            .expect("GraphQL document")
            .contains("reviewRequests")
    );
    assert!(
        body["query"]
            .as_str()
            .expect("GraphQL document")
            .contains("organization { login }")
    );
    let (_, body) = requests[4].split_once("\r\n\r\n").expect("request body");
    let body: serde_json::Value = serde_json::from_str(body).expect("JSON request body");
    let timeline_query = body["query"].as_str().expect("GraphQL document").to_owned();
    assert!(timeline_query.contains("timelineItems"));
    assert!(
        timeline_query
            .contains("itemTypes: [REVIEW_REQUESTED_EVENT, REVIEW_REQUEST_REMOVED_EVENT]")
    );
    assert!(timeline_query.contains("ReviewRequestedEvent"));
    assert!(timeline_query.contains("ReviewRequestRemovedEvent"));
    assert_eq!(body["variables"]["number"], 5);
    assert_eq!(body["variables"]["cursor"], serde_json::Value::Null);
    let (_, body) = requests[5].split_once("\r\n\r\n").expect("request body");
    let body: serde_json::Value = serde_json::from_str(body).expect("JSON request body");
    assert_eq!(body["query"], serde_json::json!(timeline_query));
    assert_eq!(body["variables"]["cursor"], "timeline-page-1");
}

#[tokio::test]
async fn code_review_domain_reads_no_timeline_without_an_outstanding_request() {
    let mut review_requests: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/review_requests_response.json"))
            .expect("review request fixture");
    review_requests["data"]["repository"]["pullRequest"]["reviewRequests"]["nodes"] =
        serde_json::json!([]);
    let (uri, requests) = json_responses(vec![
        include_str!("../fixtures/pull_request.json").to_owned(),
        include_str!("../fixtures/code_review_reviews.json").to_owned(),
        include_str!("../fixtures/review_threads_response.json").to_owned(),
        serde_json::to_string(&review_requests).expect("review request response"),
        include_str!("../fixtures/unanchored_comments.json").to_owned(),
    ])
    .await;

    let change_request = provider(uri)
        .change_request(&repository(), ChangeRequestNumber::new(5).expect("number"))
        .await
        .expect("change request");

    assert!(change_request.outstanding_requests.is_empty());
    let requests = requests.await.expect("captured requests");
    assert_eq!(requests.len(), 5);
    assert_user_request(
        &requests[4],
        "GET /repos/civitas-forge/interprex-sandbox/issues/5/comments?per_page=100 ",
    );
}

#[tokio::test]
async fn code_review_domain_preserves_a_standalone_thread() {
    let mut threads: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/review_threads_response.json"))
            .expect("thread fixture");
    threads["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"][0]["comments"]["nodes"]
        [0]["pullRequestReview"] = serde_json::Value::Null;
    let threads = serde_json::to_string(&threads).expect("thread response");
    let (uri, _) = json_responses(vec![
        include_str!("../fixtures/pull_request.json").to_owned(),
        include_str!("../fixtures/code_review_reviews.json").to_owned(),
        threads,
        include_str!("../fixtures/review_requests_response.json").to_owned(),
        include_str!("../fixtures/review_request_timeline_second_page.json").to_owned(),
        include_str!("../fixtures/unanchored_comments.json").to_owned(),
    ])
    .await;

    let change_request = provider(uri)
        .change_request(&repository(), ChangeRequestNumber::new(5).expect("number"))
        .await
        .expect("change request");

    assert_eq!(change_request.standalone_threads.len(), 1);
    assert_eq!(
        change_request.standalone_threads[0].id.as_str(),
        "PRRT_kwDOSCkZoc6LuYFt"
    );
    assert!(
        change_request
            .reviews
            .iter()
            .all(|item| item.findings.is_empty())
    );
}

#[tokio::test]
async fn code_review_domain_recovers_when_reviews_temporarily_lag_threads() {
    let mut lagging_reviews: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("../fixtures/code_review_reviews.json"))
            .expect("review fixture");
    lagging_reviews.remove(0);
    let lagging_reviews = serde_json::to_string(&lagging_reviews).expect("review response");

    let (uri, requests) = json_responses(vec![
        include_str!("../fixtures/pull_request.json").to_owned(),
        lagging_reviews,
        include_str!("../fixtures/review_threads_response.json").to_owned(),
        include_str!("../fixtures/code_review_reviews.json").to_owned(),
        include_str!("../fixtures/review_threads_response.json").to_owned(),
        include_str!("../fixtures/review_requests_response.json").to_owned(),
        include_str!("../fixtures/review_request_timeline_second_page.json").to_owned(),
        include_str!("../fixtures/unanchored_comments.json").to_owned(),
    ])
    .await;

    let change_request = provider(uri)
        .change_request(&repository(), ChangeRequestNumber::new(5).expect("number"))
        .await
        .expect("change request");

    assert_eq!(change_request.reviews.len(), 11);
    assert_eq!(change_request.reviews[0].findings.len(), 1);
    let requests = requests.await.expect("captured requests");
    assert_eq!(requests.len(), 8);
    assert_user_request(
        &requests[3],
        "GET /repos/civitas-forge/interprex-sandbox/pulls/5/reviews?per_page=100 ",
    );
    assert_user_request(&requests[4], "POST /graphql ");
}

fn check_run_page(names: impl IntoIterator<Item = String>, total_count: usize) -> String {
    let runs = names
        .into_iter()
        .map(|name| {
            serde_json::json!({
                "name": name,
                "head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "status": "completed",
                "conclusion": "success",
                "completed_at": "2026-08-24T10:04:00Z",
                "app": { "id": 1042, "slug": "quality-app", "name": "Quality App" },
                "html_url": "https://github.invalid/runs/1",
                "output": { "title": name, "summary": "settled" }
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({ "total_count": total_count, "check_runs": runs }).to_string()
}

#[tokio::test]
async fn code_review_domain_returns_check_runs_from_every_page() {
    let full_page = check_run_page((1..=100).map(|index| format!("check-{index}")), 101);
    // The last page repeats a name from the first page, as a second check
    // suite on the same commit does.
    let last_page = serde_json::json!({
        "total_count": 101,
        "check_runs": [{
            "name": "check-1",
            "head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "status": "in_progress",
            "conclusion": null,
            "completed_at": null,
            "app": { "id": 2087, "slug": "rerun-app", "name": "Rerun App" },
            "html_url": null,
            "output": { "title": null, "summary": null }
        }]
    })
    .to_string();
    let (uri, requests) = json_responses(vec![full_page, last_page]).await;

    let runs = provider(uri)
        .checks(&repository(), "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .await
        .expect("check runs");

    assert_eq!(runs.len(), 101);
    assert_eq!(runs[0].name, "check-1");
    assert_eq!(
        runs[0]
            .via_app
            .as_ref()
            .map(|app| app.id.as_str().to_owned()),
        Some("1042".to_owned()),
        "the publishing app is the identifier a ruleset names as integration_id"
    );
    assert_eq!(
        runs[0].status,
        CheckStatus::Completed {
            conclusion: CheckConclusion::Success,
            completed_at: "2026-08-24T10:04:00Z".parse().expect("completion time"),
        }
    );
    assert_eq!(
        runs[100].name, "check-1",
        "a run sharing a name with another is returned, not collapsed into it"
    );
    assert_eq!(runs[100].status, CheckStatus::InProgress);
    assert_eq!(
        runs[100]
            .via_app
            .as_ref()
            .map(|app| app.id.as_str().to_owned()),
        Some("2087".to_owned())
    );

    let requests = timeout(Duration::from_secs(1), requests)
        .await
        .expect("provider read both pages")
        .expect("captured requests");
    assert_eq!(requests.len(), 2);
    assert_user_request(
        &requests[0],
        "GET /repos/civitas-forge/interprex-sandbox/commits/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/check-runs?per_page=100&page=1&filter=latest ",
    );
    assert_user_request(
        &requests[1],
        "GET /repos/civitas-forge/interprex-sandbox/commits/aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/check-runs?per_page=100&page=2&filter=latest ",
    );
}

#[tokio::test]
async fn code_review_domain_stops_reading_check_runs_at_the_reported_total() {
    let full_page = check_run_page((1..=100).map(|index| format!("check-{index}")), 100);
    let (uri, requests) = json_responses(vec![full_page]).await;

    let runs = provider(uri)
        .checks(&repository(), "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .await
        .expect("check runs");

    assert_eq!(runs.len(), 100);
    assert_eq!(
        timeout(Duration::from_secs(1), requests)
            .await
            .expect("provider read one page")
            .expect("captured requests")
            .len(),
        1
    );
}
