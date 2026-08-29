use async_trait::async_trait;
use interprex::{
    BranchFreshness, BranchUpdateError, BranchUpdateObservation, BranchUpdateRequirement,
    BranchUpdatesProvider, ChangeRequestNumber, CommitRange, ProviderError, Repository, Result,
};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    GithubProvider,
    client::{authenticated_external, is_not_found, read_error},
};

#[derive(Deserialize)]
struct GithubBranchRule {
    #[serde(rename = "type")]
    kind: String,
    parameters: Option<Value>,
}

#[derive(Deserialize)]
struct GithubComparison {
    status: String,
}

#[derive(Deserialize)]
struct GithubBranchProtection {
    required_status_checks: Option<GithubRequiredStatusChecks>,
}

#[derive(Deserialize)]
struct GithubRequiredStatusChecks {
    strict: bool,
}

fn update_requirement(
    rules: &[GithubBranchRule],
    classic_protection: Option<&GithubBranchProtection>,
) -> Result<BranchUpdateRequirement> {
    for rule in rules
        .iter()
        .filter(|rule| rule.kind == "required_status_checks")
    {
        let parameters = rule
            .parameters
            .as_ref()
            .and_then(Value::as_object)
            .ok_or_else(|| {
                unrepresentable("an applicable required-checks rule has no parameters")
            })?;
        let strict = parameters
            .get("strict_required_status_checks_policy")
            .and_then(Value::as_bool)
            .ok_or_else(|| {
                unrepresentable(
                    "an applicable required-checks rule has no branch-freshness requirement",
                )
            })?;
        let checks = parameters
            .get("required_status_checks")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                unrepresentable("an applicable required-checks rule has no required-check list")
            })?;
        if strict && !checks.is_empty() {
            return Ok(BranchUpdateRequirement::Required);
        }
    }
    if classic_protection
        .and_then(|protection| protection.required_status_checks.as_ref())
        .is_some_and(|checks| checks.strict)
    {
        return Ok(BranchUpdateRequirement::Required);
    }
    Ok(BranchUpdateRequirement::NotRequired)
}

fn freshness(status: &str) -> Result<BranchFreshness> {
    match status {
        "ahead" | "identical" => Ok(BranchFreshness::Current),
        "behind" | "diverged" => Ok(BranchFreshness::Behind),
        other => Err(unrepresentable(format!(
            "unknown commit comparison status {other}"
        ))),
    }
}

fn unrepresentable(fact: impl Into<String>) -> ProviderError {
    ProviderError::Unrepresentable {
        provider: "github",
        fact: fact.into(),
    }
}

fn stale_head(expected: &str, observed: &str) -> BranchUpdateError {
    BranchUpdateError::StaleHead {
        expected_head_sha: expected.to_owned(),
        observed_head_sha: observed.to_owned(),
    }
}

#[async_trait]
impl BranchUpdatesProvider for GithubProvider {
    async fn branch_update(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
    ) -> Result<BranchUpdateObservation> {
        let pull_request = self.github_pull_request(repository, number).await?;
        let base_branch = utf8_percent_encode(&pull_request.base.branch, NON_ALPHANUMERIC);
        let rules: Vec<GithubBranchRule> = self
            .user()?
            .get(
                format!("/repos/{repository}/rules/branches/{base_branch}"),
                None::<&()>,
            )
            .await
            .map_err(|error| {
                read_error(
                    "read applicable branch rules",
                    format!(
                        "applicable rules for {} in {repository}",
                        pull_request.base.branch
                    ),
                    error,
                )
            })?;
        let classic_protection: Option<GithubBranchProtection> = match self
            .user()?
            .get(
                format!("/repos/{repository}/branches/{base_branch}/protection"),
                None::<&()>,
            )
            .await
        {
            Ok(protection) => Some(protection),
            Err(error) if is_not_found(&error) => None,
            Err(error) => {
                return Err(read_error(
                    "read classic branch protection",
                    format!(
                        "classic protection for {} in {repository}",
                        pull_request.base.branch
                    ),
                    error,
                ));
            }
        };
        let comparison: GithubComparison = self
            .user()?
            .get(
                format!(
                    "/repos/{repository}/compare/{}...{}",
                    pull_request.base.sha, pull_request.head.sha
                ),
                None::<&()>,
            )
            .await
            .map_err(|error| {
                read_error(
                    "compare change request revisions",
                    format!(
                        "revisions {} and {} in {repository}",
                        pull_request.base.sha, pull_request.head.sha
                    ),
                    error,
                )
            })?;

        Ok(BranchUpdateObservation {
            commit_range: CommitRange {
                base_sha: pull_request.base.sha,
                head_sha: pull_request.head.sha,
            },
            requirement: update_requirement(&rules, classic_protection.as_ref())?,
            freshness: freshness(&comparison.status)?,
        })
    }

    async fn update_change_request_branch(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        expected_head_sha: &str,
    ) -> std::result::Result<(), BranchUpdateError> {
        if expected_head_sha.is_empty() {
            return Err(ProviderError::InvalidInput {
                provider: "github",
                fact: "expected change-request head must not be empty".to_owned(),
            }
            .into());
        }
        let observed = self.github_pull_request(repository, number).await?;
        if observed.head.sha != expected_head_sha {
            return Err(stale_head(expected_head_sha, &observed.head.sha));
        }

        // This client does not retry writes. If the response is lost or GitHub
        // refuses the update, the reread below reports a changed head as stale
        // and preserves every other result as an external failure.
        let write = self
            .streaming_user()?
            .put::<Value, _, _>(
                format!("/repos/{repository}/pulls/{}/update-branch", number.get()),
                Some(&json!({"expected_head_sha": expected_head_sha})),
            )
            .await;
        match write {
            Ok(_) => Ok(()),
            Err(error) => {
                let failure = authenticated_external("update change request branch", &error);
                match self.github_pull_request(repository, number).await {
                    Ok(current) if current.head.sha != expected_head_sha => {
                        Err(stale_head(expected_head_sha, &current.head.sha))
                    }
                    Err(error @ ProviderError::NotFound { .. }) => Err(error.into()),
                    Ok(_) | Err(_) => Err(failure.into()),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_required_check_rules_are_not_guessed() {
        let rules = [GithubBranchRule {
            kind: "required_status_checks".to_owned(),
            parameters: Some(json!({"required_status_checks": []})),
        }];
        assert!(matches!(
            update_requirement(&rules, None),
            Err(ProviderError::Unrepresentable { .. })
        ));
    }
}
