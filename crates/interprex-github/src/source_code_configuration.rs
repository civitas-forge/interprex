//! Complete GitHub ruleset values and source-configuration capabilities.

use std::collections::BTreeMap;

use async_trait::async_trait;
use interprex::{
    AppliedSourceRequirements, AppliedSourceRequirementsProvider, CommitRange, ModelError,
    ProviderError, Repository, Result, SourceCodeConfigurationProvider,
};
use octocrab::Page;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    GithubProvider,
    client::{authenticated_external, read_error},
};

const WRITABLE_RULE_TYPES: &[&str] = &[
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

const READ_ONLY_RULESET_FIELDS: &[&str] = &[
    "node_id",
    "_links",
    "current_user_can_bypass",
    "created_at",
    "updated_at",
];

#[derive(Deserialize)]
struct GithubRulesetSummary {
    id: u64,
    name: String,
    target: String,
    source_type: String,
    source: String,
    enforcement: String,
}

#[derive(Serialize)]
struct RulesetListQuery {
    per_page: u8,
    includes_parents: bool,
}

#[derive(Serialize)]
struct IncludesParents {
    includes_parents: bool,
}

#[derive(Deserialize)]
struct RulesetWriteReceipt {
    id: u64,
}

#[derive(Serialize)]
struct GithubRulesetWrite<'a> {
    name: &'a str,
    target: &'a str,
    enforcement: &'a str,
    bypass_actors: &'a [GithubRulesetBypassActor],
    #[serde(skip_serializing_if = "Option::is_none")]
    conditions: Option<&'a GithubRulesetConditions>,
    rules: &'a [GithubRulesetRule],
}

