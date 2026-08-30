use std::time::Duration;

use interprex::{
    BranchFreshness, BranchUpdateError, BranchUpdatesProvider, ChangeRequestCommentsProvider,
    ChangeRequestHead, ChangeRequestNumber, ChangeRequestState, CheckConclusion, CheckStatus,
    CodeReviewsProvider, FindingResolution, FindingResolutionReason, FindingResolutionReply,
    FindingSeverity, Mergeability, ProviderApp, ProviderAppId, ProviderError, ProviderTextRecord,
    Repository, ReviewActor, ReviewActorId, ReviewActorKind, ReviewAnchor, ReviewDiffSide,
    ReviewDisposition, ReviewLine, ReviewPublicationKey, ReviewPublishingProvider,
    ReviewRequestTarget, ReviewRequestTargetInspection, ReviewState, ReviewSubmission,
    ReviewSubmissionDisposition, ReviewSubmissionFinding, ReviewTarget, ReviewTargetsProvider,
    ReviewTeamKind, ReviewThreadId, ReviewedRevision, ReviewerApplication,
    ReviewerApplicationsProvider, TextRecordsProvider,
};
use sha2::{Digest, Sha256};
use tokio::time::timeout;

use super::http_fixture::{
    ScriptedResponse, app_provider, assert_user_request, json_responses,
    json_responses_with_headers, project_app_provider, provider, repository, rest_filtered_pages,
    scripted_responses, server, status_json_responses,
};

const NOT_FOUND: &str = r#"{"message":"Not Found","documentation_url":"https://docs.github.test"}"#;

fn compare_status(status: &str) -> ScriptedResponse {
    ScriptedResponse::json(serde_json::json!({"status": status}).to_string())
}

