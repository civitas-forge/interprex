//! Requirements GitHub applies to an exact branch and head revision.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use interprex::{
    AppliedRequiredCheck, AppliedRequiredCheckState, AppliedSourceRequirements,
    AppliedSourceRequirementsProvider, BranchUpdateRequirement, CheckConclusion, CheckRun,
    CheckStatus, CommitRange, ProviderAppId, ProviderError, Repository, Result,
};
use octocrab::Page;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    GithubProvider,
    client::{authenticated_external, is_not_found, read_error},
    code_reviews::checks::normalize_check_run,
};

const PAGE_SIZE: usize = 100;

#[derive(Deserialize)]
struct GithubBranch {
    name: String,
    commit: GithubBranchCommit,
}

#[derive(Deserialize)]
struct GithubBranchCommit {
    sha: String,
}

#[derive(Deserialize)]
struct GithubAppliedRule {
    #[serde(rename = "type")]
    rule_type: String,
    #[serde(default)]
    parameters: Option<Value>,
}

#[derive(Deserialize)]
struct GithubStatusCheckParameters {
    strict_required_status_checks_policy: bool,
    required_status_checks: Vec<GithubRequiredCheck>,
}

#[derive(Deserialize)]
struct GithubRequiredCheck {
    context: String,
    #[serde(default)]
    integration_id: Option<i64>,
}

