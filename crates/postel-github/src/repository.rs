//! Repository settings, rules, and secret transport owned by the repo domain.
//!
//! Settings use GitHub's typed repository response. Rulesets use the raw REST
//! route because Octocrab does not type their policy structure; normalization
//! keeps only provider-neutral enforcement, branch, check, and approval facts.
//! Secret values are sealed with the repository public key before the write and
//! are never retained by the provider.

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use crypto_box::{PublicKey, aead::OsRng};
use octocrab::Page;
use postel::{
    ProviderError, Repository, RepositoryFacts, RepositoryProvider, RepositorySettings,
    RequiredCheck, Result, Ruleset,
};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    GithubProvider,
    client::{external, read_error},
};

#[derive(Deserialize)]
struct GithubRepository {
    owner: GithubOwner,
    name: String,
    default_branch: String,
    private: bool,
    archived: bool,
    allow_squash_merge: bool,
    allow_merge_commit: bool,
    allow_rebase_merge: bool,
    delete_branch_on_merge: bool,
}

#[derive(Deserialize)]
struct GithubOwner {
    login: String,
}

#[derive(Serialize)]
struct GithubRepositorySettings {
    #[serde(rename = "allow_squash_merge")]
    squash_merge: bool,
    #[serde(rename = "allow_merge_commit")]
    merge_commit: bool,
    #[serde(rename = "allow_rebase_merge")]
    rebase_merge: bool,
    #[serde(rename = "delete_branch_on_merge")]
    delete_branch_after_merge: bool,
}

impl From<&RepositorySettings> for GithubRepositorySettings {
    fn from(value: &RepositorySettings) -> Self {
        Self {
            squash_merge: value.allow_squash_merge,
            merge_commit: value.allow_merge_commit,
            rebase_merge: value.allow_rebase_merge,
            delete_branch_after_merge: value.delete_branch_on_merge,
        }
    }
}

impl TryFrom<GithubRepository> for (RepositoryFacts, RepositorySettings) {
    type Error = ProviderError;

    fn try_from(value: GithubRepository) -> Result<Self> {
        let repository = Repository::new(value.owner.login, value.name).map_err(|error| {
            ProviderError::External {
                provider: "github",
                operation: "normalize repository",
                message: error.to_string(),
            }
        })?;
        Ok((
            RepositoryFacts {
                repository,
                default_branch: value.default_branch,
                private: value.private,
                archived: value.archived,
            },
            RepositorySettings {
                allow_squash_merge: value.allow_squash_merge,
                allow_merge_commit: value.allow_merge_commit,
                allow_rebase_merge: value.allow_rebase_merge,
                delete_branch_on_merge: value.delete_branch_on_merge,
            },
        ))
    }
}

#[derive(Deserialize, Serialize)]
struct GithubRuleset {
    id: Option<u64>,
    name: String,
    enforcement: String,
    #[serde(default)]
    conditions: Conditions,
    #[serde(default)]
    rules: Vec<Rule>,
}

#[derive(Default, Deserialize, Serialize)]
struct Conditions {
    #[serde(default)]
    ref_name: RefName,
}

#[derive(Default, Deserialize, Serialize)]
struct RefName {
    #[serde(default)]
    include: Vec<String>,
}

#[derive(Deserialize, Serialize)]
struct Rule {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    parameters: serde_json::Value,
}

fn normalize_ruleset(value: GithubRuleset) -> Ruleset {
    let mut checks = Vec::new();
    let mut approvals = 0;
    for rule in value.rules {
        if rule.kind == "required_status_checks" {
            checks = rule.parameters["required_status_checks"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|check| {
                    Some(RequiredCheck {
                        context: check.get("context")?.as_str()?.to_owned(),
                        integration_id: check
                            .get("integration_id")
                            .and_then(serde_json::Value::as_u64),
                    })
                })
                .collect();
        } else if rule.kind == "pull_request" {
            approvals = rule.parameters["required_approving_review_count"]
                .as_u64()
                .and_then(|count| u8::try_from(count).ok())
                .unwrap_or(0);
        }
    }
    Ruleset {
        id: value.id,
        name: value.name,
        active: value.enforcement == "active",
        target_branch_patterns: value.conditions.ref_name.include,
        required_checks: checks,
        required_approvals: approvals,
    }
}

fn ruleset_body(ruleset: &Ruleset) -> serde_json::Value {
    let mut rules = vec![json!({
        "type": "required_status_checks",
        "parameters": {
            "strict_required_status_checks_policy": true,
            "do_not_enforce_on_create": false,
            "required_status_checks": ruleset.required_checks,
        }
    })];
    if ruleset.required_approvals > 0 {
        rules.push(json!({
            "type": "pull_request",
            "parameters": {
                "required_approving_review_count": ruleset.required_approvals,
                "dismiss_stale_reviews_on_push": true,
                "require_code_owner_review": false,
                "require_last_push_approval": false,
                "required_review_thread_resolution": true,
            }
        }));
    }
    json!({
        "name": ruleset.name,
        "target": "branch",
        "enforcement": if ruleset.active { "active" } else { "disabled" },
        "conditions": { "ref_name": { "include": ruleset.target_branch_patterns, "exclude": [] } },
        "rules": rules,
        "bypass_actors": [],
    })
}

