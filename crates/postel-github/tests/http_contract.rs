//! Transport contract tests run Octocrab against a one-request local server.
//!
//! These tests do not duplicate Octocrab's HTTP implementation. They prove the
//! Postel-owned endpoint choice, parameters, identity, and response
//! normalization for each domain before a request would reach GitHub.

use std::{collections::BTreeMap, time::Duration};

use postel_contracts::{JobsDomain, PrDomain, ReleasesDomain, RepoDomain, TrackerDomain};
use postel_github::{GithubConfig, from_config};
use postel_model::{DispatchInputs, IssueNumber, PullRequestNumber, Repository};
use secrecy::SecretString;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
    time::timeout,
};

async fn server(
    status: &'static str,
    content_type: &'static str,
    body: &'static str,
) -> (String, oneshot::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("local address");
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept request");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = stream.read(&mut buffer).await.expect("read request");
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            if complete_request(&request) {
                break;
            }
        }
        let response = format!(
            "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("write response");
        sender
            .send(String::from_utf8(request).expect("request is UTF-8"))
            .ok();
    });
    (format!("http://{address}"), receiver)
}

async fn rest_pages(
    route: &'static str,
    bodies: Vec<&'static str>,
) -> (String, oneshot::Receiver<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("local address");
    let base_uri = format!("http://{address}");
    let next_base = base_uri.clone();
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(async move {
        let page_count = bodies.len();
        let mut requests = Vec::with_capacity(page_count);
        for (index, body) in bodies.into_iter().enumerate() {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = stream.read(&mut buffer).await.expect("read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if complete_request(&request) {
                    break;
                }
            }
            requests.push(String::from_utf8(request).expect("request is UTF-8"));
            let link = if index + 1 < page_count {
                format!(
                    "link: <{next_base}{route}?per_page=100&page={}>; rel=\"next\"\r\n",
                    index + 2
                )
            } else {
                String::new()
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n{link}content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write response");
        }
        sender.send(requests).ok();
    });
    (base_uri, receiver)
}

async fn json_responses(bodies: Vec<&'static str>) -> (String, oneshot::Receiver<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("local address");
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(async move {
        let mut requests = Vec::with_capacity(bodies.len());
        for body in bodies {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = stream.read(&mut buffer).await.expect("read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if complete_request(&request) {
                    break;
                }
            }
            requests.push(String::from_utf8(request).expect("request is UTF-8"));
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write response");
        }
        sender.send(requests).ok();
    });
    (format!("http://{address}"), receiver)
}

fn complete_request(request: &[u8]) -> bool {
    let text = String::from_utf8_lossy(request);
    let Some((headers, body)) = text.split_once("\r\n\r\n") else {
        return false;
    };
    let length = headers
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length: ")
                .map(str::to_owned)
        })
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    body.len() >= length
}

async fn provider(base_uri: String) -> postel_github::GithubProvider {
    from_config(GithubConfig {
        gh_token: Some(SecretString::from("transport-test-token")),
        base_uri: Some(base_uri.clone()),
        upload_uri: Some(base_uri),
        ..GithubConfig::default()
    })
    .await
    .expect("provider")
}

fn repository() -> Repository {
    Repository::new("faictor", "postel-sandbox").expect("repository")
}

fn assert_user_request(request: &str, method_and_path: &str) {
    assert!(request.starts_with(method_and_path), "{request}");
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer transport-test-token"),
        "{request}"
    );
}

#[tokio::test]
async fn repo_domain_sends_the_canonical_repository_address() {
    let (uri, request) = server(
        "200 OK",
        "application/json",
        include_str!("fixtures/repository.json"),
    )
    .await;
    let settings = provider(uri)
        .await
        .settings(&repository())
        .await
        .expect("settings");
    assert!(settings.allow_squash_merge);
    assert_user_request(
        &request.await.expect("captured request"),
        "GET /repos/faictor/postel-sandbox ",
    );
}