#[tokio::test]
async fn branch_update_observation_reports_freshness_for_exact_commits() {
    for (status, expected_freshness) in [
        ("ahead", BranchFreshness::Current),
        ("identical", BranchFreshness::Current),
        ("behind", BranchFreshness::Behind),
        ("diverged", BranchFreshness::Behind),
    ] {
        let (uri, requests) = scripted_responses(vec![
            ScriptedResponse::json(include_str!("../fixtures/pull_request.json")),
            compare_status(status),
        ])
        .await;
        let observation = provider(uri)
            .branch_update(&repository(), ChangeRequestNumber::new(5).expect("number"))
            .await
            .expect("branch-update observation");

        assert_eq!(observation.freshness, expected_freshness);
        assert_eq!(
            observation.commit_range.head_sha,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        let requests = requests.await.expect("captured requests");
        assert_user_request(
            &requests[0],
            "GET /repos/civitas-forge/interprex-sandbox/pulls/5 ",
        );
        assert_user_request(
            &requests[1],
            "GET /repos/civitas-forge/interprex-sandbox/compare/bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb...aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa ",
        );
        assert_eq!(requests.len(), 2);
    }
}

#[tokio::test]
async fn branch_update_observation_rejects_an_unknown_comparison() {
    let (uri, _) = scripted_responses(vec![
        ScriptedResponse::json(include_str!("../fixtures/pull_request.json")),
        compare_status("sideways"),
    ])
    .await;

    assert!(matches!(
        provider(uri)
            .branch_update(&repository(), ChangeRequestNumber::new(5).expect("number"))
            .await,
        Err(ProviderError::Unrepresentable {
            provider: "github",
            ..
        })
    ));
}

#[tokio::test]
async fn native_branch_update_sends_the_exact_observed_head() {
    let (uri, requests) = scripted_responses(vec![
        ScriptedResponse::json(include_str!("../fixtures/pull_request.json")),
        ScriptedResponse::status(
            "202 Accepted",
            r#"{"message":"Updating pull request branch.","url":"https://api.github.test/pulls/5"}"#,
        ),
    ])
    .await;
    provider(uri)
        .update_change_request_branch(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .await
        .expect("native branch update");

    let requests = requests.await.expect("captured requests");
    assert_user_request(
        &requests[1],
        "PUT /repos/civitas-forge/interprex-sandbox/pulls/5/update-branch ",
    );
    let (_, body) = requests[1].split_once("\r\n\r\n").expect("request body");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(body).expect("JSON body"),
        serde_json::json!({
            "expected_head_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        })
    );
}

#[tokio::test]
async fn native_branch_update_rejects_a_head_changed_before_the_write() {
    let (uri, requests) = scripted_responses(vec![ScriptedResponse::json(include_str!(
        "../fixtures/pull_request.json"
    ))])
    .await;
    let error = provider(uri)
        .update_change_request_branch(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            "previous-head",
        )
        .await
        .expect_err("changed head");
    assert!(matches!(
        error,
        BranchUpdateError::StaleHead { expected_head_sha, observed_head_sha }
            if expected_head_sha == "previous-head"
                && observed_head_sha == "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    ));
    assert_eq!(requests.await.expect("captured requests").len(), 1);
}

#[tokio::test]
async fn native_branch_update_reports_a_racing_head_change_without_a_second_write() {
    let mut changed =
        serde_json::from_str::<serde_json::Value>(include_str!("../fixtures/pull_request.json"))
            .expect("pull request fixture");
    changed["head"]["sha"] = serde_json::json!("cccccccccccccccccccccccccccccccccccccccc");
    let (uri, requests) = scripted_responses(vec![
        ScriptedResponse::json(include_str!("../fixtures/pull_request.json")),
        ScriptedResponse::status("422 Unprocessable Entity", r#"{"message":"head changed"}"#),
        ScriptedResponse::json(changed.to_string()),
    ])
    .await;
    let error = provider(uri)
        .update_change_request_branch(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .await
        .expect_err("racing head change");
    assert!(matches!(
        error,
        BranchUpdateError::StaleHead { observed_head_sha, .. }
            if observed_head_sha == "cccccccccccccccccccccccccccccccccccccccc"
    ));
    let requests = requests.await.expect("captured requests");
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.starts_with("PUT "))
            .count(),
        1
    );
}

#[tokio::test]
async fn native_branch_update_preserves_a_provider_refusal() {
    let (uri, requests) = scripted_responses(vec![
        ScriptedResponse::json(include_str!("../fixtures/pull_request.json")),
        ScriptedResponse::status(
            "422 Unprocessable Entity",
            r#"{"message":"Branch update is not permitted"}"#,
        ),
        ScriptedResponse::json(include_str!("../fixtures/pull_request.json")),
    ])
    .await;
    assert!(matches!(
        provider(uri)
            .update_change_request_branch(
                &repository(),
                ChangeRequestNumber::new(5).expect("number"),
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )
            .await,
        Err(BranchUpdateError::Provider(ProviderError::External {
            provider: "github",
            operation: "update change request branch",
            ..
        }))
    ));
    assert_eq!(requests.await.expect("captured requests").len(), 3);
}

#[tokio::test]
async fn native_branch_update_preserves_the_write_failure_when_reread_fails() {
    let (uri, requests) = scripted_responses(vec![
        ScriptedResponse::json(include_str!("../fixtures/pull_request.json")),
        ScriptedResponse::status(
            "422 Unprocessable Entity",
            r#"{"message":"Branch update is not permitted"}"#,
        ),
        ScriptedResponse::status("503 Service Unavailable", r#"{"message":"Unavailable"}"#),
    ])
    .await;

    assert!(matches!(
        provider(uri)
            .update_change_request_branch(
                &repository(),
                ChangeRequestNumber::new(5).expect("number"),
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )
            .await,
        Err(BranchUpdateError::Provider(ProviderError::External {
            provider: "github",
            operation: "update change request branch",
            ..
        }))
    ));
    assert_eq!(requests.await.expect("captured requests").len(), 3);
}

#[tokio::test]
async fn native_branch_update_reports_not_found_when_the_failed_write_reread_is_absent() {
    let (uri, requests) = scripted_responses(vec![
        ScriptedResponse::json(include_str!("../fixtures/pull_request.json")),
        ScriptedResponse::status(
            "422 Unprocessable Entity",
            r#"{"message":"Branch update is not permitted"}"#,
        ),
        ScriptedResponse::status("404 Not Found", NOT_FOUND),
    ])
    .await;

    assert!(matches!(
        provider(uri)
            .update_change_request_branch(
                &repository(),
                ChangeRequestNumber::new(5).expect("number"),
                "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            )
            .await,
        Err(BranchUpdateError::Provider(ProviderError::NotFound { .. }))
    ));
    assert_eq!(requests.await.expect("captured requests").len(), 3);
}

fn reviewer_application() -> ReviewerApplication {
    ReviewerApplication::new(
        ProviderApp {
            id: ProviderAppId::new("4111233").expect("app id"),
            slug: "adr-codex-review".to_owned(),
            name: "ADR Codex Review".to_owned(),
        },
        ReviewActor {
            id: ReviewActorId::new("BOT_kgDOEZ_BKw").expect("bot id"),
            login: "adr-codex-review[bot]".to_owned(),
            kind: ReviewActorKind::Bot,
        },
    )
    .expect("reviewer application")
}

fn review_submission(disposition: ReviewSubmissionDisposition) -> ReviewSubmission {
    ReviewSubmission::new(
        ReviewPublicationKey::new("round-2:codex").expect("publication key"),
        ReviewedRevision {
            head_sha: "65ab9a843dd3a2114e3528fa6e0d7423fd32307e".to_owned(),
        },
        disposition,
        "Two findings need attention.",
        vec![
            ReviewSubmissionFinding::new(
                "src/lib.rs",
                ReviewLine::new(17).expect("line"),
                ReviewDiffSide::Right,
                "Handle the error.",
            )
            .expect("right-side finding"),
            ReviewSubmissionFinding::new(
                "src/old.rs",
                ReviewLine::new(4).expect("line"),
                ReviewDiffSide::Left,
                "Remove the stale branch.",
            )
            .expect("left-side finding"),
        ],
    )
    .expect("review submission")
}

fn review_submission_with_summary(summary: &str) -> ReviewSubmission {
    let template = review_submission(ReviewSubmissionDisposition::Commented);
    ReviewSubmission::new(
        template.publication_key().clone(),
        template.revision().clone(),
        template.disposition(),
        summary,
        template.findings().to_vec(),
    )
    .expect("review submission with custom summary")
}

fn publication_body(submission: &ReviewSubmission) -> String {
    let digest = Sha256::digest(serde_json::to_vec(submission).expect("serialize submission"));
    publication_body_with(
        submission.summary(),
        submission.publication_key().as_str(),
        &format!("sha256:{digest:x}"),
        submission.disposition(),
    )
}

fn publication_body_with(
    summary: &str,
    key: &str,
    digest: &str,
    disposition: ReviewSubmissionDisposition,
) -> String {
    let record = ProviderTextRecord::new(
        "interprex",
        "review-publication",
        serde_json::json!({
            "version": 1,
            "key": key,
            "digest": digest,
            "disposition": disposition,
        }),
    )
    .expect("publication record");
    provider("http://127.0.0.1:1".to_owned()).embed_record(summary, &record)
}

fn github_review(body: &str, state: &str, submitted_at: Option<&str>) -> serde_json::Value {
    // GitHub sends `performed_via_github_app: null` while a review is
    // pending; the field is populated once the review is submitted.
    let performed_via_github_app = if state == "PENDING" {
        serde_json::Value::Null
    } else {
        serde_json::json!({
            "id": 4111233,
            "slug": "adr-codex-review",
            "name": "ADR Codex Review"
        })
    };
    serde_json::json!({
        "id": 91,
        "node_id": "PRR_publication",
        "user": {
            "node_id": "BOT_kgDOEZ_BKw",
            "login": "adr-codex-review[bot]",
            "type": "Bot"
        },
        "body": body,
        "state": state,
        "commit_id": "65ab9a843dd3a2114e3528fa6e0d7423fd32307e",
        "submitted_at": submitted_at,
        "performed_via_github_app": performed_via_github_app
    })
}

fn installation_token() -> ScriptedResponse {
    ScriptedResponse::json(r#"{"token":"app-installation-token","permissions":{}}"#)
}

fn revision_response(submission: &ReviewSubmission) -> ScriptedResponse {
    ScriptedResponse::json(serde_json::json!([{"sha": submission.revision().head_sha}]).to_string())
}

#[tokio::test]
async fn publication_summary_rejects_a_complete_reserved_carrier_before_network_access() {
    let existing = review_submission(ReviewSubmissionDisposition::Commented);
    let submission = review_submission_with_summary(&publication_body(&existing));

    let error = app_provider("http://127.0.0.1:1".to_owned(), 4111233)
        .publish_review(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            &submission,
        )
        .await
        .expect_err("reserved publication carrier");

    assert!(matches!(error, ProviderError::InvalidInput { .. }));
}

#[tokio::test]
async fn publication_summary_rejects_a_malformed_reserved_prefix_before_network_access() {
    let submission = review_submission_with_summary(
        "Summary.\n\n<!-- interprex:review-publication-extra\nnot a record",
    );

    let error = app_provider("http://127.0.0.1:1".to_owned(), 4111233)
        .publish_review(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            &submission,
        )
        .await
        .expect_err("reserved publication prefix");

    assert!(matches!(error, ProviderError::InvalidInput { .. }));
}

#[tokio::test]
async fn app_publication_creates_one_complete_pending_review_then_submits_and_verifies_it() {
    let submission = review_submission(ReviewSubmissionDisposition::ChangesRequested);
    let body = publication_body(&submission);
    let pending = github_review(&body, "PENDING", None).to_string();
    let submitted =
        github_review(&body, "CHANGES_REQUESTED", Some("2026-08-29T10:00:00Z")).to_string();
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::json("[]"),
        revision_response(&submission),
        installation_token(),
        ScriptedResponse::status("200 OK", pending),
        ScriptedResponse::status("200 OK", submitted.clone()),
        ScriptedResponse::json(format!("[{submitted}]")),
    ])
    .await;

    let review_id = project_app_provider(uri, 4111233)
        .await
        .publish_review(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            &submission,
        )
        .await
        .expect("publish review");

    assert_eq!(review_id.as_str(), "PRR_publication");
    let requests = timeout(Duration::from_secs(2), requests)
        .await
        .expect("provider completed request sequence")
        .expect("captured requests");
    assert_eq!(requests.len(), 7);
    assert!(requests[0].starts_with("POST /app/installations/34/access_tokens "));
    assert!(
        requests[1].starts_with(
            "GET /repos/civitas-forge/interprex-sandbox/pulls/5/reviews?per_page=100 "
        )
    );
    assert!(
        requests[2].starts_with(
            "GET /repos/civitas-forge/interprex-sandbox/pulls/5/commits?per_page=100 "
        )
    );
    assert!(requests[3].starts_with("POST /app/installations/34/access_tokens "));
    assert!(
        requests[4].starts_with("POST /repos/civitas-forge/interprex-sandbox/pulls/5/reviews ")
    );
    assert!(
        requests[5]
            .starts_with("POST /repos/civitas-forge/interprex-sandbox/pulls/5/reviews/91/events ")
    );
    assert!(
        requests[6].starts_with(
            "GET /repos/civitas-forge/interprex-sandbox/pulls/5/reviews?per_page=100 "
        )
    );
    for request in [
        &requests[1],
        &requests[2],
        &requests[4],
        &requests[5],
        &requests[6],
    ] {
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer app-installation-token"),
            "{request}"
        );
    }

    let (_, create_body) = requests[4].split_once("\r\n\r\n").expect("create body");
    let create_body: serde_json::Value =
        serde_json::from_str(create_body).expect("JSON create body");
    assert_eq!(
        create_body,
        serde_json::json!({
            "commit_id": "65ab9a843dd3a2114e3528fa6e0d7423fd32307e",
            "body": body,
            "comments": [
                {"path": "src/lib.rs", "line": 17, "side": "RIGHT", "body": "Handle the error."},
                {"path": "src/old.rs", "line": 4, "side": "LEFT", "body": "Remove the stale branch."}
            ]
        })
    );
    assert!(create_body.get("event").is_none());
    let (_, submit_body) = requests[5].split_once("\r\n\r\n").expect("submit body");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(submit_body).expect("JSON submit body"),
        serde_json::json!({"event": "REQUEST_CHANGES"})
    );
}

#[tokio::test]
async fn app_publication_maps_approved_and_comment_only_dispositions() {
    for (disposition, github_state, event) in [
        (ReviewSubmissionDisposition::Approved, "APPROVED", "APPROVE"),
        (
            ReviewSubmissionDisposition::Commented,
            "COMMENTED",
            "COMMENT",
        ),
    ] {
        let submission = review_submission(disposition);
        let body = publication_body(&submission);
        let pending = github_review(&body, "PENDING", None).to_string();
        let submitted =
            github_review(&body, github_state, Some("2026-08-29T10:00:00Z")).to_string();
        let (uri, requests) = scripted_responses(vec![
            installation_token(),
            ScriptedResponse::json("[]"),
            revision_response(&submission),
            installation_token(),
            ScriptedResponse::json(pending),
            ScriptedResponse::json(submitted.clone()),
            ScriptedResponse::json(format!("[{submitted}]")),
        ])
        .await;

        app_provider(uri, 4111233)
            .publish_review(
                &repository(),
                ChangeRequestNumber::new(5).expect("number"),
                &reviewer_application(),
                &submission,
            )
            .await
            .expect("publish review");

        let requests = requests.await.expect("captured requests");
        let (_, submit_body) = requests[5].split_once("\r\n\r\n").expect("submit body");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(submit_body).expect("JSON submit body"),
            serde_json::json!({"event": event})
        );
    }
}

#[tokio::test]
async fn reviewer_app_id_mismatch_fails_before_any_network_request() {
    let error = app_provider("http://127.0.0.1:1".to_owned(), 999)
        .publish_review(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            &review_submission(ReviewSubmissionDisposition::Commented),
        )
        .await
        .expect_err("mismatched configured app ID");

    assert!(matches!(
        error,
        ProviderError::Configuration { ref reason, .. }
            if reason.contains("APP_ID 999") && reason.contains("4111233")
    ));
}

#[tokio::test]
async fn absent_review_revision_is_not_found_without_a_review_write() {
    let submission = review_submission(ReviewSubmissionDisposition::Commented);
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::json("[]"),
        ScriptedResponse::json(r#"[{"sha":"another-revision"}]"#),
    ])
    .await;

    let error = app_provider(uri, 4111233)
        .publish_review(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            &submission,
        )
        .await
        .expect_err("revision is absent from the change request");

    assert!(matches!(error, ProviderError::NotFound { .. }));
    let requests = requests.await.expect("captured requests");
    assert_eq!(requests.len(), 3);
    assert!(
        requests[2].starts_with(
            "GET /repos/civitas-forge/interprex-sandbox/pulls/5/commits?per_page=100 "
        )
    );
    assert!(!requests.iter().any(|request| {
        request.starts_with("POST /repos/civitas-forge/interprex-sandbox/pulls/5/reviews ")
    }));
}

#[tokio::test]
async fn capped_revision_observation_is_external_without_a_review_write() {
    let submission = review_submission(ReviewSubmissionDisposition::Commented);
    let commits = (0..250)
        .map(|index| serde_json::json!({"sha": format!("observed-{index}")}))
        .collect::<Vec<_>>();
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::json("[]"),
        ScriptedResponse::json(serde_json::to_string(&commits).expect("commit observation")),
    ])
    .await;

    let error = app_provider(uri, 4111233)
        .publish_review(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            &submission,
        )
        .await
        .expect_err("GitHub did not provide a complete revision observation");

    assert!(matches!(error, ProviderError::External { .. }));
    let requests = requests.await.expect("captured requests");
    assert!(!requests.iter().any(|request| {
        request.starts_with("POST /repos/civitas-forge/interprex-sandbox/pulls/5/reviews ")
    }));
}