#[async_trait]
impl RepositoryProvider for GithubProvider {
    async fn repository(&self, repository: &Repository) -> Result<RepositoryFacts> {
        let response: GithubRepository = self
            .user()?
            .get(format!("/repos/{repository}"), None::<&()>)
            .await
            .map_err(|error| read_error("read repository", repository.to_string(), error))?;
        response.try_into().map(|(facts, _)| facts)
    }

    async fn settings(&self, repository: &Repository) -> Result<RepositorySettings> {
        let response: GithubRepository = self
            .user()?
            .get(format!("/repos/{repository}"), None::<&()>)
            .await
            .map_err(|error| {
                read_error("read repository settings", repository.to_string(), error)
            })?;
        response.try_into().map(|(_, settings)| settings)
    }

    async fn apply_settings(
        &self,
        repository: &Repository,
        settings: &RepositorySettings,
    ) -> Result<RepositorySettings> {
        let body = GithubRepositorySettings::from(settings);
        let response: GithubRepository = self
            .user()?
            .patch(format!("/repos/{repository}"), Some(&body))
            .await
            .map_err(|error| external("apply repository settings", error))?;
        response.try_into().map(|(_, settings)| settings)
    }

    async fn rulesets(&self, repository: &Repository) -> Result<Vec<Ruleset>> {
        let page: Page<GithubRuleset> = self
            .user()?
            .get(
                format!("/repos/{repository}/rulesets"),
                Some(&[("per_page", 100)]),
            )
            .await
            .map_err(|error| external("list repository rulesets", error))?;
        let response = self
            .user()?
            .all_pages(page)
            .await
            .map_err(|error| external("list repository rulesets", error))?;
        Ok(response.into_iter().map(normalize_ruleset).collect())
    }

    async fn upsert_ruleset(&self, repository: &Repository, ruleset: &Ruleset) -> Result<Ruleset> {
        let body = ruleset_body(ruleset);
        let response: GithubRuleset = if let Some(id) = ruleset.id {
            self.user()?
                .put(format!("/repos/{repository}/rulesets/{id}"), Some(&body))
                .await
        } else {
            self.user()?
                .post(format!("/repos/{repository}/rulesets"), Some(&body))
                .await
        }
        .map_err(|error| external("upsert repository ruleset", error))?;
        Ok(normalize_ruleset(response))
    }

    async fn put_secret(
        &self,
        repository: &Repository,
        name: &str,
        value: SecretString,
    ) -> Result<()> {
        #[derive(Deserialize)]
        struct Key {
            key_id: String,
            key: String,
        }
        let key: Key = self
            .user()?
            .get(
                format!("/repos/{repository}/actions/secrets/public-key"),
                None::<&()>,
            )
            .await
            .map_err(|error| external("read repository secret public key", error))?;
        let decoded = STANDARD
            .decode(key.key)
            .map_err(|error| external("decode repository secret public key", error))?;
        let public_key = PublicKey::from_slice(&decoded)
            .map_err(|error| external("parse repository secret public key", error))?;
        let encrypted = public_key
            .seal(&mut OsRng, value.expose_secret().as_bytes())
            .map_err(|error| external("encrypt repository secret", error))?;
        let _: serde_json::Value = self
            .user()?
            .put(
                format!("/repos/{repository}/actions/secrets/{name}"),
                Some(
                    &json!({ "encrypted_value": STANDARD.encode(encrypted), "key_id": key.key_id }),
                ),
            )
            .await
            .map_err(|error| external("write repository secret", error))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{GithubRepository, GithubRuleset, normalize_ruleset, ruleset_body};

    #[test]
    fn repository_fixture_normalizes_only_contract_facts() {
        let fixture = include_str!("../tests/fixtures/repository.json");
        let response: GithubRepository = serde_json::from_str(fixture).expect("fixture");
        let (facts, settings) = response.try_into().expect("normalizes");
        assert_eq!(facts.repository.to_string(), "faictor/postel-sandbox");
        assert!(settings.allow_squash_merge);
        assert!(!settings.allow_merge_commit);
    }

    #[test]
    fn ruleset_fixture_round_trips_its_enforced_policy() {
        let fixture = include_str!("../tests/fixtures/ruleset.json");
        let response: GithubRuleset = serde_json::from_str(fixture).expect("fixture");
        let ruleset = normalize_ruleset(response);
        assert_eq!(ruleset.required_checks[0].context, "quality");
        assert_eq!(ruleset.required_approvals, 1);
        let body = ruleset_body(&ruleset);
        assert_eq!(body["target"], "branch");
    }
}
