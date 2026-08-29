use interprex::{
    BranchFreshness, BranchUpdateError, BranchUpdateObservation, BranchUpdateRequirement,
    CommitRange, ProviderError,
};

#[test]
fn branch_update_observation_keeps_rule_freshness_and_revisions_distinct() {
    let observation = BranchUpdateObservation {
        commit_range: CommitRange {
            base_sha: "base-2".to_owned(),
            head_sha: "head-3".to_owned(),
        },
        requirement: BranchUpdateRequirement::Required,
        freshness: BranchFreshness::Behind,
    };

    assert!(observation.update_required());
    assert_eq!(observation.commit_range.head_sha, "head-3");
    assert_eq!(
        serde_json::to_value(&observation).expect("serialize observation"),
        serde_json::json!({
            "commit_range": { "base_sha": "base-2", "head_sha": "head-3" },
            "requirement": "required",
            "freshness": "behind"
        })
    );
}

#[test]
fn current_or_optional_branches_do_not_require_an_update() {
    for (requirement, freshness) in [
        (
            BranchUpdateRequirement::NotRequired,
            BranchFreshness::Behind,
        ),
        (BranchUpdateRequirement::Required, BranchFreshness::Current),
    ] {
        let observation = BranchUpdateObservation {
            commit_range: CommitRange {
                base_sha: "base".to_owned(),
                head_sha: "head".to_owned(),
            },
            requirement,
            freshness,
        };
        assert!(!observation.update_required());
    }
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