#[tokio::test]
async fn submitted_recovery_reads_later_pages_and_ignores_a_foreign_reviewer() {
    let submission = review_submission(ReviewSubmissionDisposition::Commented);
    let body = publication_body(&submission);
    let mut foreign = github_review(&body, "COMMENTED", Some("2026-08-29T10:00:00Z"));
    foreign["id"] = serde_json::json!(90);
    foreign["node_id"] = serde_json::json!("PRR_foreign");
    foreign["user"]["node_id"] = serde_json::json!("BOT_foreign");
    foreign["performed_via_github_app"]["id"] = serde_json::json!(9876);
    let exact = github_review(&body, "COMMENTED", Some("2026-08-29T10:00:00Z"));
    let reviews_path = "/repos/civitas-forge/interprex-sandbox/pulls/5/reviews";
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::json(format!("[{foreign}]")).with_header(format!(
            "link: <{{base}}{reviews_path}?per_page=100&page=2>; rel=\"next\""
        )),
        ScriptedResponse::json(format!("[{exact}]")),
    ])
    .await;

    let review_id = app_provider(uri, 4111233)
        .resume_review_publication(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            submission.publication_key(),
        )
        .await
        .expect("recover review")
        .expect("matching review");

    assert_eq!(review_id.as_str(), "PRR_publication");
    let requests = requests.await.expect("captured requests");
    assert_eq!(requests.len(), 3);
    assert!(requests[2].starts_with(&format!("GET {reviews_path}?per_page=100&page=2 ")));
}