/// One GitHub repository ruleset, including read-only source identity and
/// every writable configuration field.
///
/// Optional fields remain absent rather than acquiring GitHub's documented
/// defaults. `additional` retains response fields added by GitHub. Rule
/// parameters and unrecognized rule forms are retained by [`GithubRulesetRule`]
/// without provider-neutral normalization. Applying a value rejects unknown
/// writable forms instead of omitting them from the GitHub request.
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

    async fn read_rulesets(&self, repository: &Repository) -> Result<Vec<Self::Ruleset>> {
        let page: Page<Value> = self
            .user()?
            .get(
                format!("/repos/{repository}/rulesets"),
                Some(&RulesetListQuery {
                    per_page: 100,
                    includes_parents: true,
                }),
            )
            .await
            .map_err(|error| {
                read_error(
                    "list source rulesets",
                    format!("source rulesets for {repository}"),
                    error,
                )
            })?;
        let summaries = self
            .user()?
            .all_pages(page)
            .await
            .map_err(|error| {
                read_error(
                    "list source rulesets",
                    format!("source rulesets for {repository}"),
                    error,
                )
            })?
            .into_iter()
            .map(parse_summary)
            .collect::<Result<Vec<_>>>()?;

        let mut rulesets = Vec::with_capacity(summaries.len());
        for summary in summaries {
            rulesets.push(self.read_ruleset_detail(repository, &summary).await?);
        }
        Ok(rulesets)
    }

    async fn apply_ruleset(
        &self,
        repository: &Repository,
        ruleset: &Self::Ruleset,
    ) -> Result<Self::Ruleset> {
        let desired = writable_ruleset(repository, ruleset)?;
        let desired_value = serde_json::to_value(&desired).map_err(|error| {
            invalid_input(format!("ruleset cannot be serialized for GitHub: {error}"))
        })?;
        let id = match ruleset.id {
            Some(id) => {
                let _: Value = self
                    .streaming_user()?
                    .put(
                        format!("/repos/{repository}/rulesets/{id}"),
                        Some(&desired_value),
                    )
                    .await
                    .map_err(|error| write_error("update source ruleset", Some(id), error))?;
                id
            }
            None => {
                let response: Value = self
                    .streaming_user()?
                    .post(
                        format!("/repos/{repository}/rulesets"),
                        Some(&desired_value),
                    )
                    .await
                    .map_err(|error| write_error("create source ruleset", None, error))?;
                parse_receipt(response)?
            }
        };

        let summary = GithubRulesetSummary {
            id,
            name: desired.name.to_owned(),
            target: desired.target.to_owned(),
            source_type: "Repository".to_owned(),
            source: repository.to_string(),
            enforcement: desired.enforcement.to_owned(),
        };
        let observed = self.read_ruleset_detail(repository, &summary).await?;
        let observed_value = serde_json::to_value(writable_ruleset(repository, &observed)?)
            .map_err(|error| {
                unrepresentable(format!("accepted ruleset cannot be read: {error}"))
            })?;
        if observed_value != desired_value {
            return Err(unrepresentable(format!(
                "ruleset {id} writable configuration changed between apply and verification"
            )));
        }
        Ok(observed)
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

impl GithubProvider {
    async fn read_ruleset_detail(
        &self,
        repository: &Repository,
        summary: &GithubRulesetSummary,
    ) -> Result<GithubRuleset> {
        let response: Value = self
            .user()?
            .get(
                format!("/repos/{repository}/rulesets/{}", summary.id),
                Some(&IncludesParents {
                    includes_parents: true,
                }),
            )
            .await
            .map_err(|error| {
                read_error(
                    "read source ruleset detail",
                    format!("source ruleset {} for {repository}", summary.id),
                    error,
                )
            })?;
        let ruleset: GithubRuleset = serde_json::from_value(response).map_err(|error| {
            unrepresentable(format!(
                "source ruleset {} detail is unreadable: {error}",
                summary.id
            ))
        })?;
        validate_detail(repository, summary, &ruleset)?;
        Ok(ruleset)
    }
}

fn parse_summary(value: Value) -> Result<GithubRulesetSummary> {
    let summary: GithubRulesetSummary = serde_json::from_value(value).map_err(|error| {
        unrepresentable(format!("source ruleset summary is unreadable: {error}"))
    })?;
    if summary.id == 0 {
        return Err(unrepresentable("source ruleset summary has a zero id"));
    }
    for (value, fact) in [
        (&summary.name, "name"),
        (&summary.target, "target"),
        (&summary.source_type, "source type"),
        (&summary.source, "source"),
        (&summary.enforcement, "enforcement"),
    ] {
        if value.is_empty() {
            return Err(unrepresentable(format!(
                "source ruleset {} summary has an empty {fact}",
                summary.id
            )));
        }
    }
    Ok(summary)
}

fn validate_detail(
    repository: &Repository,
    summary: &GithubRulesetSummary,
    ruleset: &GithubRuleset,
) -> Result<()> {
    ruleset.validate().map_err(|error| {
        unrepresentable(format!(
            "source ruleset {} detail is invalid: {error}",
            summary.id
        ))
    })?;
    if ruleset.id != Some(summary.id) {
        return Err(unrepresentable(format!(
            "source ruleset {} detail identifies {:?}",
            summary.id, ruleset.id
        )));
    }
    let target = ruleset
        .target
        .as_deref()
        .ok_or_else(|| unrepresentable(format!("source ruleset {} has no target", summary.id)))?;
    if !matches!(target, "branch" | "tag" | "push" | "repository") {
        return Err(unsupported("read an unknown ruleset target"));
    }
    let source_type = ruleset.source_type.as_deref().ok_or_else(|| {
        unrepresentable(format!("source ruleset {} has no source type", summary.id))
    })?;
    if !matches!(source_type, "Repository" | "Organization" | "Enterprise") {
        return Err(unsupported("read an unknown ruleset source type"));
    }
    let source = ruleset
        .source
        .as_deref()
        .ok_or_else(|| unrepresentable(format!("source ruleset {} has no source", summary.id)))?;
    if ruleset.source_type.as_deref() != Some(summary.source_type.as_str())
        || ruleset.source.as_deref() != Some(summary.source.as_str())
    {
        return Err(unrepresentable(format!(
            "source ruleset {} detail has a different source identity",
            summary.id
        )));
    }
    if source_type == "Repository" && source != repository.to_string() {
        return Err(unrepresentable(format!(
            "source ruleset {} belongs to repository {source}, not {repository}",
            summary.id
        )));
    }
    if ruleset.bypass_actors.is_none() {
        return Err(unsupported(
            "read complete rulesets without bypass-actor access",
        ));
    }
    if ruleset.rules.is_none() {
        return Err(unrepresentable(format!(
            "source ruleset {} detail has no rules",
            summary.id
        )));
    }
    Ok(())
}

fn writable_ruleset<'a>(
    repository: &Repository,
    ruleset: &'a GithubRuleset,
) -> Result<GithubRulesetWrite<'a>> {
    ruleset
        .validate()
        .map_err(|error| invalid_input(error.to_string()))?;
    let target = ruleset
        .target
        .as_deref()
        .ok_or_else(|| invalid_input("ruleset target must be explicit"))?;
    if target == "repository" {
        return Err(unsupported(
            "apply repository-target rulesets through a repository endpoint",
        ));
    }
    if !matches!(target, "branch" | "tag" | "push") {
        return Err(unsupported("apply an unknown ruleset target"));
    }
    if !matches!(
        ruleset.enforcement.as_str(),
        "disabled" | "active" | "evaluate"
    ) {
        return Err(unsupported("apply an unknown ruleset enforcement value"));
    }
    validate_write_source(repository, ruleset)?;
    validate_write_fields(ruleset)?;
    let bypass_actors = ruleset
        .bypass_actors
        .as_deref()
        .ok_or_else(|| invalid_input("complete ruleset requires bypass_actors, including empty"))?;
    let rules = ruleset
        .rules
        .as_deref()
        .ok_or_else(|| invalid_input("complete ruleset requires rules, including empty"))?;
    Ok(GithubRulesetWrite {
        name: &ruleset.name,
        target,
        enforcement: &ruleset.enforcement,
        bypass_actors,
        conditions: ruleset.conditions.as_ref(),
        rules,
    })
}

