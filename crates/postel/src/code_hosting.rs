use std::{fmt, str::FromStr};

use async_trait::async_trait;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};

use crate::{ModelError, Result, error::segment};

#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Repository {
    owner: String,
    name: String,
}

impl Repository {
    pub fn new(
        owner: impl Into<String>,
        name: impl Into<String>,
    ) -> std::result::Result<Self, ModelError> {
        Ok(Self {
            owner: segment(owner, "owner")?,
            name: segment(name, "repository name")?,
        })
    }

    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for Repository {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}/{}", self.owner, self.name)
    }
}

impl FromStr for Repository {
    type Err = ModelError;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        let (owner, name) = value.split_once('/').ok_or(ModelError::InvalidRepository)?;
        if name.contains('/') {
            return Err(ModelError::InvalidRepository);
        }
        Self::new(owner, name)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RepositoryFacts {
    pub repository: Repository,
    pub default_branch: String,
    pub private: bool,
    pub archived: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RepositorySettings {
    pub allow_squash_merge: bool,
    pub allow_merge_commit: bool,
    pub allow_rebase_merge: bool,
    pub delete_branch_on_merge: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RequiredCheck {
    pub context: String,
    pub integration_id: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Ruleset {
    pub id: Option<u64>,
    pub name: String,
    pub active: bool,
    pub target_branch_patterns: Vec<String>,
    pub required_checks: Vec<RequiredCheck>,
    pub required_approvals: u8,
}

#[async_trait]
pub trait CodeHostingProvider: Send + Sync {
    async fn repository(&self, repository: &Repository) -> Result<RepositoryFacts>;
    async fn settings(&self, repository: &Repository) -> Result<RepositorySettings>;
    async fn apply_settings(
        &self,
        repository: &Repository,
        settings: &RepositorySettings,
    ) -> Result<RepositorySettings>;
    async fn rulesets(&self, repository: &Repository) -> Result<Vec<Ruleset>>;
    async fn upsert_ruleset(&self, repository: &Repository, ruleset: &Ruleset) -> Result<Ruleset>;
    async fn put_secret(
        &self,
        repository: &Repository,
        name: &str,
        value: SecretString,
    ) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::Repository;
    use crate::ModelError;

    #[test]
    fn repository_round_trips_through_its_canonical_address() {
        let repository: Repository = "faictor/postel".parse().expect("valid repository");
        assert_eq!(repository.to_string(), "faictor/postel");
    }

    #[test]
    fn repository_rejects_extra_path_segments() {
        assert_eq!(
            "faictor/postel/extra".parse::<Repository>(),
            Err(ModelError::InvalidRepository)
        );
    }
}
