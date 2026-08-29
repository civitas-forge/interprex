use interprex::{
    BranchFreshness, BranchUpdateError, BranchUpdateObservation, CommitRange, ProviderError,
};

#[test]
fn branch_update_observation_keeps_freshness_and_revisions_together() {
    let observation = BranchUpdateObservation {
        commit_range: CommitRange {
            base_sha: "base-2".to_owned(),
            head_sha: "head-3".to_owned(),
        },
        freshness: BranchFreshness::Behind,
    };

    assert_eq!(observation.commit_range.head_sha, "head-3");
    assert_eq!(
        serde_json::to_value(&observation).expect("serialize observation"),
        serde_json::json!({
            "commit_range": { "base_sha": "base-2", "head_sha": "head-3" },
            "freshness": "behind"
        })
    );
}

#[test]
fn branch_update_error_retains_provider_error_type_and_detail() {
    let error = BranchUpdateError::from(ProviderError::External {
        provider: "example",
        operation: "update branch",
        message: "provider refused the update".to_owned(),
    });

    assert!(matches!(
        &error,
        BranchUpdateError::Provider(ProviderError::External {
            provider: "example",
            operation: "update branch",
            ..
        })
    ));
    assert_eq!(
        error.to_string(),
        "example update branch failed: provider refused the update"
    );
}