fn validate_write_source(repository: &Repository, ruleset: &GithubRuleset) -> Result<()> {
    match ruleset.id {
        Some(_) => match (ruleset.source_type.as_deref(), ruleset.source.as_deref()) {
            (Some("Repository"), Some(source)) if source == repository.to_string() => Ok(()),
            (Some("Repository"), Some(_)) => Err(invalid_input(
                "ruleset repository source does not match the destination repository",
            )),
            (Some("Organization"), Some(_)) | (Some("Enterprise"), Some(_)) => Err(unsupported(
                "apply an inherited ruleset through a repository endpoint",
            )),
            (Some(_), Some(_)) => Err(unsupported("apply an unknown ruleset source type")),
            _ => Err(invalid_input(
                "existing ruleset requires its repository source identity",
            )),
        },
        None if ruleset.source_type.is_some() || ruleset.source.is_some() => Err(invalid_input(
            "new ruleset must not carry read-only source identity",
        )),
        None => Ok(()),
    }
}

fn validate_write_fields(ruleset: &GithubRuleset) -> Result<()> {
    if ruleset
        .additional
        .keys()
        .any(|field| !READ_ONLY_RULESET_FIELDS.contains(&field.as_str()))
    {
        return Err(unsupported(
            "apply a ruleset with an unknown top-level field",
        ));
    }
    if ruleset.id.is_none() && !ruleset.additional.is_empty() {
        return Err(invalid_input(
            "new ruleset must not carry read-only provider metadata",
        ));
    }
    if let Some(conditions) = &ruleset.conditions
        && (!conditions.additional.is_empty()
            || conditions
                .ref_name
                .as_ref()
                .is_some_and(|condition| !condition.additional.is_empty()))
    {
        return Err(unsupported("apply a ruleset with an unknown condition"));
    }
    if let Some(ref_name) = ruleset
        .conditions
        .as_ref()
        .and_then(|conditions| conditions.ref_name.as_ref())
        && (ref_name.include.is_none() || ref_name.exclude.is_none())
    {
        return Err(invalid_input(
            "a ref-name condition requires explicit include and exclude collections",
        ));
    }
    if let Some(actors) = &ruleset.bypass_actors {
        for actor in actors {
            if actor
                .fields
                .keys()
                .any(|field| !matches!(field.as_str(), "actor_id" | "bypass_mode"))
            {
                return Err(unsupported("apply a ruleset with an unknown bypass field"));
            }
            if !matches!(
                actor.actor_type.as_str(),
                "Integration"
                    | "OrganizationAdmin"
                    | "RepositoryRole"
                    | "Team"
                    | "DeployKey"
                    | "User"
            ) {
                return Err(unsupported(
                    "apply a ruleset with an unknown bypass actor type",
                ));
            }
            match actor.actor_id() {
                Some(Value::Null) => {}
                Some(Value::Number(id)) if id.as_u64().is_some_and(|id| id > 0) => {}
                _ => {
                    return Err(invalid_input(
                        "a bypass actor requires an explicit null or positive integer actor_id",
                    ));
                }
            }
            let mode = actor
                .bypass_mode()
                .and_then(Value::as_str)
                .ok_or_else(|| invalid_input("a bypass actor requires a string bypass_mode"))?;
            if !matches!(mode, "always" | "pull_request" | "exempt") {
                return Err(unsupported("apply a ruleset with an unknown bypass mode"));
            }
        }
    }
    if let Some(rules) = &ruleset.rules {
        for rule in rules {
            if !WRITABLE_RULE_TYPES.contains(&rule.rule_type.as_str()) {
                return Err(unsupported("apply an unknown ruleset rule"));
            }
            if rule
                .fields
                .keys()
                .any(|field| field.as_str() != "parameters")
            {
                return Err(unsupported("apply a ruleset rule with an unknown field"));
            }
        }
    }
    Ok(())
}

