use async_trait::async_trait;
use interprex::{
    AppliedSourceRequirements, AppliedSourceRequirementsProvider, CodeHostingProvider, CommitRange,
    ProviderError, Repository, RepositoryFacts, RepositorySettings, Result,
    SourceCodeConfigurationProvider,
};
use secrecy::SecretString;

use crate::state::{FakeProvider, missing};

#[async_trait]
impl CodeHostingProvider for FakeProvider {
    async fn repository(&self, repository: &Repository) -> Result<RepositoryFacts> {
        self.state
            .read()
            .await
            .repositories
            .get(repository)
            .map(|(facts, _)| facts.clone())
            .ok_or_else(|| missing(repository.to_string()))
    }

    async fn settings(&self, repository: &Repository) -> Result<RepositorySettings> {
        self.state
            .read()
            .await
            .repositories
            .get(repository)
            .map(|(_, settings)| settings.clone())
            .ok_or_else(|| missing(repository.to_string()))
    }

    async fn apply_settings(
        &self,
        repository: &Repository,
        settings: &RepositorySettings,
    ) -> Result<RepositorySettings> {
        let mut state = self.state.write().await;
        let (_, current) = state
            .repositories
            .get_mut(repository)
            .ok_or_else(|| missing(repository.to_string()))?;
        *current = settings.clone();
        Ok(settings.clone())
    }

    async fn put_secret(
        &self,
        repository: &Repository,
        name: &str,
        _value: SecretString,
    ) -> Result<()> {
        self.state
            .write()
            .await
            .secret_names
            .entry(repository.clone())
            .or_default()
            .push(name.to_owned());
        Ok(())
    }
}

#[async_trait]
impl SourceCodeConfigurationProvider for FakeProvider {
    type Ruleset = serde_json::Value;

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
impl AppliedSourceRequirementsProvider for FakeProvider {
    async fn applied_requirements(
        &self,
        repository: &Repository,
        target_branch: &str,
        commit_range: &CommitRange,
    ) -> Result<AppliedSourceRequirements> {
        for (value, field) in [
            (target_branch, "target branch"),
            (&commit_range.base_sha, "base sha"),
            (&commit_range.head_sha, "head sha"),
        ] {
            if value.is_empty() {
                return Err(ProviderError::InvalidInput {
                    provider: "fake",
                    fact: format!("{field} must not be empty"),
                });
            }
        }
        let key = (
            repository.clone(),
            target_branch.to_owned(),
            commit_range.base_sha.clone(),
            commit_range.head_sha.clone(),
        );
        let state = self.state.read().await;
        if let Some(error) = state.applied_requirement_errors.get(&key) {
            return Err(error.clone());
        }
        state
            .applied_requirements
            .get(&key)
            .cloned()
            .ok_or_else(|| ProviderError::NotFound {
                entity: format!(
                    "applied requirements for {repository}:{target_branch} at {}..{}",
                    commit_range.base_sha, commit_range.head_sha
                ),
            })
    }
}

fn unsupported(operation: &'static str) -> ProviderError {
    ProviderError::Unsupported {
        provider: "fake",
        operation,
    }
}
