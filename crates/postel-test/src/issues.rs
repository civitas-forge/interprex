use async_trait::async_trait;
use postel::{Issue, IssueNumber, IssuesProvider, Label, Repository, Result};

use crate::state::{FakeProvider, missing};

#[async_trait]
impl IssuesProvider for FakeProvider {
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
