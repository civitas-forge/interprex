//! Transport contract tests run Octocrab against a one-request local server.
//!
//! These tests do not duplicate Octocrab's HTTP implementation. They prove the
//! Postel-owned endpoint choice, parameters, identity, and response
//! normalization for each domain before a request would reach GitHub.

use std::{collections::BTreeMap, time::Duration};

use bytes::Bytes;
use futures_util::{TryStreamExt, stream};
use postel::{
    AssetId, AssetStreamError, AssetUpload, CodeHostingProvider, CodeReviewNumber,
    CodeReviewsProvider, DispatchInputs, IssueNumber, IssuesProvider, JobsProvider, ReleaseId,
    ReleasesProvider, Repository, RepositorySettings, ReviewRequestTarget, ReviewThreadId,
};
use postel_github::{GithubConfig, from_config};
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

async fn json_responses<T>(bodies: Vec<T>) -> (String, oneshot::Receiver<Vec<String>>)
where
    T: Into<String> + Send + 'static,
{
    let bodies = bodies.into_iter().map(Into::into).collect::<Vec<String>>();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("local address");
    let base_uri = format!("http://{address}");
    let response_base = base_uri.clone();
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(async move {
        let mut requests = Vec::with_capacity(bodies.len());
        for body in bodies {
            let body = body.replace("{base}", &response_base);
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
    (base_uri, receiver)
}

async fn streaming_download_server() -> (String, oneshot::Sender<()>) {
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
            request.extend_from_slice(&buffer[..read]);
            if complete_request(&request) {
                break;
            }
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\ncontent-length: 11\r\nconnection: close\r\n\r\nhello ")
            .await
            .expect("write first chunk");
        receiver.await.expect("consumer read first chunk");
        stream
            .write_all(b"world")
            .await
            .expect("write second chunk");
    });
    (format!("http://{address}"), sender)
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

fn provider(base_uri: String) -> postel_github::GithubProvider {
    from_config(GithubConfig {
        gh_token: Some(SecretString::from("transport-test-token")),
        base_uri: Some(base_uri.clone()),
        upload_uri: Some(base_uri),
        ..GithubConfig::default()
    })
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
async fn code_hosting_domain_sends_the_canonical_repository_address() {
    let (uri, request) = server(
        "200 OK",
        "application/json",
        include_str!("fixtures/repository.json"),
    )
    .await;
    let settings = provider(uri)
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
async fn code_hosting_domain_maps_settings_into_the_github_request_body() {
    let (uri, request) = server(
        "200 OK",
        "application/json",
        include_str!("fixtures/repository.json"),
    )
    .await;
    provider(uri)
        .apply_settings(
            &repository(),
            &RepositorySettings {
                allow_squash_merge: false,
                allow_merge_commit: true,
                allow_rebase_merge: true,
                delete_branch_on_merge: false,
            },
        )
        .await
        .expect("apply settings");
    let request = request.await.expect("captured request");
    assert_user_request(&request, "PATCH /repos/faictor/postel-sandbox ");
    let (_, body) = request.split_once("\r\n\r\n").expect("request body");
    let body: serde_json::Value = serde_json::from_str(body).expect("JSON request body");
    assert_eq!(
        body,
        serde_json::json!({
            "allow_squash_merge": false,
            "allow_merge_commit": true,
            "allow_rebase_merge": true,
            "delete_branch_on_merge": false,
        })
    );
}

#[tokio::test]
async fn code_hosting_domain_returns_rulesets_from_every_rest_page() {
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
    let labels = provider(uri).labels(&repository()).await.expect("labels");
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
async fn code_review_domain_requests_users_bots_and_teams_through_the_login_mutation() {
    let (uri, requests) = json_responses(vec![
        include_str!("fixtures/pull_request.json"),
        r#"{"data":{"requestReviewsByLogin":{"pullRequest":{"id":"PR_kwDOExample"}}}}"#,
    ])
    .await;
    provider(uri)
        .request_reviewers(
            &repository(),
            CodeReviewNumber::new(5).expect("number"),
            &[
                ReviewRequestTarget::User("alice".to_owned()),
                ReviewRequestTarget::Bot("copilot-pull-request-reviewer".to_owned()),
                ReviewRequestTarget::Team("faictor/maintainers".to_owned()),
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
    assert_eq!(
        body["variables"]["teamSlugs"],
        serde_json::json!(["faictor/maintainers"])
    );
}

#[tokio::test]
async fn code_review_domain_resolves_github_identity_before_marking_ready() {
    let (uri, requests) = json_responses(vec![
        include_str!("fixtures/pull_request.json"),
        r#"{"data":{"markPullRequestReadyForReview":{"pullRequest":{"id":"PR_kwDOExample","isDraft":false}}}}"#,
    ])
    .await;
    provider(uri)
        .mark_ready(&repository(), CodeReviewNumber::new(5).expect("number"))
        .await
        .expect("mark ready");
    let requests = requests.await.expect("captured requests");
    assert_user_request(&requests[0], "GET /repos/faictor/postel-sandbox/pulls/5 ");
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
            CodeReviewNumber::new(5).expect("number"),
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
async fn code_review_domain_reads_one_complete_observation() {
    let (uri, requests) = json_responses(vec![
        include_str!("fixtures/pull_request.json"),
        include_str!("fixtures/code_review_reviews.json"),
        include_str!("fixtures/review_threads_response.json"),
        include_str!("fixtures/review_requests_response.json"),
        include_str!("fixtures/conversation_comments.json"),
    ])
    .await;
    let review = provider(uri)
        .code_review(&repository(), CodeReviewNumber::new(5).expect("number"))
        .await
        .expect("code review");

    assert_eq!(review.reviews.len(), 9);
    assert_eq!(review.reviews[0].findings[0].replies.len(), 1);
    assert!(
        review
            .reviews
            .last()
            .expect("last review")
            .findings
            .is_empty()
    );
    assert_eq!(review.outstanding_requests.len(), 2);
    assert_eq!(review.conversation.len(), 1);
    let requests = requests.await.expect("captured requests");
    assert_user_request(&requests[0], "GET /repos/faictor/postel-sandbox/pulls/5 ");
    assert_user_request(
        &requests[1],
        "GET /repos/faictor/postel-sandbox/pulls/5/reviews?per_page=100 ",
    );
    assert_user_request(&requests[2], "POST /graphql ");
    assert_user_request(&requests[3], "POST /graphql ");
    assert_user_request(
        &requests[4],
        "GET /repos/faictor/postel-sandbox/issues/5/comments?per_page=100 ",
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
}

#[tokio::test]
async fn code_review_domain_recovers_when_reviews_temporarily_lag_threads() {
    let mut lagging_reviews: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/code_review_reviews.json"))
            .expect("review fixture");
    lagging_reviews.remove(0);
    let lagging_reviews = serde_json::to_string(&lagging_reviews).expect("review response");

    let (uri, requests) = json_responses(vec![
        include_str!("fixtures/pull_request.json").to_owned(),
        lagging_reviews,
        include_str!("fixtures/review_threads_response.json").to_owned(),
        include_str!("fixtures/code_review_reviews.json").to_owned(),
        include_str!("fixtures/review_threads_response.json").to_owned(),
        include_str!("fixtures/review_requests_response.json").to_owned(),
        include_str!("fixtures/conversation_comments.json").to_owned(),
    ])
    .await;

    let review = provider(uri)
        .code_review(&repository(), CodeReviewNumber::new(5).expect("number"))
        .await
        .expect("code review");

    assert_eq!(review.reviews.len(), 9);
    assert_eq!(review.reviews[0].findings.len(), 1);
    let requests = requests.await.expect("captured requests");
    assert_eq!(requests.len(), 7);
    assert_user_request(
        &requests[3],
        "GET /repos/faictor/postel-sandbox/pulls/5/reviews?per_page=100 ",
    );
    assert_user_request(&requests[4], "POST /graphql ");
}

#[tokio::test]
async fn jobs_domain_dispatches_the_named_ref_and_inputs() {
    let (uri, request) = server("204 No Content", "application/json", "").await;
    let inputs = DispatchInputs(BTreeMap::from([(
        "tier".to_owned(),
        serde_json::Value::String("pull-request".to_owned()),
    )]));
    provider(uri)
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
        .release_by_tag(&repository(), "v0.1.0")
        .await
        .expect("release");
    assert_eq!(release.tag, "v0.1.0");
    assert_user_request(
        &request.await.expect("captured request"),
        "GET /repos/faictor/postel-sandbox/releases/tags/v0.1.0 ",
    );
}

#[tokio::test]
async fn releases_domain_streams_upload_chunks_to_the_upload_host() {
    let (uri, requests) = json_responses(vec![
        r#"{"url":"{base}/releases/88","html_url":"{base}/releases/88","assets_url":"{base}/releases/88/assets","upload_url":"{base}/repos/faictor/postel-sandbox/releases/88/assets{?name,label}","id":88,"node_id":"R_kwDOExample","tag_name":"v0.1.0","target_commitish":"main","name":null,"body":null,"draft":true,"prerelease":false,"assets":[]}"#,
        r#"{"id":99,"name":"postel.tar.gz","label":"Darwin arm64","size":11,"browser_download_url":"https://example.invalid/postel.tar.gz"}"#,
    ])
    .await;
    let upload = AssetUpload::new(
        11,
        stream::iter([
            Ok::<_, AssetStreamError>(Bytes::from_static(b"hello ")),
            Ok(Bytes::from_static(b"world")),
        ]),
    );
    let asset = provider(uri)
        .upload_asset(
            &repository(),
            ReleaseId::new(88).expect("release id"),
            "postel.tar.gz",
            Some("Darwin arm64"),
            upload,
        )
        .await
        .expect("upload asset");
    assert_eq!(asset.size, 11);
    let requests = requests.await.expect("captured requests");
    assert_user_request(
        &requests[0],
        "GET /repos/faictor/postel-sandbox/releases/88 ",
    );
    assert_user_request(
        &requests[1],
        "POST /repos/faictor/postel-sandbox/releases/88/assets?name=postel%2Etar%2Egz&label=Darwin%20arm64 ",
    );
    assert!(
        requests[1]
            .to_ascii_lowercase()
            .contains("content-length: 11")
    );
    assert!(requests[1].ends_with("hello world"));
}

#[tokio::test]
async fn releases_domain_returns_download_before_the_final_chunk_arrives() {
    let (uri, continue_download) = streaming_download_server().await;
    let mut download = timeout(
        Duration::from_secs(1),
        provider(uri).download_asset(&repository(), AssetId::new(99).expect("asset id")),
    )
    .await
    .expect("download opens before the complete body arrives")
    .expect("download stream");
    let first = timeout(Duration::from_secs(1), download.try_next())
        .await
        .expect("first chunk arrives")
        .expect("first chunk read")
        .expect("first chunk exists");
    assert_eq!(first, Bytes::from_static(b"hello "));
    continue_download.send(()).expect("continue download");
    let remaining: Vec<Bytes> = download.try_collect().await.expect("remaining chunks");
    assert_eq!(remaining.concat(), b"world");
}