#[tokio::test]
async fn pending_recovery_on_a_later_page_submits_the_retained_disposition() {
    let submission = review_submission(ReviewSubmissionDisposition::Approved);
    let body = publication_body(&submission);
    let pending = github_review(&body, "PENDING", None);
    let submitted = github_review(&body, "APPROVED", Some("2026-08-29T10:00:00Z")).to_string();
    let reviews_path = "/repos/civitas-forge/interprex-sandbox/pulls/5/reviews";
    let next = format!("link: <{{base}}{reviews_path}?per_page=100&page=2>; rel=\"next\"");
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::json("[]").with_header(next.clone()),
        ScriptedResponse::json(format!("[{pending}]")),
        installation_token(),
        ScriptedResponse::json(submitted.clone()),
        ScriptedResponse::json("[]").with_header(next),
        ScriptedResponse::json(format!("[{submitted}]")),
    ])
    .await;

    let review_id = app_provider(uri, 4111233)
        .resume_review_publication(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            submission.publication_key(),
        )
        .await
        .expect("resume review")
        .expect("matching review");

    assert_eq!(review_id.as_str(), "PRR_publication");
    let requests = requests.await.expect("captured requests");
    let (_, submit_body) = requests[4].split_once("\r\n\r\n").expect("submit body");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(submit_body).expect("JSON submit body"),
        serde_json::json!({"event": "APPROVE"})
    );
}

#[tokio::test]
async fn same_reviewer_key_with_another_digest_is_invalid_without_a_write() {
    let submission = review_submission(ReviewSubmissionDisposition::Commented);
    let body = publication_body_with(
        submission.summary(),
        submission.publication_key().as_str(),
        &format!("sha256:{}", "0".repeat(64)),
        submission.disposition(),
    );
    let existing = github_review(&body, "COMMENTED", Some("2026-08-29T10:00:00Z"));
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::json(format!("[{existing}]")),
    ])
    .await;

    let error = app_provider(uri, 4111233)
        .publish_review(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            &submission,
        )
        .await
        .expect_err("changed submission");

    assert!(matches!(error, ProviderError::InvalidInput { .. }));
    assert_eq!(requests.await.expect("captured requests").len(), 2);
}

#[tokio::test]
async fn exact_identity_malformed_publication_record_is_external() {
    let body = "Summary.\n\n<!-- interprex:review-publication\nnot-json\n-->";
    let malformed = github_review(body, "PENDING", None);
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::json(format!("[{malformed}]")),
    ])
    .await;

    let error = app_provider(uri, 4111233)
        .resume_review_publication(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            &ReviewPublicationKey::new("round-2:codex").expect("key"),
        )
        .await
        .expect_err("malformed exact-identity record");

    assert!(matches!(error, ProviderError::External { .. }));
    assert_eq!(requests.await.expect("captured requests").len(), 2);
}

#[tokio::test]
async fn exact_identity_duplicate_publication_records_are_external() {
    let submission = review_submission(ReviewSubmissionDisposition::Commented);
    let body = publication_body(&submission);
    let carrier = &body[body.find("<!--").expect("publication carrier")..];
    let duplicate_body = format!("{body}\n\n{carrier}");
    let duplicate = github_review(&duplicate_body, "PENDING", None);
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::json(format!("[{duplicate}]")),
    ])
    .await;

    let error = app_provider(uri, 4111233)
        .resume_review_publication(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            submission.publication_key(),
        )
        .await
        .expect_err("duplicate exact-identity records");

    assert!(matches!(error, ProviderError::External { .. }));
    assert_eq!(requests.await.expect("captured requests").len(), 2);
}

#[tokio::test]
async fn exact_identity_record_that_contradicts_review_state_is_external() {
    let submission = review_submission(ReviewSubmissionDisposition::Approved);
    let body = publication_body(&submission);
    let contradictory = github_review(&body, "COMMENTED", Some("2026-08-29T10:00:00Z"));
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::json(format!("[{contradictory}]")),
    ])
    .await;

    let error = app_provider(uri, 4111233)
        .resume_review_publication(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            submission.publication_key(),
        )
        .await
        .expect_err("contradictory review state");

    assert!(matches!(error, ProviderError::External { .. }));
    assert_eq!(requests.await.expect("captured requests").len(), 2);
}

#[tokio::test]
async fn malformed_publication_record_from_another_reviewer_is_ignored() {
    let body = "Summary.\n\n<!-- interprex:review-publication\nnot-json\n-->";
    let mut foreign = github_review(body, "PENDING", None);
    foreign["user"]["node_id"] = serde_json::json!("BOT_foreign");
    foreign["performed_via_github_app"] = serde_json::json!({
        "id": 9876,
        "slug": "foreign-review",
        "name": "Foreign Review"
    });
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::json(format!("[{foreign}]")),
    ])
    .await;

    let recovered = app_provider(uri, 4111233)
        .resume_review_publication(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            &ReviewPublicationKey::new("round-2:codex").expect("key"),
        )
        .await
        .expect("foreign record is ignored");

    assert!(recovered.is_none());
    assert_eq!(requests.await.expect("captured requests").len(), 2);
}

#[tokio::test]
async fn another_key_with_the_same_digest_does_not_satisfy_recovery() {
    let submission = review_submission(ReviewSubmissionDisposition::Commented);
    let digest = Sha256::digest(serde_json::to_vec(&submission).expect("serialize submission"));
    let body = publication_body_with(
        submission.summary(),
        "another-key",
        &format!("sha256:{digest:x}"),
        submission.disposition(),
    );
    let other = github_review(&body, "COMMENTED", Some("2026-08-29T10:00:00Z"));
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::json(format!("[{other}]")),
    ])
    .await;

    let recovered = app_provider(uri, 4111233)
        .resume_review_publication(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            submission.publication_key(),
        )
        .await
        .expect("recovery read");

    assert!(recovered.is_none());
    assert_eq!(requests.await.expect("captured requests").len(), 2);
}

#[tokio::test]
async fn ambiguous_create_connection_is_reconciled_without_another_create() {
    let submission = review_submission(ReviewSubmissionDisposition::Commented);
    let body = publication_body(&submission);
    let submitted = github_review(&body, "COMMENTED", Some("2026-08-29T10:00:00Z"));
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::json("[]"),
        revision_response(&submission),
        installation_token(),
        ScriptedResponse::Close,
        ScriptedResponse::json(format!("[{submitted}]")),
    ])
    .await;

    let review_id = app_provider(uri, 4111233)
        .publish_review(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            &submission,
        )
        .await
        .expect("reconcile accepted create");

    assert_eq!(review_id.as_str(), "PRR_publication");
    let requests = requests.await.expect("captured requests");
    assert_eq!(
        requests
            .iter()
            .filter(|request| request
                .starts_with("POST /repos/civitas-forge/interprex-sandbox/pulls/5/reviews "))
            .count(),
        1
    );
}

