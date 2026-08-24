//! Stateful in-memory adapter for tests in Postel consumers.
//!
//! The fake implements the same five interfaces as a remote provider and keeps
//! observable outcomes, not request expectations. Consumer tests can exercise
//! their rules through the contract and then read the resulting domain state;
//! they do not need to mock Postel's internal calls.

#![forbid(unsafe_code)]

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use bytes::Bytes;
use postel_contracts::{
    JobsDomain, PrDomain, ProviderError, ReleasesDomain, RepoDomain, Result, TrackerDomain,
};
use postel_model::{
    AssetId, CheckOutcome, DispatchInputs, Issue, IssueNumber, Label, NewRelease, PullRequest,
    PullRequestNumber, Release, ReleaseAsset, ReleaseId, Repository, RepositoryFacts,
    RepositorySettings, ReviewThread, Ruleset, RunId, WorkflowRun,
};
use secrecy::SecretString;
use tokio::sync::RwLock;

#[derive(Clone, Debug, Default)]
pub struct FakeProvider {
    state: Arc<RwLock<State>>,
}

#[derive(Debug, Default)]
struct State {
    repositories: BTreeMap<Repository, (RepositoryFacts, RepositorySettings)>,
    rulesets: BTreeMap<Repository, Vec<Ruleset>>,
    secret_names: BTreeMap<Repository, Vec<String>>,
    issues: BTreeMap<(Repository, IssueNumber), Issue>,
    labels: BTreeMap<Repository, Vec<Label>>,
    pull_requests: BTreeMap<(Repository, PullRequestNumber), PullRequest>,
    threads: BTreeMap<(Repository, PullRequestNumber), Vec<ReviewThread>>,
    requested_reviewers: BTreeMap<(Repository, PullRequestNumber), Vec<String>>,
    ready_pull_requests: Vec<String>,
    published_checks: Vec<(Repository, String, CheckOutcome)>,
    dispatches: Vec<(Repository, String, String, DispatchInputs)>,
    runs: BTreeMap<(Repository, RunId), WorkflowRun>,
    cancelled_runs: Vec<(Repository, RunId)>,
    releases: BTreeMap<(Repository, String), Release>,
    assets: BTreeMap<(Repository, AssetId), Bytes>,
    next_release_id: u64,
    next_asset_id: u64,
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

fn missing(entity: impl Into<String>) -> ProviderError {
    ProviderError::NotFound {
        entity: entity.into(),
    }
}

#[async_trait]
impl RepoDomain for FakeProvider {
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

#[async_trait]
impl TrackerDomain for FakeProvider {
    async fn issue(&self, repository: &Repository, number: IssueNumber) -> Result<Issue> {
        self.state
            .read()
            .await
            .issues
            .get(&(repository.clone(), number))
            .cloned()
            .ok_or_else(|| missing(format!("issue {number:?} in {repository}")))
    }

    async fn labels(&self, repository: &Repository) -> Result<Vec<Label>> {
        Ok(self
            .state
            .read()
            .await
            .labels
            .get(repository)
            .cloned()
            .unwrap_or_default())
    }

    async fn upsert_label(&self, repository: &Repository, label: &Label) -> Result<Label> {
        let mut state = self.state.write().await;
        let labels = state.labels.entry(repository.clone()).or_default();
        if let Some(existing) = labels
            .iter_mut()
            .find(|existing| existing.name == label.name)
        {
            *existing = label.clone();
        } else {
            labels.push(label.clone());
        }
        Ok(label.clone())
    }
}

#[async_trait]
impl PrDomain for FakeProvider {
    async fn pull_request(
        &self,
        repository: &Repository,
        number: PullRequestNumber,
    ) -> Result<PullRequest> {
        self.state
            .read()
            .await
            .pull_requests
            .get(&(repository.clone(), number))
            .cloned()
            .ok_or_else(|| missing(format!("pull request {number:?} in {repository}")))
    }

    async fn review_threads(
        &self,
        repository: &Repository,
        number: PullRequestNumber,
    ) -> Result<Vec<ReviewThread>> {
        Ok(self
            .state
            .read()
            .await
            .threads
            .get(&(repository.clone(), number))
            .cloned()
            .unwrap_or_default())
    }

    async fn resolve_thread(&self, thread_id: &str) -> Result<()> {
        let mut state = self.state.write().await;
        for threads in state.threads.values_mut() {
            if let Some(thread) = threads.iter_mut().find(|thread| thread.id == thread_id) {
                thread.resolved = true;
                return Ok(());
            }
        }
        Err(missing(format!("review thread {thread_id}")))
    }

    async fn request_reviewers(
        &self,
        repository: &Repository,
        number: PullRequestNumber,
        reviewers: &[String],
    ) -> Result<()> {
        self.state
            .write()
            .await
            .requested_reviewers
            .insert((repository.clone(), number), reviewers.to_vec());
        Ok(())
    }

    async fn mark_ready(&self, pull_request_node_id: &str) -> Result<()> {
        self.state
            .write()
            .await
            .ready_pull_requests
            .push(pull_request_node_id.to_owned());
        Ok(())
    }

