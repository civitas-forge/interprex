use std::collections::BTreeMap;

use interprex::{DispatchInputs, JobsProvider};

use super::http_fixture::{assert_user_request, provider, repository, server};

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
        "POST /repos/civitas-forge/interprex-sandbox/actions/workflows/quality.yml/dispatches ",
    );
    assert!(request.contains("\"ref\":\"main\""));
    assert!(request.contains("\"tier\":\"pull-request\""));
}