fn parse_receipt(value: Value) -> Result<u64> {
    let receipt: RulesetWriteReceipt = serde_json::from_value(value)
        .map_err(|error| unrepresentable(format!("created ruleset has no readable id: {error}")))?;
    if receipt.id == 0 {
        Err(unrepresentable("created ruleset has a zero id"))
    } else {
        Ok(receipt.id)
    }
}

fn write_error(
    operation: &'static str,
    ruleset_id: Option<u64>,
    error: octocrab::Error,
) -> ProviderError {
    if matches!(
        &error,
        octocrab::Error::GitHub { source, .. } if source.status_code.as_u16() == 404
    ) {
        return ProviderError::NotFound {
            entity: ruleset_id.map_or_else(
                || "repository rulesets".to_owned(),
                |id| format!("source ruleset {id}"),
            ),
        };
    }
    if let octocrab::Error::GitHub { source, .. } = &error
        && source.status_code.as_u16() == 422
        && let Some(errors) = source.errors.as_deref()
        && !errors.is_empty()
        && let Some(mut fields) = errors
            .iter()
            .map(ruleset_validation_field)
            .collect::<Option<Vec<_>>>()
    {
        fields.sort_unstable();
        fields.dedup();
        return invalid_input(format!(
            "GitHub rejected these ruleset fields: {}",
            fields.join(", ")
        ));
    }
    authenticated_external(operation, &error)
}

fn ruleset_validation_field(error: &Value) -> Option<&'static str> {
    let field = error.get("field")?.as_str()?;
    let code = error.get("code")?.as_str()?;
    if !matches!(
        code,
        "custom" | "invalid" | "missing" | "missing_field" | "unprocessable"
    ) {
        return None;
    }
    match field {
        "name" => Some("name"),
        "target" => Some("target"),
        "enforcement" => Some("enforcement"),
        "bypass_actors" => Some("bypass_actors"),
        "conditions" => Some("conditions"),
        "rules" => Some("rules"),
        _ => None,
    }
}

fn invalid_input(fact: impl Into<String>) -> ProviderError {
    ProviderError::InvalidInput {
        provider: "github",
        fact: fact.into(),
    }
}

fn unrepresentable(fact: impl Into<String>) -> ProviderError {
    ProviderError::Unrepresentable {
        provider: "github",
        fact: fact.into(),
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
