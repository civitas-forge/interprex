use std::{collections::BTreeMap, sync::Arc};

use bytes::Bytes;
use interprex::{
    AssetId, ChangeRequest, ChangeRequestNumber, CheckOutcome, CheckRun, DispatchInputs, Issue,
    IssueNumber, Label, ProviderError, Release, Repository, RepositoryFacts, RepositorySettings,
    Ruleset, RunId, WorkflowRun,
};
use tokio::sync::RwLock;

#[derive(Clone, Debug, Default)]
pub struct FakeProvider {
    pub(crate) state: Arc<RwLock<State>>,
}

#[derive(Debug, Default)]
pub(crate) struct State {
    pub(crate) repositories: BTreeMap<Repository, (RepositoryFacts, RepositorySettings)>,
    pub(crate) rulesets: BTreeMap<Repository, Vec<Ruleset>>,
    pub(crate) secret_names: BTreeMap<Repository, Vec<String>>,
    pub(crate) issues: BTreeMap<(Repository, IssueNumber), Issue>,
    pub(crate) labels: BTreeMap<Repository, Vec<Label>>,
    pub(crate) change_requests: BTreeMap<(Repository, ChangeRequestNumber), ChangeRequest>,
    pub(crate) check_runs: BTreeMap<(Repository, String), Vec<CheckRun>>,
    pub(crate) published_checks: Vec<(Repository, String, CheckOutcome)>,
    pub(crate) dispatches: Vec<(Repository, String, String, DispatchInputs)>,
    pub(crate) runs: BTreeMap<(Repository, RunId), WorkflowRun>,
    pub(crate) cancelled_runs: Vec<(Repository, RunId)>,
    pub(crate) releases: BTreeMap<(Repository, String), Release>,
    pub(crate) assets: BTreeMap<(Repository, AssetId), Vec<Bytes>>,
    pub(crate) next_release_id: u64,
    pub(crate) next_asset_id: u64,
}

impl FakeProvider {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn seed_repository(&self, facts: RepositoryFacts, settings: RepositorySettings) {
        self.state
            .write()
            .await
            .repositories
            .insert(facts.repository.clone(), (facts, settings));
    }

    pub async fn seed_issue(&self, repository: Repository, issue: Issue) {
        self.state
            .write()
            .await
            .issues
            .insert((repository, issue.number), issue);
    }

    /// Seeds one change request into the repository it targets.
    ///
    /// The head it proposes is `change_request.head`, which names the
    /// repository holding that branch and so can be a fork of `repository`.
    pub async fn seed_change_request(&self, repository: Repository, change_request: ChangeRequest) {
        self.state
            .write()
            .await
            .change_requests
            .insert((repository, change_request.number), change_request);
    }

    /// Seeds the checks observed on one commit, replacing any already seeded
    /// for that commit.
    pub async fn seed_check_runs(
        &self,
        repository: Repository,
        head_sha: impl Into<String>,
        runs: Vec<CheckRun>,
    ) {
        self.state
            .write()
            .await
            .check_runs
            .insert((repository, head_sha.into()), runs);
    }

    pub async fn seed_run(&self, repository: Repository, run: WorkflowRun) {
        self.state
            .write()
            .await
            .runs
            .insert((repository, run.id), run);
    }

    pub async fn seed_release(&self, repository: Repository, release: Release) {
        self.state
            .write()
            .await
            .releases
            .insert((repository, release.tag.clone()), release);
    }

    pub async fn published_checks(&self) -> Vec<(Repository, String, CheckOutcome)> {
        self.state.read().await.published_checks.clone()
    }

    pub async fn dispatches(&self) -> Vec<(Repository, String, String, DispatchInputs)> {
        self.state.read().await.dispatches.clone()
    }
}

pub(crate) fn missing(entity: impl Into<String>) -> ProviderError {
    ProviderError::NotFound {
        entity: entity.into(),
    }
}