#[derive(Deserialize)]
struct GithubPullRequestParameters {
    #[serde(default, rename = "allowed_merge_methods")]
    _allowed_merge_methods: Vec<String>,
    dismiss_stale_reviews_on_push: bool,
    #[serde(default)]
    dismissal_restriction: Option<GithubRulesetDismissalRestriction>,
    require_code_owner_review: bool,
    require_last_push_approval: bool,
    required_approving_review_count: u32,
    required_review_thread_resolution: bool,
    #[serde(default)]
    required_reviewers: Vec<Value>,
    #[serde(flatten)]
    additional: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
struct GithubRulesetDismissalRestriction {
    enabled: bool,
}

#[derive(Deserialize)]
struct GithubClassicProtection {
    #[serde(default)]
    required_status_checks: Option<GithubClassicStatusChecks>,
    #[serde(default)]
    required_pull_request_reviews: Option<GithubClassicReviews>,
    #[serde(default)]
    required_conversation_resolution: Option<GithubEnabledSetting>,
}

#[derive(Deserialize)]
struct GithubEnabledSetting {
    enabled: bool,
}

#[derive(Deserialize)]
struct GithubClassicStatusChecks {
    strict: bool,
    contexts: Vec<String>,
    checks: Vec<GithubClassicCheck>,
}

#[derive(Deserialize)]
struct GithubClassicCheck {
    context: String,
    #[serde(default)]
    app_id: Option<i64>,
}

#[derive(Deserialize)]
struct GithubClassicReviews {
    #[serde(default)]
    dismiss_stale_reviews: bool,
    #[serde(default)]
    require_code_owner_reviews: bool,
    #[serde(default)]
    required_approving_review_count: u32,
    #[serde(default)]
    require_last_push_approval: bool,
    #[serde(default)]
    dismissal_restrictions: Option<GithubClassicDismissalRestrictions>,
}

#[derive(Deserialize)]
struct GithubClassicDismissalRestrictions {
    #[serde(default)]
    users: Vec<Value>,
    #[serde(default)]
    teams: Vec<Value>,
    #[serde(default)]
    apps: Vec<Value>,
}

#[derive(Deserialize)]
struct GithubCombinedStatusPage {
    sha: String,
    total_count: u64,
    statuses: Vec<GithubCommitStatus>,
}

#[derive(Deserialize)]
struct GithubCommitStatus {
    context: String,
    state: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct RequiredCheckIdentity {
    folded_name: String,
    app_id: Option<u64>,
}

#[derive(Debug, Default)]
struct Requirements {
    approvals: u32,
    branch_update: bool,
    checks: BTreeMap<RequiredCheckIdentity, String>,
}

#[async_trait]
impl AppliedSourceRequirementsProvider for GithubProvider {
    async fn applied_requirements(
        &self,
        repository: &Repository,
        target_branch: &str,
        commit_range: &CommitRange,
    ) -> Result<AppliedSourceRequirements> {
        validate_request(target_branch, commit_range)?;
        self.verify_branch_revision(repository, target_branch, &commit_range.base_sha)
            .await?;

        let rules = self.applied_branch_rules(repository, target_branch).await?;
        let classic = self.classic_protection(repository, target_branch).await?;
        let requirements = normalize_requirements(rules, classic)?;
        let runs = self
            .complete_github_check_runs(repository, &commit_range.head_sha)
            .await?
            .into_iter()
            .map(normalize_check_run)
            .collect::<Result<Vec<_>>>()?;
        ensure_run_revisions(&runs, &commit_range.head_sha)?;
        let statuses = self
            .combined_statuses(repository, &commit_range.head_sha)
            .await?;

        // The target branch can advance while the independent policy and
        // result collections are read. An answer for that mixed snapshot
        // would not describe the caller's exact base revision.
        self.verify_branch_revision(repository, target_branch, &commit_range.base_sha)
            .await?;

        let checks = requirements
            .checks
            .into_iter()
            .map(|(identity, name)| {
                let state = answer_requirement(&identity, &runs, &statuses)?;
                let app = identity
                    .app_id
                    .map(|id| ProviderAppId::new(id.to_string()))
                    .transpose()
                    .map_err(unrepresentable_model)?;
                AppliedRequiredCheck::new(name, app, state).map_err(unrepresentable_model)
            })
            .collect::<Result<Vec<_>>>()?;

        AppliedSourceRequirements::new(
            repository.clone(),
            target_branch,
            commit_range.clone(),
            requirements.approvals,
            if requirements.branch_update {
                BranchUpdateRequirement::Required
            } else {
                BranchUpdateRequirement::NotRequired
            },
            checks,
        )
        .map_err(unrepresentable_model)
    }
}

impl GithubProvider {
    async fn verify_branch_revision(
        &self,
        repository: &Repository,
        branch: &str,
        expected_sha: &str,
    ) -> Result<()> {
        let segment = utf8_percent_encode(branch, NON_ALPHANUMERIC);
        let observed: GithubBranch = self
            .user()?
            .get(
                format!("/repos/{repository}/branches/{segment}"),
                None::<&()>,
            )
            .await
            .map_err(|error| {
                read_error(
                    "read target branch revision",
                    format!("branch {branch} in {repository}"),
                    error,
                )
            })?;
        if observed.name != branch || observed.commit.sha != expected_sha {
            return Err(ProviderError::NotFound {
                entity: format!("branch {branch} at revision {expected_sha} in {repository}"),
            });
        }
        Ok(())
    }

    async fn applied_branch_rules(
        &self,
        repository: &Repository,
        branch: &str,
    ) -> Result<Vec<GithubAppliedRule>> {
        let segment = utf8_percent_encode(branch, NON_ALPHANUMERIC);
        let first: Page<GithubAppliedRule> = self
            .user()?
            .get(
                format!("/repos/{repository}/rules/branches/{segment}"),
                Some(&[("per_page", PAGE_SIZE.to_string())]),
            )
            .await
            .map_err(|error| {
                read_error(
                    "read applied branch rules",
                    format!("rules applied to branch {branch} in {repository}"),
                    error,
                )
            })?;
        self.user()?.all_pages(first).await.map_err(|error| {
            read_error(
                "read applied branch rules",
                format!("rules applied to branch {branch} in {repository}"),
                error,
            )
        })
    }

