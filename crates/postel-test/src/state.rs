use std::{collections::BTreeMap, sync::Arc};

use bytes::Bytes;
use postel::{
    AssetId, CheckOutcome, DispatchInputs, Issue, IssueNumber, Label, ProviderError, PullRequest,
    PullRequestNumber, Release, Repository, RepositoryFacts, RepositorySettings, ReviewThread,
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
    pub(crate) pull_requests: BTreeMap<(Repository, PullRequestNumber), PullRequest>,
    pub(crate) threads: BTreeMap<(Repository, PullRequestNumber), Vec<ReviewThread>>,
    pub(crate) requested_reviewers: BTreeMap<(Repository, PullRequestNumber), Vec<String>>,
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

    pub async fn seed_pull_request(&self, repository: Repository, pull_request: PullRequest) {
        self.state
            .write()
            .await
            .pull_requests
            .insert((repository, pull_request.number), pull_request);
    }

    pub async fn seed_review_threads(
        &self,
        repository: Repository,
        number: PullRequestNumber,
        threads: Vec<ReviewThread>,
    ) {
        self.state
            .write()
            .await
            .threads
            .insert((repository, number), threads);
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

    pub async fn requested_reviewers(
        &self,
        repository: &Repository,
        number: PullRequestNumber,
    ) -> Vec<String> {
        self.state
            .read()
            .await
            .requested_reviewers
            .get(&(repository.clone(), number))
            .cloned()
            .unwrap_or_default()
    }
}

pub(crate) fn missing(entity: impl Into<String>) -> ProviderError {
    ProviderError::NotFound {
        entity: entity.into(),
    }
}
