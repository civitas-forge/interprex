use async_trait::async_trait;
use interprex::{
    AppliedRequiredCheck, AppliedRequiredCheckState, AppliedSourceRequirements,
    AppliedSourceRequirementsProvider, BranchUpdateRequirement, CommitRange, ModelError,
    ProviderAppId, ProviderError, Repository, Result, SourceCodeConfigurationProvider,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::fmt::Debug;

fn repository() -> Repository {
    Repository::new("civitas-forge", "interprex").expect("repository")
}

fn required_check(name: &str, app: Option<&str>) -> AppliedRequiredCheck {
    AppliedRequiredCheck::new(
        name,
        app.map(|id| ProviderAppId::new(id).expect("application id")),
        AppliedRequiredCheckState::Satisfied,
    )
    .expect("required check")
}

#[test]
fn applied_requirements_round_trip_the_exact_subject_and_provider_answers() {
    let requirements = AppliedSourceRequirements::new(
        repository(),
        "main",
        CommitRange {
            base_sha: "base-2".to_owned(),
            head_sha: "head-3".to_owned(),
        },
        2,
        BranchUpdateRequirement::Required,
        vec![
            required_check("quality", Some("15368")),
            AppliedRequiredCheck::new("legacy-status", None, AppliedRequiredCheckState::Pending)
                .expect("required check"),
        ],
    )
    .expect("requirements");

    let value = serde_json::to_value(&requirements).expect("serialize requirements");
    assert_eq!(
        value["repository"],
        serde_json::json!({"owner": "civitas-forge", "name": "interprex"})
    );
    assert_eq!(value["target_branch"], "main");
    assert_eq!(value["commit_range"]["base_sha"], "base-2");
    assert_eq!(value["commit_range"]["head_sha"], "head-3");
    assert_eq!(value["required_approvals"], 2);
    assert_eq!(value["branch_update"], "required");
    assert_eq!(value["required_checks"][0]["provider_application"], "15368");
    assert_eq!(value["required_checks"][1]["state"], "pending");
    assert_eq!(
        serde_json::from_value::<AppliedSourceRequirements>(value)
            .expect("deserialize requirements"),
        requirements
    );
}

#[test]
fn applied_requirements_reject_empty_exact_revision_fields() {
    for (target_branch, base_sha, head_sha, field) in [
        ("", "base", "head", "target branch"),
        ("main", "", "head", "base sha"),
        ("main", "base", "", "head sha"),
    ] {
        assert_eq!(
            AppliedSourceRequirements::new(
                repository(),
                target_branch,
                CommitRange {
                    base_sha: base_sha.to_owned(),
                    head_sha: head_sha.to_owned(),
                },
                0,
                BranchUpdateRequirement::NotRequired,
                vec![],
            ),
            Err(ModelError::Empty { field })
        );
    }
}

#[test]
fn required_checks_reject_empty_names_and_duplicate_identities() {
    assert_eq!(
        AppliedRequiredCheck::new("", None, AppliedRequiredCheckState::Missing),
        Err(ModelError::Empty {
            field: "required check name"
        })
    );

    let duplicate = required_check("quality", Some("15368"));
    assert!(matches!(
        AppliedSourceRequirements::new(
            repository(),
            "main",
            CommitRange {
                base_sha: "base".to_owned(),
                head_sha: "head".to_owned(),
            },
            1,
            BranchUpdateRequirement::Required,
            vec![duplicate.clone(), duplicate],
        ),
        Err(ModelError::DuplicateRequiredCheck { .. })
    ));
}

#[test]
fn the_same_check_name_from_distinct_provider_applications_is_not_a_duplicate() {
    let requirements = AppliedSourceRequirements::new(
        repository(),
        "main",
        CommitRange {
            base_sha: "base".to_owned(),
            head_sha: "head".to_owned(),
        },
        0,
        BranchUpdateRequirement::NotRequired,
        vec![
            required_check("quality", Some("15368")),
            required_check("quality", Some("20480")),
        ],
    )
    .expect("distinct native requirements");

    assert_eq!(requirements.required_checks().len(), 2);
}

#[test]
fn deserialization_revalidates_exact_subjects_and_check_identities() {
    let invalid = serde_json::json!({
        "repository": {"owner": "civitas-forge", "name": "interprex"},
        "target_branch": "main",
        "commit_range": {"base_sha": "base", "head_sha": ""},
        "required_approvals": 1,
        "branch_update": "required",
        "required_checks": [
            {"name": "quality", "provider_application": "15368", "state": "missing"}
        ]
    });
    assert!(serde_json::from_value::<AppliedSourceRequirements>(invalid).is_err());
}

#[test]
fn every_required_check_state_has_a_stable_serialized_name() {
    for (state, name) in [
        (AppliedRequiredCheckState::Missing, "missing"),
        (AppliedRequiredCheckState::Pending, "pending"),
        (AppliedRequiredCheckState::Satisfied, "satisfied"),
        (AppliedRequiredCheckState::Failed, "failed"),
    ] {
        let check = AppliedRequiredCheck::new("quality", None, state).expect("required check");
        assert_eq!(
            serde_json::to_value(check).expect("serialize check")["state"],
            name
        );
    }
}

fn accepts_object_safe_applied_provider(_: &dyn AppliedSourceRequirementsProvider) {}

fn accepts_configuration_provider<P>(_: &P)
where
    P: SourceCodeConfigurationProvider,
    P::Ruleset: Clone + Debug + DeserializeOwned + Serialize + Send + Sync + 'static,
{
}

struct InterfaceProof;

#[async_trait]
impl AppliedSourceRequirementsProvider for InterfaceProof {
    async fn applied_requirements(
        &self,
        _repository: &Repository,
        _target_branch: &str,
        _commit_range: &CommitRange,
    ) -> Result<AppliedSourceRequirements> {
        Err(ProviderError::Unsupported {
            provider: "proof",
            operation: "read applied source requirements",
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ProofRuleset;

#[async_trait]
impl SourceCodeConfigurationProvider for InterfaceProof {
    type Ruleset = ProofRuleset;

    async fn read_rulesets(&self, _repository: &Repository) -> Result<Vec<Self::Ruleset>> {
        Err(ProviderError::Unsupported {
            provider: "proof",
            operation: "read source rulesets",
        })
    }

    async fn apply_ruleset(
        &self,
        _repository: &Repository,
        _ruleset: &Self::Ruleset,
    ) -> Result<Self::Ruleset> {
        Err(ProviderError::Unsupported {
            provider: "proof",
            operation: "apply source ruleset",
        })
    }
}

#[test]
fn applied_provider_is_object_safe_and_configuration_retains_its_ruleset_type() {
    let provider = InterfaceProof;
    accepts_object_safe_applied_provider(&provider);
    accepts_configuration_provider(&provider);
}
