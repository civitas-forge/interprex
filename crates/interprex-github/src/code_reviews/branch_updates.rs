use async_trait::async_trait;
use interprex::{
    BranchFreshness, BranchUpdateError, BranchUpdateObservation, BranchUpdatesProvider,
    ChangeRequestNumber, CommitRange, ProviderError, Repository, Result,
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    GithubProvider,
    client::{authenticated_external, read_error},
};

#[derive(Deserialize)]
struct GithubComparison {
    status: String,
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