#[tokio::test]
async fn malformed_create_response_is_reconciled_without_another_create() {
    let submission = review_submission(ReviewSubmissionDisposition::Commented);
    let body = publication_body(&submission);
    let submitted = github_review(&body, "COMMENTED", Some("2026-08-29T10:00:00Z"));
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::json("[]"),
        revision_response(&submission),
        installation_token(),
        ScriptedResponse::json("{"),
        ScriptedResponse::json(format!("[{submitted}]")),
    ])
    .await;

    let review_id = app_provider(uri, 4111233)
        .publish_review(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            &submission,
        )
        .await
        .expect("reconcile malformed create response");

    assert_eq!(review_id.as_str(), "PRR_publication");
    let requests = requests.await.expect("captured requests");
    assert_eq!(requests.len(), 6);
}

#[tokio::test]
async fn create_server_error_is_not_retried_before_reconciliation() {
    let submission = review_submission(ReviewSubmissionDisposition::Commented);
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::json("[]"),
        revision_response(&submission),
        installation_token(),
        ScriptedResponse::status("500 Internal Server Error", r#"{"message":"failed"}"#),
        ScriptedResponse::json("[]"),
    ])
    .await;

    let error = timeout(
        Duration::from_secs(2),
        app_provider(uri, 4111233).publish_review(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            &submission,
        ),
    )
    .await
    .expect("write client did not retry the create")
    .expect_err("unreconciled create failure");

    assert!(matches!(error, ProviderError::External { .. }));
    let requests = requests.await.expect("captured requests");
    assert_eq!(requests.len(), 6);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request
                .starts_with("POST /repos/civitas-forge/interprex-sandbox/pulls/5/reviews "))
            .count(),
        1
    );
}

#[tokio::test]
async fn structured_create_validation_is_invalid_input_after_reconciliation() {
    let submission = review_submission(ReviewSubmissionDisposition::Commented);
    let validation = serde_json::json!({
        "message": "Validation Failed",
        "errors": [{
            "resource": "PullRequestReviewComment",
            "field": "line",
            "code": "invalid"
        }],
        "documentation_url": "https://docs.github.test/rest/pulls/reviews"
    });
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::json("[]"),
        revision_response(&submission),
        installation_token(),
        ScriptedResponse::status("422 Unprocessable Entity", validation.to_string()),
        ScriptedResponse::json("[]"),
    ])
    .await;

    let error = app_provider(uri, 4111233)
        .publish_review(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            &submission,
        )
        .await
        .expect_err("invalid review line");

    assert!(matches!(error, ProviderError::InvalidInput { .. }));
    let requests = requests.await.expect("captured requests");
    assert_eq!(
        requests
            .iter()
            .filter(|request| request
                .starts_with("POST /repos/civitas-forge/interprex-sandbox/pulls/5/reviews "))
            .count(),
        1
    );
}

#[tokio::test]
async fn ambiguous_create_422_responses_remain_external_without_another_create() {
    let submission = review_submission(ReviewSubmissionDisposition::Commented);
    for response in [
        serde_json::json!({
            "message": "You have exceeded a secondary rate limit.",
            "documentation_url": "https://docs.github.test/rest/using-the-rest-api/rate-limits"
        }),
        serde_json::json!({
            "message": "Validation Failed",
            "errors": [{"code": "custom", "message": "ambiguous provider response"}]
        }),
    ] {
        let (uri, requests) = scripted_responses(vec![
            installation_token(),
            ScriptedResponse::json("[]"),
            revision_response(&submission),
            installation_token(),
            ScriptedResponse::status("422 Unprocessable Entity", response.to_string()),
            ScriptedResponse::json("[]"),
        ])
        .await;

        let error = app_provider(uri, 4111233)
            .publish_review(
                &repository(),
                ChangeRequestNumber::new(5).expect("number"),
                &reviewer_application(),
                &submission,
            )
            .await
            .expect_err("ambiguous 422 response");

        assert!(matches!(error, ProviderError::External { .. }));
        let requests = requests.await.expect("captured requests");
        assert_eq!(
            requests
                .iter()
                .filter(|request| request
                    .starts_with("POST /repos/civitas-forge/interprex-sandbox/pulls/5/reviews "))
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn ambiguous_submit_connection_is_reconciled_without_another_submit() {
    let submission = review_submission(ReviewSubmissionDisposition::Approved);
    let body = publication_body(&submission);
    let pending = github_review(&body, "PENDING", None).to_string();
    let submitted = github_review(&body, "APPROVED", Some("2026-08-29T10:00:00Z"));
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::json("[]"),
        revision_response(&submission),
        installation_token(),
        ScriptedResponse::json(pending),
        ScriptedResponse::Close,
        ScriptedResponse::json(format!("[{submitted}]")),
    ])
    .await;

    let review_id = app_provider(uri, 4111233)
        .publish_review(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            &submission,
        )
        .await
        .expect("reconcile accepted submit");

    assert_eq!(review_id.as_str(), "PRR_publication");
    let requests = requests.await.expect("captured requests");
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.contains("/reviews/91/events "))
            .count(),
        1
    );
}

#[tokio::test]
async fn submit_reread_rejects_a_different_same_key_review() {
    let submission = review_submission(ReviewSubmissionDisposition::Approved);
    let body = publication_body(&submission);
    let pending = github_review(&body, "PENDING", None).to_string();
    let mut replacement = github_review(&body, "APPROVED", Some("2026-08-29T10:00:00Z"));
    replacement["id"] = serde_json::json!(92);
    replacement["node_id"] = serde_json::json!("PRR_replacement");
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::json("[]"),
        revision_response(&submission),
        installation_token(),
        ScriptedResponse::json(pending),
        ScriptedResponse::json(replacement.to_string()),
        ScriptedResponse::json(format!("[{replacement}]")),
    ])
    .await;

    let error = app_provider(uri, 4111233)
        .publish_review(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            &submission,
        )
        .await
        .expect_err("reread must retain the submitted review node ID");

    assert!(matches!(error, ProviderError::External { .. }));
    let requests = requests.await.expect("captured requests");
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.contains("/reviews/91/events "))
            .count(),
        1
    );
}

