use interprex::{IssueNumber, IssuesProvider};

use super::http_fixture::{assert_user_request, provider, repository, rest_pages, server};

#[tokio::test]
async fn tracker_domain_addresses_the_requested_issue_number() {
    let (uri, request) = server(
        "200 OK",
        "application/json",
        include_str!("../fixtures/issue.json"),
    )
    .await;
    provider(uri)
        .issue(&repository(), IssueNumber::new(11).expect("number"))
        .await
        .expect("issue");
    assert_user_request(
        &request.await.expect("captured request"),
        "GET /repos/civitas-forge/interprex-sandbox/issues/11 ",
    );
}

#[tokio::test]
async fn tracker_domain_returns_labels_from_every_rest_page() {
    let route = "/repos/civitas-forge/interprex-sandbox/labels";
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
            .starts_with("GET /repos/civitas-forge/interprex-sandbox/labels?per_page=100&page=2 ")
    );
}