    async fn classic_protection(
        &self,
        repository: &Repository,
        branch: &str,
    ) -> Result<Option<GithubClassicProtection>> {
        let segment = utf8_percent_encode(branch, NON_ALPHANUMERIC);
        let response = self
            .user()?
            .get(
                format!("/repos/{repository}/branches/{segment}/protection"),
                None::<&()>,
            )
            .await;
        match response {
            Ok(protection) => Ok(Some(protection)),
            Err(error) if is_unprotected_branch(&error) => Ok(None),
            Err(error) if is_not_found(&error) => Err(authenticated_external(
                "read classic branch protection",
                &error,
            )),
            Err(error) => Err(read_error(
                "read classic branch protection",
                format!("classic protection for branch {branch} in {repository}"),
                error,
            )),
        }
    }

    async fn combined_statuses(
        &self,
        repository: &Repository,
        head_sha: &str,
    ) -> Result<Vec<GithubCommitStatus>> {
        let mut statuses = Vec::new();
        let mut page = 1_u32;
        let mut expected_total = None;
        loop {
            let response: GithubCombinedStatusPage = self
                .user()?
                .get(
                    format!("/repos/{repository}/commits/{head_sha}/status"),
                    Some(&[
                        ("per_page", PAGE_SIZE.to_string()),
                        ("page", page.to_string()),
                    ]),
                )
                .await
                .map_err(|error| {
                    read_error(
                        "read combined commit status",
                        format!("commit {head_sha} in {repository}"),
                        error,
                    )
                })?;
            if response.sha != head_sha {
                return Err(unrepresentable(format!(
                    "combined status names revision {} instead of {head_sha}",
                    response.sha
                )));
            }
            match expected_total {
                Some(total) if total != response.total_count => {
                    return Err(unrepresentable(format!(
                        "combined-status total changed from {total} to {} while paging",
                        response.total_count
                    )));
                }
                None => expected_total = Some(response.total_count),
                Some(_) => {}
            }
            let received = response.statuses.len();
            statuses.extend(response.statuses);
            let expected_total = expected_total.unwrap_or(response.total_count);
            if statuses.len() as u64 > expected_total {
                return Err(unrepresentable(format!(
                    "combined status reported {expected_total} records but returned at least {}",
                    statuses.len()
                )));
            }
            if statuses.len() as u64 >= expected_total {
                return Ok(statuses);
            }
            if received < PAGE_SIZE {
                return Err(unrepresentable(format!(
                    "combined status reported {} records but returned only {}",
                    expected_total,
                    statuses.len()
                )));
            }
            page += 1;
        }
    }
}

fn validate_request(branch: &str, range: &CommitRange) -> Result<()> {
    if branch.is_empty() {
        return Err(invalid_input("target branch must not be empty"));
    }
    for (value, field) in [(&range.base_sha, "base sha"), (&range.head_sha, "head sha")] {
        if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid_input(format!(
                "{field} must be a complete 40-character Git commit SHA"
            )));
        }
    }
    Ok(())
}

