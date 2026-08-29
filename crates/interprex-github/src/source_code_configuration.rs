//! Complete GitHub ruleset values and source-configuration capabilities.

use std::collections::BTreeMap;

use async_trait::async_trait;
use interprex::{
    AppliedSourceRequirements, AppliedSourceRequirementsProvider, CommitRange, ModelError,
    ProviderError, Repository, Result, SourceCodeConfigurationProvider,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::GithubProvider;

/// One GitHub repository ruleset, including read-only source identity and
/// every writable configuration field.
///
/// Optional fields remain absent rather than acquiring GitHub's documented
/// defaults. `additional` retains response fields added by GitHub. Rule
/// parameters and unrecognized rule forms are retained by [`GithubRulesetRule`]
/// without provider-neutral normalization.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct GithubRuleset {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub enforcement: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bypass_actors: Option<Vec<GithubRulesetBypassActor>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conditions: Option<GithubRulesetConditions>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rules: Option<Vec<GithubRulesetRule>>,
    #[serde(flatten)]
    pub additional: BTreeMap<String, Value>,
}

impl GithubRuleset {
    /// Checks local invariants without interpreting unknown GitHub values.
    ///
    /// This rejects empty identities and zero numeric identifiers. It does not
    /// replace provider validation of whether a retained rule, target, or
    /// parameter is writable in the repository that receives it.
    pub fn validate(&self) -> std::result::Result<(), ModelError> {
        non_empty(&self.name, "GitHub ruleset name")?;
        non_empty(&self.enforcement, "GitHub ruleset enforcement")?;
        if self.id == Some(0) {
            return Err(ModelError::InvalidNumber);
        }
        for (value, field) in [
            (self.target.as_deref(), "GitHub ruleset target"),
            (self.source_type.as_deref(), "GitHub ruleset source type"),
            (self.source.as_deref(), "GitHub ruleset source"),
        ] {
            if let Some(value) = value {
                non_empty(value, field)?;
            }
        }
        if let Some(actors) = &self.bypass_actors {
            for actor in actors {
                actor.validate()?;
            }
        }
        if let Some(rules) = &self.rules {
            for rule in rules {
                rule.validate()?;
            }
        }
        Ok(())
    }
}

/// GitHub ruleset conditions.
///
/// `ref_name` exposes include and exclusion patterns. Conditions used by
/// organization or future rulesets remain in `additional` and round-trip
/// unchanged.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct GithubRulesetConditions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_name: Option<GithubRefNameCondition>,
    #[serde(flatten)]
    pub additional: BTreeMap<String, Value>,
}

/// Include and exclusion patterns for a GitHub ref-name condition.
///
/// Absence is distinct from an explicitly empty collection, so Interprex does
/// not invent a target pattern or exclusion list.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct GithubRefNameCondition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<Vec<String>>,
    #[serde(flatten)]
    pub additional: BTreeMap<String, Value>,
}

/// A GitHub actor allowed to bypass a ruleset.
///
/// Provider strings are retained verbatim so a newer actor type or bypass
/// mode is visible to configuration tools rather than discarded. A missing
/// mode remains missing instead of being changed to GitHub's default.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct GithubRulesetBypassActor {
    pub actor_type: String,
    #[serde(flatten)]
    pub fields: BTreeMap<String, Value>,
}

impl GithubRulesetBypassActor {
    fn validate(&self) -> std::result::Result<(), ModelError> {
        non_empty(&self.actor_type, "GitHub bypass actor type")?;
        if self.fields.get("actor_id").and_then(Value::as_u64) == Some(0) {
            return Err(ModelError::InvalidNumber);
        }
        if let Some(mode) = self.fields.get("bypass_mode").and_then(Value::as_str) {
            non_empty(mode, "GitHub bypass mode")?;
        }
        Ok(())
    }

    /// The actor ID exactly as GitHub represented it, including explicit null.
    #[must_use]
    pub fn actor_id(&self) -> Option<&Value> {
        self.fields.get("actor_id")
    }

    /// The bypass mode exactly as GitHub represented it.
    #[must_use]
    pub fn bypass_mode(&self) -> Option<&Value> {
        self.fields.get("bypass_mode")
    }
}

/// One GitHub ruleset rule in its complete native shape.
///
/// `rule_type` identifies the documented or future rule. `fields` retains all
/// remaining fields, including the complete `parameters` object for rules that
/// have one. Parameterless and unknown rules therefore round-trip without a
/// fabricated empty parameters object.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct GithubRulesetRule {
    #[serde(rename = "type")]
    pub rule_type: String,
    #[serde(flatten)]
    pub fields: BTreeMap<String, Value>,
}

impl GithubRulesetRule {
    fn validate(&self) -> std::result::Result<(), ModelError> {
        non_empty(&self.rule_type, "GitHub rule type")
    }