#[tokio::test]
async fn submit_server_error_is_not_retried_before_reconciliation() {
    let submission = review_submission(ReviewSubmissionDisposition::Approved);
    let body = publication_body(&submission);
    let pending = github_review(&body, "PENDING", None).to_string();
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::json("[]"),
        revision_response(&submission),
        installation_token(),
        ScriptedResponse::json(pending.clone()),
        ScriptedResponse::status("500 Internal Server Error", r#"{"message":"failed"}"#),
        ScriptedResponse::json(format!("[{pending}]")),
    ])
    .await;

    let error = timeout(
        Duration::from_secs(2),
        app_provider(uri, 4111233).publish_review(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            &submission,
        ),
    )
    .await
    .expect("write client did not retry the submit")
    .expect_err("unreconciled submit failure");

    assert!(matches!(error, ProviderError::External { .. }));
    let requests = requests.await.expect("captured requests");
    assert_eq!(requests.len(), 7);
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.contains("/reviews/91/events "))
            .count(),
        1
    );
}

#[tokio::test]
async fn app_review_errors_do_not_repeat_credentials_or_authorization_data() {
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::status(
            "401 Unauthorized",
            r#"{"message":"app-installation-token Authorization: Bearer leaked-value"}"#,
        ),
    ])
    .await;

    let error = app_provider(uri, 4111233)
        .resume_review_publication(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            &ReviewPublicationKey::new("round-2:codex").expect("key"),
        )
        .await
        .expect_err("unauthorized review read");

    let diagnostic = error.to_string();
    assert!(!diagnostic.contains("app-installation-token"));
    assert!(!diagnostic.contains("leaked-value"));
    assert!(!diagnostic.to_ascii_lowercase().contains("authorization:"));
    assert_eq!(requests.await.expect("captured requests").len(), 2);
}

#[tokio::test]
async fn missing_change_request_is_reported_as_not_found() {
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::status("404 Not Found", NOT_FOUND),
    ])
    .await;

    let error = app_provider(uri, 4111233)
        .resume_review_publication(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            &ReviewPublicationKey::new("round-2:codex").expect("key"),
        )
        .await
        .expect_err("missing change request");

    assert!(matches!(
        error,
        ProviderError::NotFound { ref entity }
            if entity == "change request 5 in civitas-forge/interprex-sandbox"
    ));
    assert_eq!(requests.await.expect("captured requests").len(), 2);
}

#[tokio::test]
async fn published_review_round_trips_through_change_request_observation() {
    let submission = review_submission(ReviewSubmissionDisposition::ChangesRequested);
    let body = publication_body(&submission);
    let pending = github_review(&body, "PENDING", None).to_string();
    let submitted =
        github_review(&body, "CHANGES_REQUESTED", Some("2026-08-29T10:00:00Z")).to_string();
    let threads = serde_json::json!({
        "data": {"repository": {"pullRequest": {"reviewThreads": {
            "pageInfo": {"hasNextPage": false, "endCursor": null},
            "nodes": [
                {
                    "id": "PRRT_right",
                    "isResolved": false,
                    "isOutdated": false,
                    "path": "src/lib.rs",
                    "subjectType": "LINE",
                    "diffSide": "RIGHT",
                    "line": 17,
                    "startLine": null,
                    "originalLine": 17,
                    "originalStartLine": null,
                    "comments": {
                        "pageInfo": {"hasNextPage": false, "endCursor": null},
                        "nodes": [{
                            "id": "PRRC_right",
                            "body": "Handle the error.",
                            "createdAt": "2026-08-29T10:00:00Z",
                            "updatedAt": "2026-08-29T10:00:00Z",
                            "author": {"id": "BOT_kgDOEZ_BKw", "login": "adr-codex-review[bot]", "__typename": "Bot"},
                            "pullRequestReview": {"id": "PRR_publication"}
                        }]
                    }
                },
                {
                    "id": "PRRT_left",
                    "isResolved": false,
                    "isOutdated": false,
                    "path": "src/old.rs",
                    "subjectType": "LINE",
                    "diffSide": "LEFT",
                    "line": null,
                    "startLine": null,
                    "originalLine": 4,
                    "originalStartLine": null,
                    "comments": {
                        "pageInfo": {"hasNextPage": false, "endCursor": null},
                        "nodes": [{
                            "id": "PRRC_left",
                            "body": "Remove the stale branch.",
                            "createdAt": "2026-08-29T10:00:00Z",
                            "updatedAt": "2026-08-29T10:00:00Z",
                            "author": {"id": "BOT_kgDOEZ_BKw", "login": "adr-codex-review[bot]", "__typename": "Bot"},
                            "pullRequestReview": {"id": "PRR_publication"}
                        }]
                    }
                }
            ]
        }}}}
    })
    .to_string();
    let no_requests = serde_json::json!({
        "data": {"repository": {"pullRequest": {"reviewRequests": {
            "pageInfo": {"hasNextPage": false, "endCursor": null},
            "nodes": []
        }}}}
    })
    .to_string();
    let (uri, requests) = scripted_responses(vec![
        installation_token(),
        ScriptedResponse::json("[]"),
        revision_response(&submission),
        installation_token(),
        ScriptedResponse::json(pending),
        ScriptedResponse::json(submitted.clone()),
        ScriptedResponse::json(format!("[{submitted}]")),
        ScriptedResponse::json(include_str!("../fixtures/pull_request.json")),
        ScriptedResponse::json(format!("[{submitted}]")),
        ScriptedResponse::json(threads),
        ScriptedResponse::json(no_requests),
        ScriptedResponse::json("[]"),
    ])
    .await;
    let provider = app_provider(uri, 4111233);

    provider
        .publish_review(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &reviewer_application(),
            &submission,
        )
        .await
        .expect("publish review");
    let change_request = provider
        .change_request(&repository(), ChangeRequestNumber::new(5).expect("number"))
        .await
        .expect("observe published review");

    assert_eq!(change_request.reviews.len(), 1);
    let observed = &change_request.reviews[0];
    assert_eq!(observed.id.as_str(), "PRR_publication");
    assert_eq!(
        observed.via_app.as_ref().map(|app| app.id.as_str()),
        Some("4111233")
    );
    assert_eq!(
        observed.author.actor(&change_request.author).id.as_str(),
        "BOT_kgDOEZ_BKw"
    );
    assert_eq!(observed.revision, submission.revision().clone());
    assert!(matches!(
        observed.state,
        ReviewState::Submitted {
            disposition: ReviewDisposition::ChangesRequested,
            ..
        }
    ));
    assert_eq!(observed.findings.len(), 2);
    assert!(matches!(
        observed.findings[0].thread.location.anchor,
        ReviewAnchor::Lines {
            side: ReviewDiffSide::Right,
            ..
        }
    ));
    assert!(matches!(
        observed.findings[1].thread.location.anchor,
        ReviewAnchor::Lines {
            side: ReviewDiffSide::Left,
            ..
        }
    ));
    assert!(
        observed
            .summary
            .as_deref()
            .is_some_and(|summary| summary.starts_with(submission.summary())
                && summary.contains("<!-- interprex:review-publication"))
    );
    let requests = requests.await.expect("captured requests");
    assert_eq!(requests.len(), 12);
    for request in &requests[7..] {
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer transport-test-token"),
            "{request}"
        );
    }
}

