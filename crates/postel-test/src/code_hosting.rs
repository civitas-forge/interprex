use async_trait::async_trait;
use postel::{
    CodeHostingProvider, Repository, RepositoryFacts, RepositorySettings, Result, Ruleset,
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

    async fn rulesets(&self, repository: &Repository) -> Result<Vec<Ruleset>> {
        Ok(self
            .state
            .read()
            .await
            .rulesets
            .get(repository)
            .cloned()
            .unwrap_or_default())
    }

    async fn upsert_ruleset(&self, repository: &Repository, ruleset: &Ruleset) -> Result<Ruleset> {
        let mut state = self.state.write().await;
        let rulesets = state.rulesets.entry(repository.clone()).or_default();
        if let Some(existing) = rulesets.iter_mut().find(|existing| {
            ruleset.id.is_some() && existing.id == ruleset.id || existing.name == ruleset.name
        }) {
            *existing = ruleset.clone();
        } else {
            rulesets.push(ruleset.clone());
        }
        Ok(ruleset.clone())
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
