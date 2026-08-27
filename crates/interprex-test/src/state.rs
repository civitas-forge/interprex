use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use bytes::Bytes;
use interprex::{
    AssetId, ChangeRequest, ChangeRequestNumber, CheckOutcome, CheckRun, DispatchInputs, Issue,
    IssueNumber, Label, ProviderError, Release, Repository, RepositoryFacts, RepositorySettings,
    ReviewRequestTarget, ReviewTarget, Ruleset, RunId, WorkflowRun,
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
    pub(crate) review_target_observations: Vec<(Repository, ReviewRequestTarget, ReviewTarget)>,
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

    /// Seeds the provider observation returned for one review-request target.
    ///
    /// The target category is part of the lookup key while `observed` supplies
    /// the actual actor or team category. Keeping them separate lets tests
    /// model a request target that resolves to the wrong kind without the fake
    /// inferring identity facts from the requested enum variant.
    pub async fn seed_review_request_target(
        &self,
        repository: Repository,
        target: ReviewRequestTarget,
        observed: ReviewTarget,
    ) {
        let mut state = self.state.write().await;
        state
            .review_target_observations
            .retain(|(seeded_repository, seeded_target, _)| {
                seeded_repository != &repository || seeded_target != &target
            });
        state
            .review_target_observations
            .push((repository, target, observed));
    }

    /// Seeds observed checks, each on the commit it names, replacing whatever
    /// was already seeded on the commits this call names.
    ///
    /// The commit comes from every run's own `head_sha`, so no seeded
    /// observation can place a run on a commit it does not name.
    pub async fn seed_check_runs(&self, repository: Repository, runs: Vec<CheckRun>) {
        let mut state = self.state.write().await;
        let mut replaced = BTreeSet::new();
        for run in runs {
            let key = (repository.clone(), run.head_sha.clone());
            if replaced.insert(key.clone()) {
                state.check_runs.insert(key.clone(), Vec::new());
            }
            state.check_runs.entry(key).or_default().push(run);
        }
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
