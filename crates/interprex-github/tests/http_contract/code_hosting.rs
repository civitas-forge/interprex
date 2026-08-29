use interprex::{
    AppliedSourceRequirementsProvider, CodeHostingProvider, CommitRange, ProviderError,
    RepositorySettings, SourceCodeConfigurationProvider,
};
use interprex_github::GithubRuleset;

use super::http_fixture::{
    ScriptedResponse, assert_user_request, provider, repository, scripted_responses, server,
};

fn summary(id: u64, target: &str, source_type: &str, source: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "name": format!("ruleset-{id}"),
        "target": target,
        "source_type": source_type,
        "source": source,
        "enforcement": "active",
        "node_id": format!("RRS_{id}")
    })
}

fn detail(id: u64, target: &str, source_type: &str, source: &str) -> serde_json::Value {
    let mut value = summary(id, target, source_type, source);
    let object = value.as_object_mut().expect("ruleset object");
    object.insert(
        "bypass_actors".to_owned(),
        serde_json::json!([{
            "actor_id": 15368,
            "actor_type": "Integration",
            "bypass_mode": "pull_request"
        }]),
    );
    object.insert(
        "conditions".to_owned(),
        serde_json::json!({
            "ref_name": {
                "include": ["~DEFAULT_BRANCH"],
                "exclude": ["refs/heads/release/*"]
            }
        }),
    );
    object.insert(
        "rules".to_owned(),
        serde_json::json!([{
            "type": "required_status_checks",
            "parameters": {
                "strict_required_status_checks_policy": true,
                "do_not_enforce_on_create": false,
                "required_status_checks": [{"context": "quality", "integration_id": 15368}]
            }
        }, {
            "type": "future_rule",
            "parameters": {"future_option": true}
        }]),
    );
    object.insert(
        "current_user_can_bypass".to_owned(),
        serde_json::json!("never"),
    );
    value
}

fn desired_ruleset(id: Option<u64>) -> GithubRuleset {
    let mut value = detail(
        id.unwrap_or(7),
        "branch",
        "Repository",
        "civitas-forge/interprex-sandbox",
    );
    value["rules"]
        .as_array_mut()
        .expect("rules")
        .pop()
        .expect("future rule");
    if id.is_none() {
        let object = value.as_object_mut().expect("ruleset object");
        object.remove("id");
        object.remove("source_type");
        object.remove("source");
        object.remove("node_id");
        object.remove("current_user_can_bypass");
    }
    serde_json::from_value(value).expect("desired ruleset")
}

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
async fn source_configuration_expands_summaries_and_preserves_complete_details() {
    let mut list_item = summary(7, "branch", "Repository", "civitas-forge/interprex-sandbox");
    list_item
        .as_object_mut()
        .expect("summary object")
        .remove("target");
    let list = serde_json::json!([list_item]);
    let complete = detail(7, "branch", "Repository", "civitas-forge/interprex-sandbox");
    let (uri, requests) = scripted_responses(vec![
        ScriptedResponse::json(list.to_string()),
        ScriptedResponse::json(complete.clone().to_string()),
    ])
    .await;

    let rulesets = provider(uri)
        .read_rulesets(&repository())
        .await
        .expect("complete rulesets");

    assert_eq!(rulesets.len(), 1);
    assert_eq!(
        serde_json::to_value(&rulesets[0]).expect("serialize ruleset"),
        complete
    );
    let requests = requests.await.expect("captured requests");
    assert_user_request(
        &requests[0],
        "GET /repos/civitas-forge/interprex-sandbox/rulesets?per_page=100&includes_parents=true ",
    );
    assert_user_request(
        &requests[1],
        "GET /repos/civitas-forge/interprex-sandbox/rulesets/7?includes_parents=true ",
    );
}

#[tokio::test]
async fn source_configuration_paginates_summaries_before_expanding_in_order() {
    let first = summary(8, "tag", "Repository", "civitas-forge/interprex-sandbox");
    let second = summary(7, "branch", "Repository", "civitas-forge/interprex-sandbox");
    let (uri, requests) = scripted_responses(vec![
        ScriptedResponse::json(serde_json::json!([first]).to_string()).with_header(
            "link: <{base}/repos/civitas-forge/interprex-sandbox/rulesets?per_page=100&includes_parents=true&page=2>; rel=\"next\"",
        ),
        ScriptedResponse::json(serde_json::json!([second]).to_string()),
        ScriptedResponse::json(
            detail(
                7,
                "branch",
                "Repository",
                "civitas-forge/interprex-sandbox",
            )
            .to_string(),
        ),
        ScriptedResponse::json(
            detail(
                8,
                "tag",
                "Repository",
                "civitas-forge/interprex-sandbox",
            )
            .to_string(),
        ),
    ])
    .await;

    let rulesets = provider(uri)
        .read_rulesets(&repository())
        .await
        .expect("complete rulesets");
    assert_eq!(
        rulesets
            .iter()
            .map(|ruleset| ruleset.id)
            .collect::<Vec<_>>(),
        [Some(7), Some(8)]
    );
    let requests = requests.await.expect("captured requests");
    assert_user_request(
        &requests[1],
        "GET /repos/civitas-forge/interprex-sandbox/rulesets?per_page=100&includes_parents=true&page=2 ",
    );
    assert_user_request(
        &requests[2],
        "GET /repos/civitas-forge/interprex-sandbox/rulesets/7?includes_parents=true ",
    );
    assert_user_request(
        &requests[3],
        "GET /repos/civitas-forge/interprex-sandbox/rulesets/8?includes_parents=true ",
    );
}

