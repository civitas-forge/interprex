use interprex::{
    AppliedRequiredCheckState, AppliedSourceRequirementsProvider, BranchUpdateRequirement,
    CodeHostingProvider, CommitRange, ProviderError, RepositorySettings,
    SourceCodeConfigurationProvider,
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

const BASE_SHA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const HEAD_SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn branch(name: &str, sha: &str) -> String {
    serde_json::json!({"name": name, "commit": {"sha": sha}}).to_string()
}

fn pull_request_parameters(approvals: u32) -> serde_json::Value {
    serde_json::json!({
        "allowed_merge_methods": ["merge", "squash", "rebase"],
        "dismiss_stale_reviews_on_push": false,
        "dismissal_restriction": {"enabled": false, "allowed_actors": []},
        "require_code_owner_review": false,
        "require_last_push_approval": false,
        "required_approving_review_count": approvals,
        "required_review_thread_resolution": false,
        "required_reviewers": []
    })
}

fn check_run(
    name: &str,
    status: &str,
    conclusion: Option<&str>,
    app_id: Option<u64>,
) -> serde_json::Value {
    let suite_id = app_id.unwrap_or(1);
    let run_id = name.bytes().fold(suite_id * 10_000 + 1, |value, byte| {
        value.wrapping_mul(16_777_619).wrapping_add(u64::from(byte))
    });
    serde_json::json!({
        "id": run_id,
        "name": name,
        "head_sha": HEAD_SHA,
        "status": status,
        "conclusion": conclusion,
        "completed_at": conclusion.map(|_| "2026-08-29T12:00:00Z"),
        "app": app_id.map(|id| serde_json::json!({
            "id": id,
            "slug": format!("app-{id}"),
            "name": format!("App {id}")
        })),
        "check_suite": {"id": suite_id}
    })
}

fn check_suites(ids: &[u64], total: usize) -> String {
    let suites = ids
        .iter()
        .map(|id| serde_json::json!({"id": id, "head_sha": HEAD_SHA}))
        .collect::<Vec<_>>();
    serde_json::json!({"total_count": total, "check_suites": suites}).to_string()
}

fn check_runs(runs: Vec<serde_json::Value>, total: usize) -> String {
    serde_json::json!({"total_count": total, "check_runs": runs}).to_string()
}

fn combined_statuses(statuses: Vec<(&str, &str)>, total: usize) -> String {
    let statuses = statuses
        .into_iter()
        .map(|(context, state)| serde_json::json!({"context": context, "state": state}))
        .collect::<Vec<_>>();
    serde_json::json!({"sha": HEAD_SHA, "total_count": total, "statuses": statuses}).to_string()
}

fn no_classic_protection() -> ScriptedResponse {
    ScriptedResponse::status("404 Not Found", r#"{"message":"Branch not protected"}"#)
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
async fn applied_requirements_combine_native_policy_and_exact_head_answers() {
    let rules = serde_json::json!([{
        "type": "pull_request",
        "parameters": pull_request_parameters(2),
        "ruleset_source_type": "Repository",
        "ruleset_source": "civitas-forge/interprex-sandbox"
    }, {
        "type": "pull_request",
        "parameters": pull_request_parameters(3),
        "ruleset_source_type": "Organization",
        "ruleset_source": "civitas-forge"
    }, {
        "type": "required_status_checks",
        "parameters": {
            "strict_required_status_checks_policy": true,
            "required_status_checks": [
                {"context": "Quality", "integration_id": 42},
                {"context": "legacy", "integration_id": null},
                {"context": "dual", "integration_id": null},
                {"context": "neutral", "integration_id": null},
                {"context": "skipped", "integration_id": null},
                {"context": "pending", "integration_id": null},
                {"context": "failed", "integration_id": null},
                {"context": "missing", "integration_id": null},
                {"context": "app-bound", "integration_id": 99}
            ]
        },
        "ruleset_source_type": "Organization",
        "ruleset_source": "civitas-forge"
    }]);
    let classic = serde_json::json!({
        "required_status_checks": {
            "strict": false,
            "contexts": ["classic", "quality"],
            "checks": [{"context": "Quality", "app_id": 42}]
        },
        "required_pull_request_reviews": {"required_approving_review_count": 4}
    });
    let app_42_runs = vec![check_run("quality", "completed", Some("success"), Some(42))];
    let unowned_runs = vec![
        check_run("dual", "completed", Some("success"), Some(7)),
        check_run("neutral", "completed", Some("neutral"), Some(7)),
        check_run("skipped", "completed", Some("skipped"), Some(7)),
        check_run("pending", "in_progress", None, Some(7)),
        check_run("failed", "completed", Some("failure"), Some(7)),
    ];
    let app_98_runs = vec![check_run(
        "app-bound",
        "completed",
        Some("success"),
        Some(98),
    )];
    let statuses = vec![
        ("legacy", "success"),
        ("dual", "failure"),
        ("classic", "success"),
        ("app-bound", "success"),
    ];
    let status_count = statuses.len();
    let (uri, requests) = scripted_responses(vec![
        ScriptedResponse::json(branch("release/v1", BASE_SHA)),
        ScriptedResponse::json(rules.to_string()),
        ScriptedResponse::json(classic.to_string()),
        ScriptedResponse::json(check_suites(&[42, 7, 98], 3)),
        ScriptedResponse::json(check_runs(app_42_runs, 1)),
        ScriptedResponse::json(check_runs(unowned_runs, 5)),
        ScriptedResponse::json(check_runs(app_98_runs, 1)),
        ScriptedResponse::json(combined_statuses(statuses, status_count)),
        ScriptedResponse::json(branch("release/v1", BASE_SHA)),
    ])
    .await;
    let range = CommitRange {
        base_sha: BASE_SHA.to_owned(),
        head_sha: HEAD_SHA.to_owned(),
    };

    let observed = provider(uri)
        .applied_requirements(&repository(), "release/v1", &range)
        .await
        .expect("applied requirements");

    assert_eq!(observed.repository(), &repository());
    assert_eq!(observed.target_branch(), "release/v1");
    assert_eq!(observed.commit_range(), &range);
    assert_eq!(observed.required_approvals(), 4);
    assert_eq!(observed.branch_update(), BranchUpdateRequirement::Required);
    let answers = observed
        .required_checks()
        .iter()
        .map(|check| {
            (
                check.name(),
                check.provider_application().map(|app| app.as_str()),
                check.state(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        answers,
        vec![
            ("app-bound", Some("99"), AppliedRequiredCheckState::Missing),
            ("classic", None, AppliedRequiredCheckState::Satisfied),
            ("dual", None, AppliedRequiredCheckState::Failed),
            ("failed", None, AppliedRequiredCheckState::Failed),
            ("legacy", None, AppliedRequiredCheckState::Satisfied),
            ("missing", None, AppliedRequiredCheckState::Missing),
            ("neutral", None, AppliedRequiredCheckState::Satisfied),
            ("pending", None, AppliedRequiredCheckState::Pending),
            ("Quality", Some("42"), AppliedRequiredCheckState::Satisfied),
            ("skipped", None, AppliedRequiredCheckState::Satisfied),
        ]
    );

    let requests = requests.await.expect("captured requests");
    assert_eq!(requests.len(), 9);
    assert_user_request(
        &requests[0],
        "GET /repos/civitas-forge/interprex-sandbox/branches/release%2Fv1 ",
    );
    assert_user_request(
        &requests[1],
        "GET /repos/civitas-forge/interprex-sandbox/rules/branches/release%2Fv1?per_page=100 ",
    );
    assert_user_request(
        &requests[2],
        "GET /repos/civitas-forge/interprex-sandbox/branches/release%2Fv1/protection ",
    );
    assert_user_request(
        &requests[3],
        &format!(
            "GET /repos/civitas-forge/interprex-sandbox/commits/{HEAD_SHA}/check-suites?per_page=100&page=1 "
        ),
    );
    assert_user_request(
        &requests[4],
        "GET /repos/civitas-forge/interprex-sandbox/check-suites/42/check-runs?per_page=100&page=1&filter=latest ",
    );
    assert_user_request(
        &requests[5],
        "GET /repos/civitas-forge/interprex-sandbox/check-suites/7/check-runs?per_page=100&page=1&filter=latest ",
    );
    assert_user_request(
        &requests[6],
        "GET /repos/civitas-forge/interprex-sandbox/check-suites/98/check-runs?per_page=100&page=1&filter=latest ",
    );
    assert_user_request(
        &requests[7],
        &format!(
            "GET /repos/civitas-forge/interprex-sandbox/commits/{HEAD_SHA}/status?per_page=100&page=1 "
        ),
    );
}

#[tokio::test]
async fn applied_branch_endpoint_is_the_authority_for_excluded_rulesets() {
    let (uri, requests) = scripted_responses(vec![
        ScriptedResponse::json(branch("main", BASE_SHA)),
        // GitHub has already evaluated repository and inherited ruleset
        // conditions. An include-all ruleset excluded for `main` is absent.
        ScriptedResponse::json("[]"),
        no_classic_protection(),
        ScriptedResponse::json(check_suites(&[], 0)),
        ScriptedResponse::json(combined_statuses(Vec::new(), 0)),
        ScriptedResponse::json(branch("main", BASE_SHA)),
    ])
    .await;

    let observed = provider(uri)
        .applied_requirements(
            &repository(),
            "main",
            &CommitRange {
                base_sha: BASE_SHA.to_owned(),
                head_sha: HEAD_SHA.to_owned(),
            },
        )
        .await
        .expect("empty applied policy");

    assert_eq!(observed.required_approvals(), 0);
    assert_eq!(
        observed.branch_update(),
        BranchUpdateRequirement::NotRequired
    );
    assert!(observed.required_checks().is_empty());
    assert_eq!(requests.await.expect("captured requests").len(), 6);
}

#[tokio::test]
async fn applied_requirements_refuse_non_scalar_classic_review_constraints() {
    let classic = serde_json::json!({
        "required_pull_request_reviews": {
            "url": "https://api.github.test/repos/civitas-forge/interprex-sandbox/branches/main/protection/required_pull_request_reviews",
            "dismiss_stale_reviews": false,
            "require_code_owner_reviews": true,
            "required_approving_review_count": 2,
            "require_last_push_approval": false,
            "dismissal_restrictions": {"users": [], "teams": [], "apps": []}
        },
        "required_conversation_resolution": {"enabled": false}
    });
    let (uri, requests) = scripted_responses(vec![
        ScriptedResponse::json(branch("main", BASE_SHA)),
        ScriptedResponse::json("[]"),
        ScriptedResponse::json(classic.to_string()),
    ])
    .await;

    let error = provider(uri)
        .applied_requirements(
            &repository(),
            "main",
            &CommitRange {
                base_sha: BASE_SHA.to_owned(),
                head_sha: HEAD_SHA.to_owned(),
            },
        )
        .await
        .expect_err("code-owner identity is not a scalar approval count");

    assert!(matches!(
        error,
        ProviderError::Unrepresentable { fact, .. } if fact.contains("code-owner")
    ));
    assert_eq!(requests.await.expect("captured requests").len(), 3);
}

#[tokio::test]
async fn applied_requirements_paginate_rules_check_runs_and_statuses() {
    let first_rules = (0..100)
        .map(|_| serde_json::json!({"type": "creation"}))
        .collect::<Vec<_>>();
    let second_rules = serde_json::json!([{
        "type": "required_status_checks",
        "parameters": {
            "strict_required_status_checks_policy": false,
            "required_status_checks": [{"context": "last", "integration_id": null}]
        }
    }]);
    let first_runs = (0..100)
        .map(|index| check_run(&format!("run-{index}"), "completed", Some("success"), None))
        .collect::<Vec<_>>();
    let first_statuses = (0..100)
        .map(|index| (format!("status-{index}"), "success".to_owned()))
        .collect::<Vec<_>>();
    let first_statuses_json = serde_json::json!({
        "sha": HEAD_SHA,
        "total_count": 101,
        "statuses": first_statuses
            .iter()
            .map(|(context, state)| serde_json::json!({"context": context, "state": state}))
            .collect::<Vec<_>>()
    });
    let (uri, requests) = scripted_responses(vec![
        ScriptedResponse::json(branch("main", BASE_SHA)),
        ScriptedResponse::json(serde_json::to_string(&first_rules).expect("rules")).with_header(
            "link: <{base}/repos/civitas-forge/interprex-sandbox/rules/branches/main?per_page=100&page=2>; rel=\"next\"",
        ),
        ScriptedResponse::json(second_rules.to_string()),
        no_classic_protection(),
        ScriptedResponse::json(check_suites(&[1], 1)),
        ScriptedResponse::json(check_runs(first_runs, 101)),
        ScriptedResponse::json(check_runs(
            vec![check_run("last", "completed", Some("success"), None)],
            101,
        )),
        ScriptedResponse::json(first_statuses_json.to_string()),
        ScriptedResponse::json(combined_statuses(vec![("last", "success")], 101)),
        ScriptedResponse::json(branch("main", BASE_SHA)),
    ])
    .await;

    let observed = provider(uri)
        .applied_requirements(
            &repository(),
            "main",
            &CommitRange {
                base_sha: BASE_SHA.to_owned(),
                head_sha: HEAD_SHA.to_owned(),
            },
        )
        .await
        .expect("paginated applied requirements");
    assert_eq!(observed.required_checks().len(), 1);
    assert_eq!(
        observed.required_checks()[0].state(),
        AppliedRequiredCheckState::Satisfied
    );
    let requests = requests.await.expect("captured requests");
    assert_eq!(requests.len(), 10);
    assert!(requests[2].starts_with(
        "GET /repos/civitas-forge/interprex-sandbox/rules/branches/main?per_page=100&page=2 "
    ));
    assert!(requests[6].contains("page=2&filter=latest"));
    assert!(requests[8].contains("/status?per_page=100&page=2"));
}

#[tokio::test]
async fn applied_requirements_paginate_every_check_suite_without_a_count_cutoff() {
    let first_suite_ids = (1..=100).collect::<Vec<_>>();
    let mut responses = vec![
        ScriptedResponse::json(branch("main", BASE_SHA)),
        ScriptedResponse::json("[]"),
        no_classic_protection(),
        ScriptedResponse::json(check_suites(&first_suite_ids, 101)),
        ScriptedResponse::json(check_suites(&[101], 101)),
    ];
    for _ in 1..=101 {
        responses.push(ScriptedResponse::json(check_runs(Vec::new(), 0)));
    }
    responses.push(ScriptedResponse::json(combined_statuses(Vec::new(), 0)));
    responses.push(ScriptedResponse::json(branch("main", BASE_SHA)));
    let (uri, requests) = scripted_responses(responses).await;

    let observed = provider(uri)
        .applied_requirements(
            &repository(),
            "main",
            &CommitRange {
                base_sha: BASE_SHA.to_owned(),
                head_sha: HEAD_SHA.to_owned(),
            },
        )
        .await
        .expect("all suites were enumerated");

    assert!(observed.required_checks().is_empty());
    let requests = requests.await.expect("captured requests");
    assert_eq!(requests.len(), 108);
    assert_user_request(
        &requests[4],
        &format!(
            "GET /repos/civitas-forge/interprex-sandbox/commits/{HEAD_SHA}/check-suites?per_page=100&page=2 "
        ),
    );
    assert!(requests.iter().any(|request| request.starts_with(
        "GET /repos/civitas-forge/interprex-sandbox/check-suites/101/check-runs?per_page=100&page=1&filter=latest "
    )));
}

#[tokio::test]
async fn applied_requirements_never_turn_read_or_shape_errors_into_empty_policy() {
    let cases = vec![
        (
            vec![
                ScriptedResponse::json(branch("main", BASE_SHA)),
                ScriptedResponse::status("403 Forbidden", r#"{"message":"forbidden"}"#),
            ],
            "external",
        ),
        (
            vec![
                ScriptedResponse::json(branch("main", BASE_SHA)),
                ScriptedResponse::json(r#"[{"type":"pull_request","parameters":{}}]"#),
                no_classic_protection(),
            ],
            "unrepresentable",
        ),
        (
            vec![
                ScriptedResponse::json(branch("main", BASE_SHA)),
                ScriptedResponse::json(
                    serde_json::json!([{
                        "type": "pull_request",
                        "parameters": {
                            "allowed_merge_methods": ["squash", "merge", "rebase"],
                            "dismiss_stale_reviews_on_push": false,
                            "dismissal_restriction": {"enabled": false, "allowed_actors": []},
                            "require_code_owner_review": false,
                            "require_last_push_approval": false,
                            "required_approving_review_count": 1,
                            "required_review_thread_resolution": false,
                            "required_reviewers": [{
                                "file_patterns": ["src/**"],
                                "minimum_approvals": 1,
                                "reviewer": {"id": 9, "type": "Team"}
                            }]
                        }
                    }])
                    .to_string(),
                ),
                no_classic_protection(),
            ],
            "unrepresentable",
        ),
        (
            vec![
                ScriptedResponse::json(branch("main", BASE_SHA)),
                ScriptedResponse::json("[]"),
                ScriptedResponse::status("403 Forbidden", r#"{"message":"forbidden"}"#),
            ],
            "external",
        ),
        (
            vec![
                ScriptedResponse::json(branch("main", BASE_SHA)),
                ScriptedResponse::json("[]"),
                ScriptedResponse::status("404 Not Found", r#"{"message":"Not Found"}"#),
            ],
            "external",
        ),
        (
            vec![
                ScriptedResponse::json(branch("main", BASE_SHA)),
                ScriptedResponse::json("[]"),
                no_classic_protection(),
                ScriptedResponse::status("404 Not Found", r#"{"message":"missing"}"#),
            ],
            "not_found",
        ),
        (
            vec![
                ScriptedResponse::json(branch("main", BASE_SHA)),
                ScriptedResponse::json("[]"),
                no_classic_protection(),
                ScriptedResponse::json(check_suites(&[1], 1)),
                ScriptedResponse::json(
                    serde_json::json!({
                        "total_count": 1,
                        "check_runs": [{
                            "id": 11,
                            "name": "quality",
                            "head_sha": "cccccccccccccccccccccccccccccccccccccccc",
                            "status": "queued",
                            "conclusion": null,
                            "completed_at": null,
                            "app": null,
                            "check_suite": {"id": 1}
                        }]
                    })
                    .to_string(),
                ),
            ],
            "unrepresentable",
        ),
        (
            vec![
                ScriptedResponse::json(branch("main", BASE_SHA)),
                ScriptedResponse::json("[]"),
                no_classic_protection(),
                ScriptedResponse::json(check_suites(&[], 0)),
                ScriptedResponse::json(
                    serde_json::json!({
                        "sha": "cccccccccccccccccccccccccccccccccccccccc",
                        "total_count": 0,
                        "statuses": []
                    })
                    .to_string(),
                ),
            ],
            "unrepresentable",
        ),
    ];
    for (responses, expected) in cases {
        let (uri, requests) = scripted_responses(responses).await;
        let error = provider(uri)
            .applied_requirements(
                &repository(),
                "main",
                &CommitRange {
                    base_sha: BASE_SHA.to_owned(),
                    head_sha: HEAD_SHA.to_owned(),
                },
            )
            .await
            .expect_err("provider error");
        assert!(match expected {
            "external" => matches!(error, ProviderError::External { .. }),
            "unrepresentable" => matches!(error, ProviderError::Unrepresentable { .. }),
            "not_found" => matches!(error, ProviderError::NotFound { .. }),
            _ => false,
        });
        assert!(!requests.await.expect("captured requests").is_empty());
    }
}

#[tokio::test]
async fn applied_requirements_reject_invalid_or_changed_exact_scope() {
    let offline = provider("http://127.0.0.1:1".to_owned());
    for range in [
        CommitRange {
            base_sha: "short".to_owned(),
            head_sha: HEAD_SHA.to_owned(),
        },
        CommitRange {
            base_sha: BASE_SHA.to_owned(),
            head_sha: String::new(),
        },
    ] {
        assert!(matches!(
            offline
                .applied_requirements(&repository(), "main", &range)
                .await,
            Err(ProviderError::InvalidInput { .. })
        ));
    }

    let (uri, requests) = scripted_responses(vec![
        ScriptedResponse::json(branch("main", BASE_SHA)),
        ScriptedResponse::json("[]"),
        no_classic_protection(),
        ScriptedResponse::json(check_suites(&[], 0)),
        ScriptedResponse::json(combined_statuses(Vec::new(), 0)),
        ScriptedResponse::json(branch("main", "cccccccccccccccccccccccccccccccccccccccc")),
    ])
    .await;
    assert!(matches!(
        provider(uri)
            .applied_requirements(
                &repository(),
                "main",
                &CommitRange {
                    base_sha: BASE_SHA.to_owned(),
                    head_sha: HEAD_SHA.to_owned(),
                },
            )
            .await,
        Err(ProviderError::NotFound { .. })
    ));
    assert_eq!(requests.await.expect("captured requests").len(), 6);
}