#[tokio::test]
async fn repo_domain_returns_rulesets_from_every_rest_page() {
    let route = "/repos/faictor/postel-sandbox/rulesets";
    let (uri, requests) = rest_pages(
        route,
        vec![
            r#"[{"id":1,"name":"first","enforcement":"active","conditions":{},"rules":[]}]"#,
            r#"[{"id":2,"name":"second","enforcement":"disabled","conditions":{},"rules":[]}]"#,
        ],
    )
    .await;
    let rulesets = provider(uri)
        .await
        .rulesets(&repository())
        .await
        .expect("rulesets");
    assert_eq!(
        rulesets
            .iter()
            .map(|ruleset| ruleset.name.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert!(
        requests.await.expect("captured requests")[1]
            .starts_with("GET /repos/faictor/postel-sandbox/rulesets?per_page=100&page=2 ")
    );
}

#[tokio::test]
async fn tracker_domain_addresses_the_requested_issue_number() {
    let (uri, request) = server(
        "200 OK",
        "application/json",
        include_str!("fixtures/issue.json"),
    )
    .await;
    provider(uri)
        .await
        .issue(&repository(), IssueNumber::new(11).expect("number"))
        .await
        .expect("issue");
    assert_user_request(
        &request.await.expect("captured request"),
        "GET /repos/faictor/postel-sandbox/issues/11 ",
    );
}

#[tokio::test]
async fn tracker_domain_returns_labels_from_every_rest_page() {
    let route = "/repos/faictor/postel-sandbox/labels";
    let (uri, requests) = rest_pages(
        route,
        vec![
            r#"[{"name":"first","color":"111111","description":null}]"#,
            r#"[{"name":"second","color":"222222","description":null}]"#,
        ],
    )
    .await;
    let labels = provider(uri)
        .await
        .labels(&repository())
        .await
        .expect("labels");
    assert_eq!(
        labels
            .iter()
            .map(|label| label.name.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert!(
        requests.await.expect("captured requests")[1]
            .starts_with("GET /repos/faictor/postel-sandbox/labels?per_page=100&page=2 ")
    );
}

#[tokio::test]
async fn pr_domain_requests_copilot_through_the_login_mutation() {
    let (uri, requests) = json_responses(vec![
        include_str!("fixtures/pull_request.json"),
        r#"{"data":{"requestReviewsByLogin":{"pullRequest":{"id":"PR_kwDOExample"}}}}"#,
    ])
    .await;
    provider(uri)
        .await
        .request_reviewers(
            &repository(),
            PullRequestNumber::new(5).expect("number"),
            &[
                "alice".to_owned(),
                "copilot-pull-request-reviewer[bot]".to_owned(),
            ],
        )
        .await
        .expect("review request");
    let requests = timeout(Duration::from_secs(1), requests)
        .await
        .expect("provider sent both requests")
        .expect("captured requests");
    assert_user_request(&requests[0], "GET /repos/faictor/postel-sandbox/pulls/5 ");
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
}

#[tokio::test]
async fn pr_domain_returns_review_threads_from_every_graphql_page() {
    let (uri, requests) = json_responses(vec![
        r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"thread-1","isResolved":false,"path":"src/lib.rs","line":10,"comments":{"nodes":[{"body":"first","author":{"login":"alice"}}]}}],"pageInfo":{"hasNextPage":true,"endCursor":"cursor-1"}}}}}}"#,
        r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{"nodes":[{"id":"thread-2","isResolved":true,"path":"src/lib.rs","line":20,"comments":{"nodes":[{"body":"second","author":{"login":"bob"}}]}}],"pageInfo":{"hasNextPage":false,"endCursor":null}}}}}}"#,
    ])
    .await;
    let threads = provider(uri)
        .await
        .review_threads(&repository(), PullRequestNumber::new(5).expect("number"))
        .await
        .expect("review threads");
    assert_eq!(
        threads
            .iter()
            .map(|thread| thread.id.as_str())
            .collect::<Vec<_>>(),
        ["thread-1", "thread-2"]
    );
    assert!(requests.await.expect("captured requests")[1].contains("\"cursor\":\"cursor-1\""));
}

#[tokio::test]
async fn jobs_domain_dispatches_the_named_ref_and_inputs() {
    let (uri, request) = server("204 No Content", "application/json", "").await;
    let inputs = DispatchInputs(BTreeMap::from([(
        "tier".to_owned(),
        serde_json::Value::String("pull-request".to_owned()),
    )]));
    provider(uri)
        .await
        .dispatch(&repository(), "quality.yml", "main", &inputs)
        .await
        .expect("dispatch");
    let request = request.await.expect("captured request");
    assert_user_request(
        &request,
        "POST /repos/faictor/postel-sandbox/actions/workflows/quality.yml/dispatches ",
    );
    assert!(request.contains("\"ref\":\"main\""));
    assert!(request.contains("\"tier\":\"pull-request\""));
}

#[tokio::test]
async fn releases_domain_reads_by_tag_without_vendor_types_escaping() {
    let (uri, request) = server(
        "200 OK",
        "application/json",
        include_str!("fixtures/release.json"),
    )
    .await;
    let release = provider(uri)
        .await
        .release_by_tag(&repository(), "v0.1.0")
        .await
        .expect("release");
    assert_eq!(release.tag, "v0.1.0");
    assert_user_request(
        &request.await.expect("captured request"),
        "GET /repos/faictor/postel-sandbox/releases/tags/v0.1.0 ",
    );
}