#[tokio::test]
async fn source_configuration_inventory_order_is_independent_of_response_order() {
    let seven = summary(7, "branch", "Repository", "civitas-forge/interprex-sandbox");
    let eight = summary(8, "tag", "Repository", "civitas-forge/interprex-sandbox");
    let mut observations = Vec::new();

    for list in [
        serde_json::json!([eight.clone(), seven.clone()]),
        serde_json::json!([seven.clone(), eight.clone()]),
    ] {
        let (uri, _) = scripted_responses(vec![
            ScriptedResponse::json(list.to_string()),
            ScriptedResponse::json(
                detail(7, "branch", "Repository", "civitas-forge/interprex-sandbox").to_string(),
            ),
            ScriptedResponse::json(
                detail(8, "tag", "Repository", "civitas-forge/interprex-sandbox").to_string(),
            ),
        ])
        .await;
        observations.push(
            provider(uri)
                .read_rulesets(&repository())
                .await
                .expect("ordered rulesets"),
        );
    }

    assert_eq!(observations[0], observations[1]);
    assert_eq!(
        observations[0]
            .iter()
            .map(|ruleset| ruleset.id)
            .collect::<Vec<_>>(),
        [Some(7), Some(8)]
    );
}

#[tokio::test]
async fn source_configuration_keeps_every_github_target_distinct() {
    let targets = ["branch", "tag", "push", "repository"];
    let list = targets
        .iter()
        .enumerate()
        .map(|(index, target)| {
            summary(
                10 + index as u64,
                target,
                "Repository",
                "civitas-forge/interprex-sandbox",
            )
        })
        .collect::<Vec<_>>();
    let mut responses = vec![ScriptedResponse::json(
        serde_json::to_string(&list).expect("list"),
    )];
    responses.extend(targets.iter().enumerate().map(|(index, target)| {
        ScriptedResponse::json(
            detail(
                10 + index as u64,
                target,
                "Repository",
                "civitas-forge/interprex-sandbox",
            )
            .to_string(),
        )
    }));
    let (uri, _) = scripted_responses(responses).await;

    let rulesets = provider(uri)
        .read_rulesets(&repository())
        .await
        .expect("all targets");
    assert_eq!(
        rulesets
            .iter()
            .map(|ruleset| ruleset.target.as_deref().expect("target"))
            .collect::<Vec<_>>(),
        targets
    );
}

#[tokio::test]
async fn source_configuration_reads_complete_inherited_rulesets() {
    let list = serde_json::json!([summary(21, "branch", "Organization", "civitas-forge")]);
    let mut complete = detail(21, "branch", "Organization", "civitas-forge");
    complete["conditions"]["repository_name"] = serde_json::json!({
        "include": ["important-*"],
        "exclude": ["archive-*"],
        "protected": true
    });
    let (uri, _) = scripted_responses(vec![
        ScriptedResponse::json(list.to_string()),
        ScriptedResponse::json(complete.to_string()),
    ])
    .await;

    let rulesets = provider(uri)
        .read_rulesets(&repository())
        .await
        .expect("inherited ruleset detail");
    assert_eq!(rulesets[0].source_type.as_deref(), Some("Organization"));
    assert_eq!(rulesets[0].source.as_deref(), Some("civitas-forge"));
    assert_eq!(
        serde_json::to_value(&rulesets[0]).expect("inherited ruleset"),
        complete
    );
}

