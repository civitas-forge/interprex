//! Repository settings and secret transport owned by code hosting.
//!
//! Secret values are sealed with the repository public key before the write
//! and are never retained by the provider.

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use crypto_box::{PublicKey, aead::OsRng};
use interprex::{
    CodeHostingProvider, ProviderError, Repository, RepositoryFacts, RepositorySettings, Result,
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
            ProviderError::Unrepresentable {
                provider: "github",
                fact: error.to_string(),
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

#[async_trait]
impl CodeHostingProvider for GithubProvider {
    #[tracing::instrument(
        name = "interprex.provider.code_hosting.repository",
        skip_all,
        fields(interprex.provider.name = "github")
    )]
    async fn repository(&self, repository: &Repository) -> Result<RepositoryFacts> {
        let response: GithubRepository = self
            .user()?
            .get(format!("/repos/{repository}"), None::<&()>)
            .await
            .map_err(|error| read_error("read repository", repository.to_string(), error))?;
        response.try_into().map(|(facts, _)| facts)
    }

    #[tracing::instrument(
        name = "interprex.provider.code_hosting.settings",
        skip_all,
        fields(interprex.provider.name = "github")
    )]
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

    #[tracing::instrument(
        name = "interprex.provider.code_hosting.apply_settings",
        skip_all,
        fields(interprex.provider.name = "github")
    )]
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

    #[tracing::instrument(
        name = "interprex.provider.code_hosting.put_secret",
        skip_all,
        fields(interprex.provider.name = "github")
    )]
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
    use super::GithubRepository;

    #[test]
    fn repository_fixture_normalizes_only_contract_facts() {
        let fixture = include_str!("../tests/fixtures/repository.json");
        let response: GithubRepository = serde_json::from_str(fixture).expect("fixture");
        let (facts, settings) = response.try_into().expect("normalizes");
        assert_eq!(
            facts.repository.to_string(),
            "civitas-forge/interprex-sandbox"
        );
        assert!(settings.allow_squash_merge);
        assert!(!settings.allow_merge_commit);
    }
}