    async fn publish_check(
        &self,
        repository: &Repository,
        app_identity: &str,
        outcome: &CheckOutcome,
    ) -> Result<()> {
        self.state.write().await.published_checks.push((
            repository.clone(),
            app_identity.to_owned(),
            outcome.clone(),
        ));
        Ok(())
    }
}

#[async_trait]
impl JobsDomain for FakeProvider {
    async fn dispatch(
        &self,
        repository: &Repository,
        workflow: &str,
        git_ref: &str,
        inputs: &DispatchInputs,
    ) -> Result<()> {
        self.state.write().await.dispatches.push((
            repository.clone(),
            workflow.to_owned(),
            git_ref.to_owned(),
            inputs.clone(),
        ));
        Ok(())
    }

    async fn run(&self, repository: &Repository, run_id: RunId) -> Result<WorkflowRun> {
        self.state
            .read()
            .await
            .runs
            .get(&(repository.clone(), run_id))
            .cloned()
            .ok_or_else(|| missing(format!("workflow run {run_id:?} in {repository}")))
    }

    async fn cancel_run(&self, repository: &Repository, run_id: RunId) -> Result<()> {
        self.state
            .write()
            .await
            .cancelled_runs
            .push((repository.clone(), run_id));
        Ok(())
    }
}

#[async_trait]
impl ReleasesDomain for FakeProvider {
    async fn release_by_tag(&self, repository: &Repository, tag: &str) -> Result<Release> {
        self.state
            .read()
            .await
            .releases
            .get(&(repository.clone(), tag.to_owned()))
            .cloned()
            .ok_or_else(|| missing(format!("release {tag} in {repository}")))
    }

    async fn create_release(
        &self,
        repository: &Repository,
        release: &NewRelease,
    ) -> Result<Release> {
        let mut state = self.state.write().await;
        state.next_release_id += 1;
        let created = Release {
            id: ReleaseId::new(state.next_release_id).expect("increment starts at one"),
            tag: release.tag.clone(),
            name: release.name.clone(),
            body: release.body.clone(),
            draft: release.draft,
            prerelease: release.prerelease,
            assets: Vec::new(),
        };
        state
            .releases
            .insert((repository.clone(), created.tag.clone()), created.clone());
        Ok(created)
    }

    async fn upload_asset(
        &self,
        repository: &Repository,
        release_id: ReleaseId,
        name: &str,
        label: Option<&str>,
        content: Bytes,
    ) -> Result<ReleaseAsset> {
        let mut state = self.state.write().await;
        state.next_asset_id += 1;
        let asset = ReleaseAsset {
            id: AssetId::new(state.next_asset_id).expect("increment starts at one"),
            name: name.to_owned(),
            label: label.map(str::to_owned),
            size: content.len() as u64,
            download_url: format!("memory://{}/{}", repository, state.next_asset_id),
        };
        let release = state
            .releases
            .values_mut()
            .find(|release| release.id == release_id)
            .ok_or_else(|| missing(format!("release {release_id:?}")))?;
        release.assets.push(asset.clone());
        state.assets.insert((repository.clone(), asset.id), content);
        Ok(asset)
    }

    async fn download_asset(&self, repository: &Repository, asset_id: AssetId) -> Result<Bytes> {
        self.state
            .read()
            .await
            .assets
            .get(&(repository.clone(), asset_id))
            .cloned()
            .ok_or_else(|| missing(format!("asset {asset_id:?} in {repository}")))
    }
}

#[cfg(test)]
mod tests {
    use super::FakeProvider;
    use postel_contracts::{PrDomain, RepoDomain};
    use postel_model::{
        PullRequestNumber, Repository, RepositoryFacts, RepositorySettings, ReviewComment,
        ReviewThread,
    };

    #[tokio::test]
    async fn consumer_observes_changes_through_the_same_contract() {
        let provider = FakeProvider::new();
        let repository = Repository::new("faictor", "sandbox").expect("repository");
        provider
            .seed_repository(
                RepositoryFacts {
                    repository: repository.clone(),
                    default_branch: "main".to_owned(),
                    private: true,
                    archived: false,
                },
                RepositorySettings {
                    allow_squash_merge: true,
                    allow_merge_commit: false,
                    allow_rebase_merge: false,
                    delete_branch_on_merge: true,
                },
            )
            .await;
        assert!(
            provider
                .settings(&repository)
                .await
                .expect("settings")
                .allow_squash_merge
        );

        let number = PullRequestNumber::new(3).expect("number");
        provider
            .request_reviewers(&repository, number, &["reviewer".to_owned()])
            .await
            .expect("request reviewers");
        assert_eq!(
            provider.requested_reviewers(&repository, number).await,
            ["reviewer"]
        );
    }

    #[tokio::test]
    async fn consumer_reads_complete_review_conversations_through_the_contract() {
        let provider = FakeProvider::new();
        let repository = Repository::new("faictor", "sandbox").expect("repository");
        let number = PullRequestNumber::new(3).expect("number");
        let thread = ReviewThread {
            id: "thread-1".to_owned(),
            resolved: false,
            path: Some("src/lib.rs".to_owned()),
            line: Some(10),
            comments: vec![
                ReviewComment {
                    body: "question".to_owned(),
                    author: "reviewer".to_owned(),
                },
                ReviewComment {
                    body: "answer".to_owned(),
                    author: "author".to_owned(),
                },
            ],
        };
        provider
            .seed_review_threads(repository.clone(), number, vec![thread.clone()])
            .await;

        assert_eq!(
            provider
                .review_threads(&repository, number)
                .await
                .expect("review threads"),
            [thread]
        );
    }
}
