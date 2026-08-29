//! Source-code ruleset configuration and exact-revision applied requirements.

use std::{collections::HashSet, fmt::Debug};

use async_trait::async_trait;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::{CommitRange, ModelError, ProviderAppId, Repository, Result};

/// Whether the applied source configuration requires the change-request head
/// to contain the current target-branch tip.
///
/// This fact is independent of branch freshness and mergeability. Callers
/// combine it with observations from the code-review domain when deciding
/// whether to request a branch update.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BranchUpdateRequirement {
    Required,
    NotRequired,
}

/// The provider's answer for one native required-check requirement.
///
/// The provider matches its native requirement against native check runs,
/// commit statuses, or equivalent records. Consumers interpret this answer as
/// policy; they do not repeat provider-specific matching.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AppliedRequiredCheckState {
    /// No provider record answers the requirement at the observed head.
    Missing,
    /// A provider record answers the requirement but has not finished.
    Pending,
    /// The provider reports that the requirement is satisfied.
    Satisfied,
    /// The provider reports that the requirement completed unsuccessfully.
    Failed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AppliedRequiredCheckWire {
    name: String,
    provider_application: Option<ProviderAppId>,
    state: AppliedRequiredCheckState,
}

/// One provider requirement and its answer at the observed head revision.
///
/// `name` and `provider_application` together identify the native
/// requirement. The application identifier is opaque: consumers compare it
/// for equality but do not parse it as a GitHub integer or infer an
/// application from a mutable name. A missing application means the native
/// requirement accepts an answer without selecting one application.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    try_from = "AppliedRequiredCheckWire",
    into = "AppliedRequiredCheckWire"
)]
pub struct AppliedRequiredCheck {
    name: String,
    provider_application: Option<ProviderAppId>,
    state: AppliedRequiredCheckState,
}

impl AppliedRequiredCheck {
    pub fn new(
        name: impl Into<String>,
        provider_application: Option<ProviderAppId>,
        state: AppliedRequiredCheckState,
    ) -> std::result::Result<Self, ModelError> {
        let name = non_empty(name, "required check name")?;
        Ok(Self {
            name,
            provider_application,
            state,
        })
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub const fn provider_application(&self) -> Option<&ProviderAppId> {
        self.provider_application.as_ref()
    }

    #[must_use]
    pub const fn state(&self) -> AppliedRequiredCheckState {
        self.state
    }
}

impl TryFrom<AppliedRequiredCheckWire> for AppliedRequiredCheck {
    type Error = ModelError;

    fn try_from(value: AppliedRequiredCheckWire) -> std::result::Result<Self, Self::Error> {
        Self::new(value.name, value.provider_application, value.state)
    }
}

impl From<AppliedRequiredCheck> for AppliedRequiredCheckWire {
    fn from(value: AppliedRequiredCheck) -> Self {
        Self {
            name: value.name,
            provider_application: value.provider_application,
            state: value.state,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct AppliedSourceRequirementsWire {
    repository: Repository,
    target_branch: String,
    commit_range: CommitRange,
    required_approvals: u32,
    branch_update: BranchUpdateRequirement,
    required_checks: Vec<AppliedRequiredCheck>,
}

/// Requirements applied to one exact source-code subject and their answers.
///
/// The subject is the target repository and branch at exactly the stated base
/// and head revisions. A provider must not substitute a newer branch tip or
/// head revision. `required_approvals` is the strongest applicable minimum;
/// `required_checks` contains exactly one answer for every applicable native
/// check requirement, in stable provider requirement order. No two entries
/// have the same name and provider-application identity.
///
/// A later read may return a different subject or different answers. This
/// value is evidence about the revisions it names, not a subscription to
/// branch state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    try_from = "AppliedSourceRequirementsWire",
    into = "AppliedSourceRequirementsWire"
)]
pub struct AppliedSourceRequirements {
    repository: Repository,
    target_branch: String,
    commit_range: CommitRange,
    required_approvals: u32,
    branch_update: BranchUpdateRequirement,
    required_checks: Vec<AppliedRequiredCheck>,
}

impl AppliedSourceRequirements {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        repository: Repository,
        target_branch: impl Into<String>,
        commit_range: CommitRange,
        required_approvals: u32,
        branch_update: BranchUpdateRequirement,
        required_checks: Vec<AppliedRequiredCheck>,
    ) -> std::result::Result<Self, ModelError> {
        let target_branch = non_empty(target_branch, "target branch")?;
        non_empty(&commit_range.base_sha, "base sha")?;
        non_empty(&commit_range.head_sha, "head sha")?;

        let mut identities = HashSet::with_capacity(required_checks.len());
        for check in &required_checks {
            let identity = (check.name.clone(), check.provider_application.clone());
            if !identities.insert(identity) {
                return Err(ModelError::DuplicateRequiredCheck {
                    name: check.name.clone(),
                });
            }
        }

        Ok(Self {
            repository,
            target_branch,
            commit_range,
            required_approvals,
            branch_update,
            required_checks,
        })
    }

    #[must_use]
    pub const fn repository(&self) -> &Repository {
        &self.repository
    }

    #[must_use]
    pub fn target_branch(&self) -> &str {
        &self.target_branch
    }

    #[must_use]
    pub const fn commit_range(&self) -> &CommitRange {
        &self.commit_range
    }

