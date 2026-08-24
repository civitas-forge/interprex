//! Issue and label operations owned by the tracker domain.
//!
//! GitHub presents pull requests through its issues routes too, but this module
//! intentionally models only tracker issues and label taxonomy. Pull-request
//! review facts remain in `pr`, preventing a convenient vendor route from
//! moving ownership between contracts.

use async_trait::async_trait;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use postel_contracts::{ProviderError, Result, TrackerDomain};
use postel_model::{Issue, IssueNumber, Label, OpenClosed, Repository};
use serde::Deserialize;

use crate::{GithubProvider, api::external};

#[derive(Deserialize)]
struct GithubIssue {
    number: u64,
    title: String,
    body: Option<String>,
    state: String,
    #[serde(default)]
    labels: Vec<GithubLabel>,
    updated_at: chrono::DateTime<chrono::Utc>,
    pull_request: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct GithubLabel {
    name: String,
    color: String,
    description: Option<String>,
}

impl From<GithubLabel> for Label {
    fn from(value: GithubLabel) -> Self {
        Self {
            name: value.name,
            color: value.color,
            description: value.description,
        }
    }
}

fn normalize_issue(value: GithubIssue) -> Result<Issue> {
    if value.pull_request.is_some() {
        return Err(ProviderError::Refused {
            provider: "github",
            fact: "an issue-domain read addressed a pull request".to_owned(),
        });
    }
    Ok(Issue {
        number: IssueNumber::new(value.number).map_err(|error| ProviderError::External {
            provider: "github",
            operation: "normalize issue",
            message: error.to_string(),
        })?,
        title: value.title,
        body: value.body,
        state: if value.state == "open" {
            OpenClosed::Open
        } else {
            OpenClosed::Closed
        },
        labels: value.labels.into_iter().map(Into::into).collect(),
        updated_at: value.updated_at,
    })
}

#[async_trait]
impl TrackerDomain for GithubProvider {
    async fn issue(&self, repository: &Repository, number: IssueNumber) -> Result<Issue> {
        let response: GithubIssue = self
            .user()?
            .get(
                format!("/repos/{repository}/issues/{}", number.get()),
                None::<&()>,
            )
            .await
            .map_err(|error| {
                crate::api::read_error(
                    "read issue",
                    format!("issue {} in {repository}", number.get()),
                    error,
                )
            })?;
        normalize_issue(response)
    }

    async fn labels(&self, repository: &Repository) -> Result<Vec<Label>> {
        let response: Vec<GithubLabel> = self
            .user()?
            .get(
                format!("/repos/{repository}/labels"),
                Some(&[("per_page", 100)]),
            )
            .await
            .map_err(|error| external("list labels", error))?;
        Ok(response.into_iter().map(Into::into).collect())
    }

    async fn upsert_label(&self, repository: &Repository, label: &Label) -> Result<Label> {
        let existing = self.labels(repository).await?;
        let response: GithubLabel = if existing.iter().any(|existing| existing.name == label.name) {
            self.user()?
                .patch(
                    format!(
                        "/repos/{repository}/labels/{}",
                        utf8_percent_encode(&label.name, NON_ALPHANUMERIC)
                    ),
                    Some(label),
                )
                .await
                .map_err(|error| external("update label", error))?
        } else {
            self.user()?
                .post(format!("/repos/{repository}/labels"), Some(label))
                .await
                .map_err(|error| external("create label", error))?
        };
        Ok(response.into())
    }
}

#[cfg(test)]
mod tests {
    use super::{GithubIssue, normalize_issue};

    #[test]
    fn issue_fixture_normalizes_labels_and_state() {
        let response: GithubIssue =
            serde_json::from_str(include_str!("../tests/fixtures/issue.json")).expect("fixture");
        let issue = normalize_issue(response).expect("normalizes");
        assert_eq!(issue.number.get(), 11);
        assert_eq!(issue.labels[0].name, "feature");
    }
}