fn normalize_requirements(
    rules: Vec<GithubAppliedRule>,
    classic: Option<GithubClassicProtection>,
) -> Result<Requirements> {
    let mut result = Requirements::default();
    for rule in rules {
        match rule.rule_type.as_str() {
            "required_status_checks" => {
                let parameters: GithubStatusCheckParameters = parse_parameters(&rule)?;
                result.branch_update |= parameters.strict_required_status_checks_policy;
                for check in parameters.required_status_checks {
                    insert_check(&mut result.checks, check.context, check.integration_id)?;
                }
            }
            "pull_request" => {
                let parameters: GithubPullRequestParameters = parse_parameters(&rule)?;
                validate_ruleset_review_constraints(&parameters)?;
                result.approvals = result
                    .approvals
                    .max(parameters.required_approving_review_count);
            }
            _ => {}
        }
    }

    if let Some(classic) = classic {
        if let Some(checks) = classic.required_status_checks {
            result.branch_update |= checks.strict;
            let checked_contexts = checks
                .checks
                .iter()
                .map(|check| fold_context(&check.context))
                .collect::<Result<BTreeSet<_>>>()?;
            for check in checks.checks {
                insert_check(&mut result.checks, check.context, check.app_id)?;
            }
            for context in checks.contexts {
                if !checked_contexts.contains(&fold_context(&context)?) {
                    insert_check(&mut result.checks, context, None)?;
                }
            }
        }
        if let Some(reviews) = classic.required_pull_request_reviews {
            validate_classic_review_constraints(&reviews)?;
            result.approvals = result
                .approvals
                .max(reviews.required_approving_review_count);
        }
        if classic
            .required_conversation_resolution
            .is_some_and(|setting| setting.enabled)
        {
            return Err(unrepresentable(
                "classic protection requires review-thread resolution",
            ));
        }
    }
    Ok(result)
}