    /// The complete GitHub parameters value when this rule carries one.
    #[must_use]
    pub fn parameters(&self) -> Option<&Value> {
        self.fields.get("parameters")
    }
}

#[async_trait]
impl SourceCodeConfigurationProvider for GithubProvider {
    type Ruleset = GithubRuleset;

    async fn read_rulesets(&self, _repository: &Repository) -> Result<Vec<Self::Ruleset>> {
        Err(unsupported("read source rulesets"))
    }

    async fn apply_ruleset(
        &self,
        _repository: &Repository,
        _ruleset: &Self::Ruleset,
    ) -> Result<Self::Ruleset> {
        Err(unsupported("apply source ruleset"))
    }
}

#[async_trait]
impl AppliedSourceRequirementsProvider for GithubProvider {
    async fn applied_requirements(
        &self,
        _repository: &Repository,
        _target_branch: &str,
        _commit_range: &CommitRange,
    ) -> Result<AppliedSourceRequirements> {
        Err(unsupported("read applied source requirements"))
    }
}

fn unsupported(operation: &'static str) -> ProviderError {
    ProviderError::Unsupported {
        provider: "github",
        operation,
    }
}

fn non_empty(value: &str, field: &'static str) -> std::result::Result<(), ModelError> {
    if value.is_empty() {
        Err(ModelError::Empty { field })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_ruleset_round_trips_known_and_unknown_native_fields() {
        let fixture = serde_json::json!({
            "id": 7,
            "name": "pull requests",
            "target": "branch",
            "source_type": "Repository",
            "source": "civitas-forge/interprex",
            "enforcement": "evaluate",
            "bypass_actors": [
                {"actor_id": 15368, "actor_type": "Integration", "bypass_mode": "pull_request"},
                {"actor_id": null, "actor_type": "FutureActor", "bypass_mode": "future_mode", "future": true}
            ],
            "conditions": {
                "ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": ["refs/heads/release/*"]},
                "repository_name": {"include": ["important-*"], "exclude": ["archive-*"], "protected": true}
            },
            "rules": [
                {"type": "required_status_checks", "parameters": {
                    "strict_required_status_checks_policy": true,
                    "do_not_enforce_on_create": false,
                    "required_status_checks": [{"context": "quality", "integration_id": 15368}]
                }},
                {"type": "required_signatures"},
                {"type": "future_rule", "parameters": {"new_option": true}, "future": "retained"}
            ],
            "current_user_can_bypass": "never",
            "future_top_level": {"retained": true}
        });

        let ruleset: GithubRuleset =
            serde_json::from_value(fixture.clone()).expect("deserialize complete ruleset");
        ruleset.validate().expect("valid ruleset");
        assert_eq!(
            ruleset
                .conditions
                .as_ref()
                .and_then(|conditions| conditions.ref_name.as_ref())
                .and_then(|condition| condition.exclude.as_ref()),
            Some(&vec!["refs/heads/release/*".to_owned()])
        );
        assert_eq!(
            ruleset.rules.as_ref().expect("rules")[0].parameters(),
            Some(&fixture["rules"][0]["parameters"])
        );
        assert_eq!(
            serde_json::to_value(ruleset).expect("serialize complete ruleset"),
            fixture
        );
    }

    #[test]
    fn ruleset_validation_rejects_empty_native_identities() {
        let ruleset: GithubRuleset = serde_json::from_value(serde_json::json!({
            "id": 7,
            "name": "pull requests",
            "enforcement": "active",
            "rules": [{"type": ""}]
        }))
        .expect("deserialize ruleset");
        assert_eq!(
            ruleset.validate(),
            Err(ModelError::Empty {
                field: "GitHub rule type"
            })
        );
    }

    #[test]
    fn every_current_writable_rule_kind_retains_its_complete_fields() {
        let kinds = [
            "creation",
            "update",
            "deletion",
            "required_linear_history",
            "merge_queue",
            "required_deployments",
            "required_signatures",
            "pull_request",
            "required_status_checks",
            "non_fast_forward",
            "commit_message_pattern",
            "commit_author_email_pattern",
            "committer_email_pattern",
            "branch_name_pattern",
            "tag_name_pattern",
            "workflows",
            "code_scanning",
            "copilot_code_review",
            "license_compliance_scanning",
            "file_path_restriction",
            "max_file_path_length",
            "file_extension_restriction",
            "max_file_size",
        ];
        for kind in kinds {
            let fixture = serde_json::json!({
                "type": kind,
                "parameters": {"provider_owned": [true, 7, "value"]},
                "future_field": {"retained": true}
            });
            let rule: GithubRulesetRule =
                serde_json::from_value(fixture.clone()).expect("deserialize rule");
            assert_eq!(serde_json::to_value(rule).expect("serialize rule"), fixture);
        }
    }
}
