use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{Repository, Result};

platform_number!(IssueNumber);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenClosed {
    Open,
    Closed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Label {
    pub name: String,
    pub color: String,
    pub description: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Issue {
    pub number: IssueNumber,
    pub title: String,
    pub body: Option<String>,
    pub state: OpenClosed,
    pub labels: Vec<Label>,
    pub updated_at: DateTime<Utc>,
}

#[async_trait]
pub trait IssuesProvider: Send + Sync {
    async fn issue(&self, repository: &Repository, number: IssueNumber) -> Result<Issue>;
    async fn labels(&self, repository: &Repository) -> Result<Vec<Label>>;
    async fn upsert_label(&self, repository: &Repository, label: &Label) -> Result<Label>;
}

#[cfg(test)]
mod tests {
    use super::IssueNumber;
    use crate::ModelError;

    #[test]
    fn platform_numbers_are_one_based() {
        assert_eq!(IssueNumber::new(0), Err(ModelError::InvalidNumber));
        assert_eq!(IssueNumber::new(42).expect("positive").get(), 42);
    }
}
