use interprex::{
    AppliedSourceRequirementsProvider, CodeHostingProvider, CommitRange, ProviderError,
    RepositorySettings, SourceCodeConfigurationProvider,
};
use interprex_github::GithubRuleset;

use super::http_fixture::{assert_user_request, provider, repository, server};

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
async fn source_configuration_capabilities_fail_explicitly_until_implemented() {
    let provider = provider("http://127.0.0.1:1".to_owned());
    assert!(matches!(
        provider.read_rulesets(&repository()).await,
        Err(ProviderError::Unsupported {
            provider: "github",
            operation: "read source rulesets"
        })
    ));
    let ruleset: GithubRuleset = serde_json::from_value(serde_json::json!({
        "name": "main",
        "target": "branch",
        "enforcement": "active",
        "conditions": {"ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": []}},
        "rules": [],
        "bypass_actors": []
    }))
    .expect("ruleset");
    assert!(matches!(
        provider.apply_ruleset(&repository(), &ruleset).await,
        Err(ProviderError::Unsupported {
            provider: "github",
            operation: "apply source ruleset"
        })
    ));
    assert!(matches!(
        provider
            .applied_requirements(
                &repository(),
                "main",
                &CommitRange {
                    base_sha: "base".to_owned(),
                    head_sha: "head".to_owned(),
                },
            )
            .await,
        Err(ProviderError::Unsupported {
            provider: "github",
            operation: "read applied source requirements"
        })
    ));
}