    #[must_use]
    pub const fn required_approvals(&self) -> u32 {
        self.required_approvals
    }

    #[must_use]
    pub const fn branch_update(&self) -> BranchUpdateRequirement {
        self.branch_update
    }

    #[must_use]
    pub fn required_checks(&self) -> &[AppliedRequiredCheck] {
        &self.required_checks
    }
}

impl TryFrom<AppliedSourceRequirementsWire> for AppliedSourceRequirements {
    type Error = ModelError;

    fn try_from(value: AppliedSourceRequirementsWire) -> std::result::Result<Self, Self::Error> {
        Self::new(
            value.repository,
            value.target_branch,
            value.commit_range,
            value.required_approvals,
            value.branch_update,
            value.required_checks,
        )
    }
}

impl From<AppliedSourceRequirements> for AppliedSourceRequirementsWire {
    fn from(value: AppliedSourceRequirements) -> Self {
        Self {
            repository: value.repository,
            target_branch: value.target_branch,
            commit_range: value.commit_range,
            required_approvals: value.required_approvals,
            branch_update: value.branch_update,
            required_checks: value.required_checks,
        }
    }
}

/// Provider capability for reading and applying complete native rulesets.
///
/// The associated type preserves the provider's complete source-code
/// configuration, including provider-specific rule parameters and unknown
/// fields. A configuration tool that uses this trait chooses a concrete
/// provider and its `Ruleset` type; policy consumers use
/// [`AppliedSourceRequirementsProvider`] instead.
#[async_trait]
pub trait SourceCodeConfigurationProvider: Send + Sync {
    type Ruleset: Clone + Debug + DeserializeOwned + Serialize + Send + Sync + 'static;

    /// Reads every ruleset visible for `repository` in stable provider order.
    ///
    /// The provider completely paginates the collection and retains fields it
    /// cannot interpret. It returns an explicit
    /// [`crate::ProviderError::Unsupported`] error when it cannot provide the
    /// complete native configuration.
    ///
    /// # Errors
    ///
    /// Returns [`crate::ProviderError::NotFound`] when the repository is
    /// absent, [`crate::ProviderError::MissingCredential`] when the operation
    /// lacks credentials, [`crate::ProviderError::Unrepresentable`] when a
    /// response cannot be retained without loss,
    /// [`crate::ProviderError::Unsupported`] when this provider has no
    /// complete ruleset implementation, and
    /// [`crate::ProviderError::External`] for provider read failures.
    async fn read_rulesets(&self, repository: &Repository) -> Result<Vec<Self::Ruleset>>;

    /// Creates or replaces the native ruleset identified by `ruleset`.
    ///
    /// The provider-specific value carries the identity and complete desired
    /// configuration. The returned value is the provider's complete accepted
    /// representation. The operation never fills omitted fields with
    /// Interprex defaults.
    ///
    /// # Errors
    ///
    /// Returns [`crate::ProviderError::InvalidInput`] when `ruleset` is not a
    /// complete writable configuration, [`crate::ProviderError::NotFound`]
    /// when its repository or existing native identity is absent,
    /// [`crate::ProviderError::MissingCredential`] when the operation lacks
    /// credentials, [`crate::ProviderError::Unrepresentable`] when accepted
    /// provider data cannot be read or verified without loss,
    /// [`crate::ProviderError::Unsupported`] when this provider has no complete
    /// ruleset implementation, and [`crate::ProviderError::External`] for
    /// provider refusal or transport failure.
    async fn apply_ruleset(
        &self,
        repository: &Repository,
        ruleset: &Self::Ruleset,
    ) -> Result<Self::Ruleset>;
}

/// Provider capability for reading provider-neutral requirements already
/// applied to one exact source-code subject.
///
/// The trait has no associated types and is object-safe. Callers can therefore
/// select a provider at runtime while receiving only the facts needed for
/// policy. Native ruleset configuration remains behind
/// [`SourceCodeConfigurationProvider`].
#[async_trait]
pub trait AppliedSourceRequirementsProvider: Send + Sync {
    /// Reads the requirements applied to the exact requested revisions.
    ///
    /// The returned value repeats `repository`, `target_branch`, and
    /// `commit_range`. The provider must return those exact values or an error;
    /// it must not answer for a newer branch tip or head. Required checks are
    /// completely matched against native records and retain stable native
    /// requirement order.
    ///
    /// # Errors
    ///
    /// Returns [`crate::ProviderError::InvalidInput`] for an empty branch or
    /// revision, [`crate::ProviderError::NotFound`] when the repository,
    /// branch, or either revision is absent,
    /// [`crate::ProviderError::MissingCredential`] when the operation lacks
    /// credentials, [`crate::ProviderError::Unrepresentable`] when an
    /// applicable native requirement or answer cannot be represented,
    /// [`crate::ProviderError::Unsupported`] when this provider has no applied
    /// requirements implementation, and [`crate::ProviderError::External`]
    /// for provider read failures.
    async fn applied_requirements(
        &self,
        repository: &Repository,
        target_branch: &str,
        commit_range: &CommitRange,
    ) -> Result<AppliedSourceRequirements>;
}

fn non_empty(
    value: impl Into<String>,
    field: &'static str,
) -> std::result::Result<String, ModelError> {
    let value = value.into();
    if value.is_empty() {
        Err(ModelError::Empty { field })
    } else {
        Ok(value)
    }
}