#[tokio::test]
async fn reviewer_application_resolution_uses_the_canonical_slug_and_requests_its_bot_once() {
    let (uri, requests) = json_responses(vec![
        r#"{"id":4111233,"slug":"adr-codex-review","name":"ADR Codex Review"}"#,
        r#"{"node_id":"BOT_kgDOEZ_BKw","login":"adr-codex-review[bot]","type":"Bot"}"#,
        include_str!("../fixtures/pull_request.json"),
        r#"{"data":{"requestReviewsByLogin":{"pullRequest":{"id":"PR_kwDOExample"}}}}"#,
    ])
    .await;
    let provider = provider(uri);

    let application = provider
        .resolve_reviewer_application(&repository(), "requested alias/app")
        .await
        .expect("reviewer application");
    assert_eq!(application.app().id.as_str(), "4111233");
    assert_eq!(application.app().slug, "adr-codex-review");
    assert_eq!(application.app().name, "ADR Codex Review");
    assert_eq!(application.bot().id.as_str(), "BOT_kgDOEZ_BKw");
    assert_eq!(application.bot().login, "adr-codex-review[bot]");
    assert_eq!(application.bot().kind, ReviewActorKind::Bot);

    provider
        .request_reviewers(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            &[ReviewRequestTarget::Bot(application.bot().login.clone())],
        )
        .await
        .expect("review request");

    let requests = timeout(Duration::from_secs(1), requests)
        .await
        .expect("provider sent all requests")
        .expect("captured requests");
    assert_user_request(&requests[0], "GET /apps/requested%20alias%2Fapp ");
    assert_user_request(&requests[1], "GET /users/adr%2Dcodex%2Dreview%5Bbot%5D ");
    let (_, body) = requests[3].split_once("\r\n\r\n").expect("request body");
    let body: serde_json::Value = serde_json::from_str(body).expect("JSON request body");
    assert_eq!(
        body["variables"]["botLogins"],
        serde_json::json!(["adr-codex-review[bot]"])
    );
}

#[tokio::test]
async fn reviewer_application_resolution_distinguishes_missing_apps_and_bots() {
    let (app_uri, app_requests) = status_json_responses(vec![("404 Not Found", NOT_FOUND)]).await;
    let app_error = provider(app_uri)
        .resolve_reviewer_application(&repository(), "missing-app")
        .await
        .expect_err("missing app");
    assert!(matches!(
        app_error,
        ProviderError::NotFound { ref entity } if entity == "reviewer application missing-app"
    ));
    assert_eq!(app_requests.await.expect("captured app request").len(), 1);

    let (bot_uri, bot_requests) = status_json_responses(vec![
        (
            "200 OK",
            r#"{"id":4111233,"slug":"missing-bot","name":"Missing Bot"}"#,
        ),
        ("404 Not Found", NOT_FOUND),
    ])
    .await;
    let bot_error = provider(bot_uri)
        .resolve_reviewer_application(&repository(), "missing-bot")
        .await
        .expect_err("missing bot");
    assert!(matches!(
        bot_error,
        ProviderError::NotFound { ref entity }
            if entity == "reviewer application bot missing-bot[bot]"
    ));
    assert_eq!(bot_requests.await.expect("captured bot requests").len(), 2);
}