#[tokio::test]
async fn source_configuration_reports_missing_and_partial_detail_reads() {
    for response in [
        ScriptedResponse::status("404 Not Found", r#"{"message":"Not Found"}"#),
        ScriptedResponse::status("500 Internal Server Error", r#"{"message":"failure"}"#),
    ] {
        let list = serde_json::json!([
            summary(7, "branch", "Repository", "civitas-forge/interprex-sandbox"),
            summary(8, "tag", "Repository", "civitas-forge/interprex-sandbox")
        ]);
        let (uri, requests) = scripted_responses(vec![
            ScriptedResponse::json(list.to_string()),
            ScriptedResponse::json(
                detail(7, "branch", "Repository", "civitas-forge/interprex-sandbox").to_string(),
            ),
            response,
        ])
        .await;

        let error = provider(uri)
            .read_rulesets(&repository())
            .await
            .expect_err("detail failure");
        assert!(matches!(
            error,
            ProviderError::NotFound { .. } | ProviderError::External { .. }
        ));
        assert_eq!(requests.await.expect("captured requests").len(), 3);
    }
}

#[tokio::test]
async fn source_configuration_reports_malformed_detail_without_defaulting() {
    let list = serde_json::json!([summary(
        7,
        "branch",
        "Repository",
        "civitas-forge/interprex-sandbox"
    )]);
    let (uri, requests) = scripted_responses(vec![
        ScriptedResponse::json(list.to_string()),
        ScriptedResponse::json(r#"{"id":7,"name":"ruleset-7"}"#),
    ])
    .await;

    assert!(matches!(
        provider(uri).read_rulesets(&repository()).await,
        Err(ProviderError::Unrepresentable {
            provider: "github",
            ..
        })
    ));
    assert_eq!(requests.await.expect("captured requests").len(), 2);
}

#[tokio::test]
async fn source_configuration_rejects_details_hidden_by_permissions() {
    let list = serde_json::json!([summary(
        7,
        "branch",
        "Repository",
        "civitas-forge/interprex-sandbox"
    )]);
    let mut incomplete = detail(7, "branch", "Repository", "civitas-forge/interprex-sandbox");
    incomplete
        .as_object_mut()
        .expect("ruleset object")
        .remove("bypass_actors");
    let (uri, _) = scripted_responses(vec![
        ScriptedResponse::json(list.to_string()),
        ScriptedResponse::json(incomplete.to_string()),
    ])
    .await;

    assert!(matches!(
        provider(uri).read_rulesets(&repository()).await,
        Err(ProviderError::Unsupported {
            provider: "github",
            operation: "read complete rulesets without bypass-actor access"
        })
    ));
}

#[tokio::test]
async fn source_configuration_creates_and_updates_complete_desired_rulesets() {
    for (desired, method_and_path, write_status) in [
        (
            desired_ruleset(None),
            "POST /repos/civitas-forge/interprex-sandbox/rulesets ",
            "201 Created",
        ),
        (
            desired_ruleset(Some(7)),
            "PUT /repos/civitas-forge/interprex-sandbox/rulesets/7 ",
            "200 OK",
        ),
    ] {
        let returned = desired_ruleset(Some(7));
        let (uri, requests) = scripted_responses(vec![
            ScriptedResponse::status(write_status, r#"{"id":7}"#),
            ScriptedResponse::json(
                serde_json::to_value(&returned)
                    .expect("returned ruleset")
                    .to_string(),
            ),
        ])
        .await;
        let applied = provider(uri)
            .apply_ruleset(&repository(), &desired)
            .await
            .expect("applied ruleset");
        assert_eq!(applied.id, Some(7));

        let requests = requests.await.expect("captured requests");
        assert_eq!(requests.len(), 2);
        assert_user_request(&requests[0], method_and_path);
        assert_user_request(
            &requests[1],
            "GET /repos/civitas-forge/interprex-sandbox/rulesets/7?includes_parents=true ",
        );
        let (_, body) = requests[0].split_once("\r\n\r\n").expect("request body");
        let body: serde_json::Value = serde_json::from_str(body).expect("JSON request body");
        assert_eq!(
            body,
            serde_json::json!({
                "name": "ruleset-7",
                "target": "branch",
                "enforcement": "active",
                "bypass_actors": [{
                    "actor_id": 15368,
                    "actor_type": "Integration",
                    "bypass_mode": "pull_request"
                }],
                "conditions": {
                    "ref_name": {
                        "include": ["~DEFAULT_BRANCH"],
                        "exclude": ["refs/heads/release/*"]
                    }
                },
                "rules": [{
                    "type": "required_status_checks",
                    "parameters": {
                        "strict_required_status_checks_policy": true,
                        "do_not_enforce_on_create": false,
                        "required_status_checks": [{"context": "quality", "integration_id": 15368}]
                    }
                }]
            })
        );
    }
}

#[tokio::test]
async fn source_configuration_refuses_unknown_writable_fields_before_transport() {
    let provider = provider("http://127.0.0.1:1".to_owned());
    let mut unknown_rule = desired_ruleset(Some(7));
    unknown_rule.rules.as_mut().expect("rules").push(
        serde_json::from_value(serde_json::json!({"type": "future_rule"})).expect("unknown rule"),
    );
    let mut unknown_field = desired_ruleset(Some(7));
    unknown_field
        .additional
        .insert("future_writable_field".to_owned(), serde_json::json!(true));

    for ruleset in [unknown_rule, unknown_field] {
        assert!(matches!(
            provider.apply_ruleset(&repository(), &ruleset).await,
            Err(ProviderError::Unsupported {
                provider: "github",
                ..
            })
        ));
    }
}

#[tokio::test]
async fn source_configuration_rejects_incomplete_or_misdirected_desired_rulesets() {
    let provider = provider("http://127.0.0.1:1".to_owned());
    let mut missing_bypass_mode = desired_ruleset(Some(7));
    missing_bypass_mode.bypass_actors.as_mut().expect("actors")[0]
        .fields
        .remove("bypass_mode");
    let mut incomplete_ref_condition = desired_ruleset(Some(7));
    incomplete_ref_condition
        .conditions
        .as_mut()
        .expect("conditions")
        .ref_name
        .as_mut()
        .expect("ref-name condition")
        .exclude = None;
    let mut other_repository = desired_ruleset(Some(7));
    other_repository.source = Some("civitas-forge/other".to_owned());

    for ruleset in [
        missing_bypass_mode,
        incomplete_ref_condition,
        other_repository,
    ] {
        assert!(matches!(
            provider.apply_ruleset(&repository(), &ruleset).await,
            Err(ProviderError::InvalidInput {
                provider: "github",
                ..
            })
        ));
    }
}

#[tokio::test]
async fn source_configuration_refuses_a_lossy_accepted_ruleset() {
    let desired = desired_ruleset(Some(7));
    let mut observed = desired_ruleset(Some(7));
    observed.conditions = None;
    let (uri, requests) = scripted_responses(vec![
        ScriptedResponse::status("200 OK", r#"{"id":7}"#),
        ScriptedResponse::json(
            serde_json::to_value(observed)
                .expect("observed ruleset")
                .to_string(),
        ),
    ])
    .await;

    assert!(matches!(
        provider(uri).apply_ruleset(&repository(), &desired).await,
        Err(ProviderError::Unrepresentable {
            provider: "github",
            ..
        })
    ));
    assert_eq!(requests.await.expect("captured requests").len(), 2);
}

#[tokio::test]
async fn source_configuration_maps_write_failures_without_retrying() {
    for (response, expected) in [
        (
            ScriptedResponse::status("404 Not Found", r#"{"message":"Not Found"}"#),
            "not_found",
        ),
        (
            ScriptedResponse::status(
                "422 Unprocessable Entity",
                r#"{"message":"Validation Failed","errors":[{"resource":"RepositoryRuleset","field":"rules","code":"invalid"}]}"#,
            ),
            "invalid_input",
        ),
        (
            ScriptedResponse::status(
                "422 Unprocessable Entity",
                r#"{"message":"You have exceeded a secondary rate limit"}"#,
            ),
            "external",
        ),
        (
            ScriptedResponse::status("500 Internal Server Error", r#"{"message":"failure"}"#),
            "external",
        ),
        (ScriptedResponse::Close, "external"),
    ] {
        let (uri, requests) = scripted_responses(vec![response]).await;
        let error = provider(uri)
            .apply_ruleset(&repository(), &desired_ruleset(Some(7)))
            .await
            .expect_err("write failure");
        assert!(match expected {
            "not_found" => matches!(error, ProviderError::NotFound { .. }),
            "invalid_input" => matches!(error, ProviderError::InvalidInput { .. }),
            "external" => matches!(error, ProviderError::External { .. }),
            _ => false,
        });
        assert_eq!(requests.await.expect("captured requests").len(), 1);
    }
}

#[tokio::test]
async fn source_configuration_refuses_inherited_and_nonwritable_targets_before_transport() {
    let provider = provider("http://127.0.0.1:1".to_owned());
    let inherited: GithubRuleset =
        serde_json::from_value(detail(21, "branch", "Organization", "civitas-forge"))
            .expect("inherited ruleset");
    let repository_target: GithubRuleset = serde_json::from_value(detail(
        22,
        "repository",
        "Repository",
        "civitas-forge/interprex-sandbox",
    ))
    .expect("repository ruleset");
    for ruleset in [inherited, repository_target] {
        assert!(matches!(
            provider.apply_ruleset(&repository(), &ruleset).await,
            Err(ProviderError::Unsupported {
                provider: "github",
                ..
            })
        ));
    }
}

#[tokio::test]
async fn applied_requirements_remain_explicitly_unimplemented() {
    let provider = provider("http://127.0.0.1:1".to_owned());
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
