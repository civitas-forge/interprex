use interprex::{CodeHostingProvider, RepositorySettings};

use super::http_fixture::{assert_user_request, provider, repository, rest_pages, server};

#[tokio::test]
async fn code_hosting_domain_sends_the_canonical_repository_address() {
    let (uri, request) = server(
        "200 OK",
        "application/json",
        include_str!("../fixtures/repository.json"),
    )
    .await;
    let settings = provider(uri)
        .settings(&repository())
        .await
        .expect("settings");
    assert!(settings.allow_squash_merge);
    assert_user_request(
        &request.await.expect("captured request"),
        "GET /repos/civitas-forge/interprex-sandbox ",
    );
}

#[tokio::test]
async fn code_hosting_domain_maps_settings_into_the_github_request_body() {
    let (uri, request) = server(
        "200 OK",
        "application/json",
        include_str!("../fixtures/repository.json"),
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
    assert_user_request(&request, "PATCH /repos/civitas-forge/interprex-sandbox ");
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
    let route = "/repos/civitas-forge/interprex-sandbox/rulesets";
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
        requests.await.expect("captured requests")[1].starts_with(
            "GET /repos/civitas-forge/interprex-sandbox/rulesets?per_page=100&page=2 "
        )
    );
}