#[tokio::test]
async fn reviewer_application_resolution_rejects_non_bot_and_invalid_provider_data() {
    let (non_bot_uri, non_bot_requests) = status_json_responses(vec![
        (
            "200 OK",
            r#"{"id":4111233,"slug":"review-app","name":"Review App"}"#,
        ),
        (
            "200 OK",
            r#"{"node_id":"U_reviewer","login":"review-app[bot]","type":"User"}"#,
        ),
    ])
    .await;
    let non_bot_error = provider(non_bot_uri)
        .resolve_reviewer_application(&repository(), "review-app")
        .await
        .expect_err("non-bot actor");
    assert!(matches!(
        non_bot_error,
        ProviderError::Unrepresentable { provider: "github", ref fact }
            if fact.contains("must be a bot")
    ));
    assert_eq!(
        non_bot_requests
            .await
            .expect("captured non-bot requests")
            .len(),
        2
    );

    let (invalid_uri, invalid_requests) =
        status_json_responses(vec![("200 OK", r#"{"id":4111233,"slug":"review-app"}"#)]).await;
    let invalid_error = provider(invalid_uri)
        .resolve_reviewer_application(&repository(), "review-app")
        .await
        .expect_err("invalid app data");
    assert!(matches!(
        invalid_error,
        ProviderError::Unrepresentable { provider: "github", ref fact }
            if fact.contains("reviewer application review-app")
    ));
    assert_eq!(
        invalid_requests
            .await
            .expect("captured invalid request")
            .len(),
        1
    );
}

#[tokio::test]
async fn reviewer_application_resolution_reports_transport_failures_as_external() {
    let (uri, requests) = status_json_responses(vec![(
        "500 Internal Server Error",
        r#"{"message":"service unavailable","documentation_url":"https://docs.github.test"}"#,
    )])
    .await;
    let error = provider(uri)
        .resolve_reviewer_application(&repository(), "review-app")
        .await
        .expect_err("transport failure");
    assert!(matches!(
        error,
        ProviderError::External {
            provider: "github",
            operation: "resolve reviewer application",
            ..
        }
    ));
    assert_eq!(requests.await.expect("captured failed request").len(), 1);
}

#[tokio::test]
async fn change_request_comment_creation_posts_the_exact_body_and_returns_its_node_id() {
    let (uri, request) = server(
        "201 Created",
        "application/json",
        r#"{"node_id":"IC_created-comment"}"#,
    )
    .await;
    let body = "Comitia planned round 3.\n\n<!-- comitia:loop-event\n{\"version\":1}\n-->";

    let id = provider(uri)
        .create_unanchored_comment(
            &repository(),
            ChangeRequestNumber::new(5).expect("number"),
            body,
        )
        .await
        .expect("create comment");

    assert_eq!(id.as_str(), "IC_created-comment");
    let request = request.await.expect("captured request");
    assert_user_request(
        &request,
        "POST /repos/civitas-forge/interprex-sandbox/issues/5/comments ",
    );
    let (_, request_body) = request.split_once("\r\n\r\n").expect("request body");
    let request_body: serde_json::Value =
        serde_json::from_str(request_body).expect("JSON request body");
    assert_eq!(request_body, serde_json::json!({ "body": body }));
}

#[tokio::test]
async fn review_target_inspection_finds_a_bot_after_the_user_spelling_is_missing() {
    let (uri, requests) = status_json_responses(vec![
        ("404 Not Found", NOT_FOUND),
        (
            "200 OK",
            r#"{"node_id":"BOT_review","login":"review-bot[bot]","type":"Bot"}"#,
        ),
    ])
    .await;

    let inspection = provider(uri)
        .inspect_review_request_target(
            &repository(),
            &ReviewRequestTarget::User("review-bot".to_owned()),
        )
        .await
        .expect("target inspection");

    assert!(matches!(
        inspection,
        ReviewRequestTargetInspection::KindMismatch(ReviewTarget::Actor(actor))
            if actor.login == "review-bot[bot]" && actor.kind == ReviewActorKind::Bot
    ));
    let requests = requests.await.expect("captured requests");
    assert_user_request(&requests[0], "GET /users/review%2Dbot ");
    assert_user_request(&requests[1], "GET /users/review%2Dbot%5Bbot%5D ");
}

#[tokio::test]
async fn user_inspection_stops_when_the_exact_spelling_exists() {
    let (uri, requests) = status_json_responses(vec![(
        "200 OK",
        r#"{"node_id":"U_review","login":"review-bot","type":"User"}"#,
    )])
    .await;

    let inspection = provider(uri)
        .inspect_review_request_target(
            &repository(),
            &ReviewRequestTarget::User("review-bot".to_owned()),
        )
        .await
        .expect("target inspection");

    assert!(matches!(
        inspection,
        ReviewRequestTargetInspection::Matching(ReviewTarget::Actor(actor))
            if actor.login == "review-bot" && actor.kind == ReviewActorKind::User
    ));
    let requests = requests.await.expect("captured requests");
    assert_eq!(requests.len(), 1);
    assert_user_request(&requests[0], "GET /users/review%2Dbot ");
}

#[tokio::test]
async fn bot_inspection_prefers_the_canonical_suffix() {
    let (uri, requests) = status_json_responses(vec![(
        "200 OK",
        r#"{"node_id":"BOT_review","login":"review-bot[bot]","type":"Bot"}"#,
    )])
    .await;

    let inspection = provider(uri)
        .inspect_review_request_target(
            &repository(),
            &ReviewRequestTarget::Bot("review-bot".to_owned()),
        )
        .await
        .expect("target inspection");

    assert!(matches!(
        inspection,
        ReviewRequestTargetInspection::Matching(ReviewTarget::Actor(actor))
            if actor.kind == ReviewActorKind::Bot
    ));
    let requests = requests.await.expect("captured requests");
    assert_eq!(requests.len(), 1);
    assert_user_request(&requests[0], "GET /users/review%2Dbot%5Bbot%5D ");
}

#[tokio::test]
async fn review_target_inspection_distinguishes_unresolvable_from_operational_failure() {
    let (missing_uri, missing_requests) = status_json_responses(vec![
        ("404 Not Found", NOT_FOUND),
        ("404 Not Found", NOT_FOUND),
    ])
    .await;
    assert_eq!(
        provider(missing_uri)
            .inspect_review_request_target(
                &repository(),
                &ReviewRequestTarget::User("unknown".to_owned()),
            )
            .await
            .expect("missing is an inspection outcome"),
        ReviewRequestTargetInspection::NotResolvable
    );
    assert_eq!(missing_requests.await.expect("captured requests").len(), 2);

    let (failure_uri, failure_requests) = status_json_responses(vec![(
        "500 Internal Server Error",
        r#"{"message":"service unavailable","documentation_url":"https://docs.github.test"}"#,
    )])
    .await;
    let error = provider(failure_uri)
        .inspect_review_request_target(
            &repository(),
            &ReviewRequestTarget::User("unknown".to_owned()),
        )
        .await
        .expect_err("operational failure remains an error");
    assert!(matches!(
        error,
        ProviderError::External {
            provider: "github",
            operation: "inspect review request target",
            ..
        }
    ));
    assert_eq!(
        failure_requests.await.expect("captured requests").len(),
        1,
        "a non-404 failure must stop fallback lookup"
    );
}

#[tokio::test]
async fn review_target_inspection_normalizes_organization_teams() {
    let (uri, requests) = status_json_responses(vec![(
        "200 OK",
        r#"{"node_id":"T_maintainers","slug":"maintainers","name":"Maintainers"}"#,
    )])
    .await;
    let inspection = provider(uri)
        .inspect_review_request_target(
            &repository(),
            &ReviewRequestTarget::Team("civitas-forge/maintainers".to_owned()),
        )
        .await
        .expect("team inspection");
    assert!(matches!(
        inspection,
        ReviewRequestTargetInspection::Matching(ReviewTarget::Team(team))
            if team.id.as_str() == "T_maintainers"
                && team.slug == "maintainers"
                && team.name == "Maintainers"
                && team.kind == ReviewTeamKind::Organization
    ));
    let requests = requests.await.expect("captured requests");
    assert_user_request(&requests[0], "GET /orgs/civitas%2Dforge/teams/maintainers ");
}

#[tokio::test]
async fn review_target_inspection_rejects_a_malformed_team_before_transport() {
    let error = provider("http://127.0.0.1:1".to_owned())
        .inspect_review_request_target(
            &repository(),
            &ReviewRequestTarget::Team("maintainers".to_owned()),
        )
        .await
        .expect_err("malformed team address");
    assert!(matches!(
        error,
        ProviderError::InvalidInput { provider: "github", ref fact }
            if fact.contains("organization/team-slug")
    ));
}

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
async fn change_request_comments_keep_github_order_across_rest_pages() {
    let mut review_requests: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/review_requests_response.json"))
            .expect("review request fixture");
    review_requests["data"]["repository"]["pullRequest"]["reviewRequests"]["nodes"] =
        serde_json::json!([]);
    let comment = |id: u64, node_id: &str| {
        serde_json::json!({
            "id": id,
            "node_id": node_id,
            "user": {
                "node_id": "U_comment_author",
                "login": "comment-author",
                "type": "User"
            },
            "body": format!("Comment {id}"),
            "created_at": "2026-08-29T10:00:00Z",
            "updated_at": "2026-08-29T10:00:00Z"
        })
    };
    let first_page = serde_json::json!([
        comment(20, "A_lexically_first"),
        comment(30, "M_lexically_middle")
    ])
    .to_string();
    let second_page = serde_json::json!([comment(10, "Z_lexically_last")]).to_string();
    let (uri, requests) = json_responses_with_headers(vec![
        (include_str!("../fixtures/pull_request.json").to_owned(), ""),
        (
            include_str!("../fixtures/code_review_reviews.json").to_owned(),
            "",
        ),
        (
            include_str!("../fixtures/review_threads_response.json").to_owned(),
            "",
        ),
        (
            serde_json::to_string(&review_requests).expect("review request response"),
            "",
        ),
        (
            first_page,
            "link: <{base}/repos/civitas-forge/interprex-sandbox/issues/5/comments?per_page=100&page=2>; rel=\"next\"\r\n",
        ),
        (second_page, ""),
    ])
    .await;

    let change_request = provider(uri)
        .change_request(&repository(), ChangeRequestNumber::new(5).expect("number"))
        .await
        .expect("change request");

    assert_eq!(
        change_request
            .unanchored_comments
            .iter()
            .map(|comment| comment.id.as_str())
            .collect::<Vec<_>>(),
        [
            "Z_lexically_last",
            "A_lexically_first",
            "M_lexically_middle"
        ]
    );
    let requests = requests.await.expect("captured requests");
    assert_user_request(
        &requests[5],
        "GET /repos/civitas-forge/interprex-sandbox/issues/5/comments?per_page=100&page=2 ",
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