fn validate_ruleset_review_constraints(parameters: &GithubPullRequestParameters) -> Result<()> {
    let active = if !parameters.required_reviewers.is_empty() {
        Some("path-specific required reviewers")
    } else if parameters.dismiss_stale_reviews_on_push {
        Some("dismissal of stale reviews")
    } else if parameters
        .dismissal_restriction
        .as_ref()
        .is_some_and(|restriction| restriction.enabled)
    {
        Some("review-dismissal restrictions")
    } else if parameters.require_code_owner_review {
        Some("code-owner review")
    } else if parameters.require_last_push_approval {
        Some("approval by someone other than the last pusher")
    } else if parameters.required_review_thread_resolution {
        Some("review-thread resolution")
    } else {
        None
    };
    if let Some(constraint) = active {
        return Err(unrepresentable(format!(
            "an applied pull-request rule requires {constraint}, which cannot be represented as an approval count"
        )));
    }
    if !parameters.additional.is_empty() {
        return Err(unrepresentable(format!(
            "an applied pull-request rule contains unsupported parameters: {}",
            parameters
                .additional
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    Ok(())
}

fn validate_classic_review_constraints(reviews: &GithubClassicReviews) -> Result<()> {
    let restricted_dismissal = reviews
        .dismissal_restrictions
        .as_ref()
        .is_some_and(|value| {
            !value.users.is_empty() || !value.teams.is_empty() || !value.apps.is_empty()
        });
    let active = if reviews.dismiss_stale_reviews {
        Some("dismissal of stale reviews")
    } else if reviews.require_code_owner_reviews {
        Some("code-owner review")
    } else if reviews.require_last_push_approval {
        Some("approval by someone other than the last pusher")
    } else if restricted_dismissal {
        Some("review-dismissal restrictions")
    } else {
        None
    };
    if let Some(constraint) = active {
        return Err(unrepresentable(format!(
            "classic protection requires {constraint}, which cannot be represented as an approval count"
        )));
    }
    Ok(())
}

fn parse_parameters<T: for<'de> Deserialize<'de>>(rule: &GithubAppliedRule) -> Result<T> {
    let parameters = rule.parameters.clone().ok_or_else(|| {
        unrepresentable(format!("applied {} rule has no parameters", rule.rule_type))
    })?;
    serde_json::from_value(parameters).map_err(|error| {
        unrepresentable(format!(
            "applied {} rule parameters are unreadable: {error}",
            rule.rule_type
        ))
    })
}

fn insert_check(
    checks: &mut BTreeMap<RequiredCheckIdentity, String>,
    name: String,
    app_id: Option<i64>,
) -> Result<()> {
    if name.is_empty() {
        return Err(unrepresentable(
            "an applied required check has an empty context",
        ));
    }
    let folded_name = fold_context(&name)?;
    let app_id = match app_id {
        None | Some(-1) => None,
        Some(value) if value > 0 => Some(value as u64),
        Some(value) => {
            return Err(unrepresentable(format!(
                "required check {name} has invalid application id {value}"
            )));
        }
    };
    let identity = RequiredCheckIdentity {
        folded_name,
        app_id,
    };
    checks
        .entry(identity)
        .and_modify(|existing| {
            if name < *existing {
                existing.clone_from(&name);
            }
        })
        .or_insert(name);
    Ok(())
}

fn ensure_run_revisions(runs: &[CheckRun], expected: &str) -> Result<()> {
    if let Some(run) = runs.iter().find(|run| run.head_sha != expected) {
        return Err(unrepresentable(format!(
            "check run {} names revision {} instead of {expected}",
            run.name, run.head_sha
        )));
    }
    Ok(())
}

fn answer_requirement(
    identity: &RequiredCheckIdentity,
    runs: &[CheckRun],
    statuses: &[GithubCommitStatus],
) -> Result<AppliedRequiredCheckState> {
    let mut matching_runs = Vec::new();
    for run in runs {
        if fold_context(&run.name)? == identity.folded_name
            && identity.app_id.is_none_or(|required| {
                run.via_app
                    .as_ref()
                    .is_some_and(|app| app.id.as_str() == required.to_string())
            })
        {
            matching_runs.push(check_run_state(run));
        }
    }
    if identity.app_id.is_some() && matching_runs.is_empty() {
        return Ok(AppliedRequiredCheckState::Missing);
    }
    let mut matching_statuses = Vec::new();
    for status in statuses {
        if fold_context(&status.context)? == identity.folded_name {
            matching_statuses.push(commit_status_state(status)?);
        }
    }

    let mut answers = matching_runs;
    answers.extend(matching_statuses);
    Ok(combine_states(&answers))
}

fn fold_context(context: &str) -> Result<String> {
    if !context.is_ascii() {
        return Err(unrepresentable(format!(
            "check context {context:?} is non-ASCII; GitHub does not define a reproducible Unicode case-folding contract"
        )));
    }
    Ok(context.to_ascii_lowercase())
}

fn check_run_state(run: &CheckRun) -> AppliedRequiredCheckState {
    match &run.status {
        CheckStatus::Completed { conclusion, .. } => match conclusion {
            CheckConclusion::Success | CheckConclusion::Neutral | CheckConclusion::Skipped => {
                AppliedRequiredCheckState::Satisfied
            }
            CheckConclusion::Failure
            | CheckConclusion::Cancelled
            | CheckConclusion::TimedOut
            | CheckConclusion::ActionRequired
            | CheckConclusion::Stale => AppliedRequiredCheckState::Failed,
        },
        CheckStatus::Requested
        | CheckStatus::Queued
        | CheckStatus::Pending
        | CheckStatus::Waiting
        | CheckStatus::InProgress => AppliedRequiredCheckState::Pending,
    }
}

fn commit_status_state(status: &GithubCommitStatus) -> Result<AppliedRequiredCheckState> {
    match status.state.as_str() {
        "success" => Ok(AppliedRequiredCheckState::Satisfied),
        "pending" => Ok(AppliedRequiredCheckState::Pending),
        "failure" | "error" => Ok(AppliedRequiredCheckState::Failed),
        other => Err(unrepresentable(format!(
            "commit status {} has unknown state {other}",
            status.context
        ))),
    }
}

fn combine_states(states: &[AppliedRequiredCheckState]) -> AppliedRequiredCheckState {
    if states.contains(&AppliedRequiredCheckState::Failed) {
        AppliedRequiredCheckState::Failed
    } else if states.contains(&AppliedRequiredCheckState::Pending) {
        AppliedRequiredCheckState::Pending
    } else if states.contains(&AppliedRequiredCheckState::Satisfied) {
        AppliedRequiredCheckState::Satisfied
    } else {
        AppliedRequiredCheckState::Missing
    }
}

fn invalid_input(fact: impl Into<String>) -> ProviderError {
    ProviderError::InvalidInput {
        provider: "github",
        fact: fact.into(),
    }
}

fn is_unprotected_branch(error: &octocrab::Error) -> bool {
    matches!(
        error,
        octocrab::Error::GitHub { source, .. }
            if source.status_code.as_u16() == 404 && source.message == "Branch not protected"
    )
}

fn unrepresentable(fact: impl Into<String>) -> ProviderError {
    ProviderError::Unrepresentable {
        provider: "github",
        fact: fact.into(),
    }
}

fn unrepresentable_model(error: impl std::fmt::Display) -> ProviderError {
    unrepresentable(error.to_string())
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use interprex::{CheckStatus, ProviderApp};

    use super::*;

    fn run(name: &str, app: Option<&str>, status: CheckStatus) -> CheckRun {
        CheckRun {
            name: name.to_owned(),
            head_sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            via_app: app.map(|id| ProviderApp {
                id: ProviderAppId::new(id).expect("app id"),
                slug: "app".to_owned(),
                name: "App".to_owned(),
            }),
            status,
            summary: None,
            html_url: None,
        }
    }

    fn completed(conclusion: CheckConclusion) -> CheckStatus {
        CheckStatus::Completed {
            conclusion,
            completed_at: chrono::Utc.timestamp_opt(1, 0).single().expect("time"),
        }
    }

    fn complete_pull_request_parameters() -> Value {
        serde_json::json!({
            "allowed_merge_methods": ["merge", "squash", "rebase"],
            "dismiss_stale_reviews_on_push": false,
            "dismissal_restriction": {"enabled": false, "allowed_actors": []},
            "require_code_owner_review": false,
            "require_last_push_approval": false,
            "required_approving_review_count": 1,
            "required_review_thread_resolution": false,
            "required_reviewers": []
        })
    }

    #[test]
    fn native_answers_use_failure_precedence_and_all_success_conclusions() {
        for conclusion in [
            CheckConclusion::Success,
            CheckConclusion::Neutral,
            CheckConclusion::Skipped,
        ] {
            assert_eq!(
                check_run_state(&run("quality", None, completed(conclusion))),
                AppliedRequiredCheckState::Satisfied
            );
        }
        assert_eq!(
            combine_states(&[
                AppliedRequiredCheckState::Satisfied,
                AppliedRequiredCheckState::Pending,
                AppliedRequiredCheckState::Failed,
            ]),
            AppliedRequiredCheckState::Failed
        );
    }

    #[test]
    fn app_bound_requirement_needs_its_app_but_also_honors_an_existing_status() {
        let identity = RequiredCheckIdentity {
            folded_name: "quality".to_owned(),
            app_id: Some(42),
        };
        let wrong_app = vec![run(
            "QUALITY",
            Some("41"),
            completed(CheckConclusion::Success),
        )];
        let success_status = vec![GithubCommitStatus {
            context: "Quality".to_owned(),
            state: "success".to_owned(),
        }];
        assert_eq!(
            answer_requirement(&identity, &wrong_app, &success_status).expect("answer"),
            AppliedRequiredCheckState::Missing
        );

        let matching = vec![run(
            "QUALITY",
            Some("42"),
            completed(CheckConclusion::Success),
        )];
        let failed_status = vec![GithubCommitStatus {
            context: "quality".to_owned(),
            state: "failure".to_owned(),
        }];
        assert_eq!(
            answer_requirement(&identity, &matching, &failed_status).expect("answer"),
            AppliedRequiredCheckState::Failed
        );
    }

    #[test]
    fn requirement_union_is_order_independent_and_classic_checks_supersede_context_aliases() {
        let first = GithubAppliedRule {
            rule_type: "required_status_checks".to_owned(),
            parameters: Some(serde_json::json!({
                "strict_required_status_checks_policy": false,
                "required_status_checks": [{"context": "Zulu", "integration_id": null}]
            })),
        };
        let second = GithubAppliedRule {
            rule_type: "required_status_checks".to_owned(),
            parameters: Some(serde_json::json!({
                "strict_required_status_checks_policy": true,
                "required_status_checks": [{"context": "alpha", "integration_id": 42}]
            })),
        };
        let classic = || GithubClassicProtection {
            required_status_checks: Some(GithubClassicStatusChecks {
                strict: false,
                contexts: vec!["ALPHA".to_owned()],
                checks: vec![GithubClassicCheck {
                    context: "alpha".to_owned(),
                    app_id: Some(42),
                }],
            }),
            required_pull_request_reviews: None,
            required_conversation_resolution: None,
        };

        let left =
            normalize_requirements(vec![first, second], Some(classic())).expect("first order");
        let right = normalize_requirements(
            vec![
                GithubAppliedRule {
                    rule_type: "required_status_checks".to_owned(),
                    parameters: Some(serde_json::json!({
                        "strict_required_status_checks_policy": true,
                        "required_status_checks": [{"context": "alpha", "integration_id": 42}]
                    })),
                },
                GithubAppliedRule {
                    rule_type: "required_status_checks".to_owned(),
                    parameters: Some(serde_json::json!({
                        "strict_required_status_checks_policy": false,
                        "required_status_checks": [{"context": "Zulu", "integration_id": null}]
                    })),
                },
            ],
            Some(classic()),
        )
        .expect("second order");
        assert_eq!(left.checks, right.checks);
        assert!(left.branch_update);
        assert_eq!(
            left.checks.len(),
            2,
            "classic contexts must not duplicate checks"
        );
    }

    #[test]
    fn nested_path_specific_required_reviewers_are_not_flattened_into_an_approval_count() {
        let error = normalize_requirements(
            vec![GithubAppliedRule {
                rule_type: "pull_request".to_owned(),
                parameters: Some(serde_json::json!({
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
                })),
            }],
            None,
        )
        .expect_err("path-specific reviewers are not a scalar");
        assert!(matches!(
            error,
            ProviderError::Unrepresentable { fact, .. }
                if fact.contains("path-specific required reviewers")
        ));
    }

    #[test]
    fn every_non_scalar_ruleset_review_constraint_is_rejected() {
        for (field, value) in [
            ("dismiss_stale_reviews_on_push", serde_json::json!(true)),
            (
                "dismissal_restriction",
                serde_json::json!({"enabled": true, "allowed_actors": []}),
            ),
            ("require_code_owner_review", serde_json::json!(true)),
            ("require_last_push_approval", serde_json::json!(true)),
            ("required_review_thread_resolution", serde_json::json!(true)),
        ] {
            let mut parameters = serde_json::json!({
                "allowed_merge_methods": ["squash"],
                "dismiss_stale_reviews_on_push": false,
                "dismissal_restriction": {"enabled": false, "allowed_actors": []},
                "require_code_owner_review": false,
                "require_last_push_approval": false,
                "required_approving_review_count": 1,
                "required_review_thread_resolution": false,
                "required_reviewers": []
            });
            parameters[field] = value;
            let error = normalize_requirements(
                vec![GithubAppliedRule {
                    rule_type: "pull_request".to_owned(),
                    parameters: Some(parameters),
                }],
                None,
            )
            .expect_err("non-scalar review policy is not an approval count");
            assert!(
                matches!(error, ProviderError::Unrepresentable { .. }),
                "field {field}"
            );
        }
    }

    #[test]
    fn pull_request_rules_require_every_field_github_promises() {
        for missing in [
            "dismiss_stale_reviews_on_push",
            "require_code_owner_review",
            "require_last_push_approval",
            "required_approving_review_count",
            "required_review_thread_resolution",
        ] {
            let mut parameters = complete_pull_request_parameters();
            parameters
                .as_object_mut()
                .expect("parameters object")
                .remove(missing);
            let error = normalize_requirements(
                vec![GithubAppliedRule {
                    rule_type: "pull_request".to_owned(),
                    parameters: Some(parameters),
                }],
                None,
            )
            .expect_err("a promised field must not become a permissive default");
            assert!(
                matches!(error, ProviderError::Unrepresentable { .. }),
                "missing field {missing}"
            );
        }

        let mut parameters = complete_pull_request_parameters();
        parameters["dismissal_restriction"] = serde_json::json!({"allowed_actors": []});
        assert!(matches!(
            normalize_requirements(
                vec![GithubAppliedRule {
                    rule_type: "pull_request".to_owned(),
                    parameters: Some(parameters),
                }],
                None,
            ),
            Err(ProviderError::Unrepresentable { .. })
        ));
    }

    #[test]
    fn required_status_check_rules_do_not_default_required_fields() {
        for parameters in [
            serde_json::json!({"required_status_checks": []}),
            serde_json::json!({"strict_required_status_checks_policy": false}),
        ] {
            assert!(matches!(
                normalize_requirements(
                    vec![GithubAppliedRule {
                        rule_type: "required_status_checks".to_owned(),
                        parameters: Some(parameters),
                    }],
                    None,
                ),
                Err(ProviderError::Unrepresentable { .. })
            ));
        }
    }

    #[test]
    fn non_scalar_classic_review_constraints_are_not_flattened() {
        let error = normalize_requirements(
            Vec::new(),
            Some(GithubClassicProtection {
                required_status_checks: None,
                required_pull_request_reviews: Some(GithubClassicReviews {
                    dismiss_stale_reviews: false,
                    require_code_owner_reviews: true,
                    required_approving_review_count: 2,
                    require_last_push_approval: false,
                    dismissal_restrictions: Some(GithubClassicDismissalRestrictions {
                        users: Vec::new(),
                        teams: Vec::new(),
                        apps: Vec::new(),
                    }),
                }),
                required_conversation_resolution: None,
            }),
        )
        .expect_err("code-owner identity is not a scalar approval count");
        assert!(matches!(
            error,
            ProviderError::Unrepresentable { fact, .. }
                if fact.contains("code-owner")
        ));
    }

    #[test]
    fn context_matching_rejects_undefined_non_ascii_case_folding() {
        let rule_error = insert_check(&mut BTreeMap::new(), "CAFÉ".to_owned(), None)
            .expect_err("ruleset context must be reproducibly comparable");
        assert!(matches!(
            rule_error,
            ProviderError::Unrepresentable { fact, .. } if fact.contains("non-ASCII")
        ));

        let classic_error = normalize_requirements(
            Vec::new(),
            Some(GithubClassicProtection {
                required_status_checks: Some(GithubClassicStatusChecks {
                    strict: false,
                    contexts: vec!["CAFÉ".to_owned()],
                    checks: Vec::new(),
                }),
                required_pull_request_reviews: None,
                required_conversation_resolution: None,
            }),
        )
        .expect_err("classic aliases must be reproducibly comparable");
        assert!(matches!(
            classic_error,
            ProviderError::Unrepresentable { fact, .. } if fact.contains("non-ASCII")
        ));

        let identity = RequiredCheckIdentity {
            folded_name: "quality".to_owned(),
            app_id: None,
        };
        for answer in [
            answer_requirement(
                &identity,
                &[run("qualité", None, completed(CheckConclusion::Success))],
                &[],
            ),
            answer_requirement(
                &identity,
                &[],
                &[GithubCommitStatus {
                    context: "qualité".to_owned(),
                    state: "success".to_owned(),
                }],
            ),
        ] {
            assert!(matches!(
                answer,
                Err(ProviderError::Unrepresentable { fact, .. }) if fact.contains("non-ASCII")
            ));
        }
    }
}
