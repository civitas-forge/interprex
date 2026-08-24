//! Transport contract tests run Octocrab against a one-request local server.
//!
//! These tests do not duplicate Octocrab's HTTP implementation. They prove the
//! Postel-owned endpoint choice, parameters, identity, and response
//! normalization for each domain before a request would reach GitHub.

use std::collections::BTreeMap;

use postel_contracts::{JobsDomain, PrDomain, ReleasesDomain, RepoDomain, TrackerDomain};
use postel_github::{GithubConfig, from_config};
use postel_model::{DispatchInputs, IssueNumber, PullRequestNumber, Repository};
use secrecy::SecretString;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
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
async fn pr_domain_passes_the_exact_reviewer_set() {
    let (uri, request) = server("201 Created", "application/json", "{}").await;
    provider(uri)
        .await
        .request_reviewers(
            &repository(),
            PullRequestNumber::new(5).expect("number"),
            &["copilot-pull-request-reviewer[bot]".to_owned()],
        )
        .await
        .expect("review request");
    let request = request.await.expect("captured request");
    assert_user_request(
        &request,
        "POST /repos/faictor/postel-sandbox/pulls/5/requested_reviewers ",
    );
    assert!(request.contains("copilot-pull-request-reviewer[bot]"));
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
