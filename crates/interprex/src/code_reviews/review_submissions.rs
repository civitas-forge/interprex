use serde::{Deserialize, Serialize};

use super::{ReviewDiffSide, ReviewDisposition, ReviewLine, ReviewedRevision};
use crate::ModelError;

/// A caller-assigned identifier for one review publication.
///
/// The key is meaningful only for one reviewer identity within the change
/// request supplied to
/// [`ReviewPublishingProvider::publish_review`](super::ReviewPublishingProvider::publish_review).
/// Two distinct reviewer identities can use the same key independently. A
/// caller retains the key across retries so a provider can find a review
/// created by an earlier attempt from that reviewer.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct ReviewPublicationKey(String);

impl ReviewPublicationKey {
    /// Constructs a nonblank publication key.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError::Empty`] when `value` contains no visible text.
    pub fn new(value: impl Into<String>) -> std::result::Result<Self, ModelError> {
        Ok(Self(nonblank(value, "review publication key")?))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ReviewPublicationKey {
    type Error = ModelError;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ReviewPublicationKey> for String {
    fn from(value: ReviewPublicationKey) -> Self {
        value.0
    }
}

/// A disposition a provider can publish for a completed review.
///
/// Dismissal is absent because it changes an existing review rather than
/// describing a newly submitted one.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewSubmissionDisposition {
    Approved,
    ChangesRequested,
    Commented,
}

impl From<ReviewSubmissionDisposition> for ReviewDisposition {
    fn from(value: ReviewSubmissionDisposition) -> Self {
        match value {
            ReviewSubmissionDisposition::Approved => Self::Approved,
            ReviewSubmissionDisposition::ChangesRequested => Self::ChangesRequested,
            ReviewSubmissionDisposition::Commented => Self::Commented,
        }
    }
}

/// One inline finding included in a submitted review.
///
/// The line and side identify a line in the exact revision carried by the
/// enclosing [`ReviewSubmission`]. File-level findings and findings a provider
/// cannot anchor are not representable here; callers retain them in the review
/// summary or in their own structured text record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    try_from = "SerializedReviewSubmissionFinding",
    into = "SerializedReviewSubmissionFinding"
)]
pub struct ReviewSubmissionFinding {
    path: String,
    line: ReviewLine,
    side: ReviewDiffSide,
    body: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct SerializedReviewSubmissionFinding {
    path: String,
    line: ReviewLine,
    side: ReviewDiffSide,
    body: String,
}

impl ReviewSubmissionFinding {
    /// Constructs an inline finding with a nonblank path and body.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError::Empty`] when `path` or `body` contains no visible
    /// text. [`ReviewLine`] rejects zero before this call.
    pub fn new(
        path: impl Into<String>,
        line: ReviewLine,
        side: ReviewDiffSide,
        body: impl Into<String>,
    ) -> std::result::Result<Self, ModelError> {
        Ok(Self {
            path: nonblank(path, "review finding path")?,
            line,
            side,
            body: nonblank(body, "review finding body")?,
        })
    }

    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    #[must_use]
    pub const fn line(&self) -> ReviewLine {
        self.line
    }

    #[must_use]
    pub const fn side(&self) -> &ReviewDiffSide {
        &self.side
    }

    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }
}

impl TryFrom<SerializedReviewSubmissionFinding> for ReviewSubmissionFinding {
    type Error = ModelError;

    fn try_from(
        value: SerializedReviewSubmissionFinding,
    ) -> std::result::Result<Self, Self::Error> {
        Self::new(value.path, value.line, value.side, value.body)
    }
}

impl From<ReviewSubmissionFinding> for SerializedReviewSubmissionFinding {
    fn from(value: ReviewSubmissionFinding) -> Self {
        Self {
            path: value.path,
            line: value.line,
            side: value.side,
            body: value.body,
        }
    }
}

/// One complete review that a provider publishes as a reviewer identity.
///
/// `revision` names the exact commit reviewed. A provider must publish against
/// that commit and must not replace it with the change request's current head.
/// The visible `summary` and every finding body may include structured records
/// produced through [`TextRecordsProvider`](super::TextRecordsProvider).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    try_from = "SerializedReviewSubmission",
    into = "SerializedReviewSubmission"
)]
pub struct ReviewSubmission {
    publication_key: ReviewPublicationKey,
    revision: ReviewedRevision,
    disposition: ReviewSubmissionDisposition,
    summary: String,
    findings: Vec<ReviewSubmissionFinding>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
struct SerializedReviewSubmission {
    publication_key: ReviewPublicationKey,
    revision: ReviewedRevision,
    disposition: ReviewSubmissionDisposition,
    summary: String,
    findings: Vec<ReviewSubmissionFinding>,
}

impl ReviewSubmission {
    /// Constructs a complete review after validating its text and revision.
    ///
    /// # Errors
    ///
    /// Returns [`ModelError::Empty`] when `revision.head_sha` or `summary`
    /// contains no visible text. The publication key and findings have already
    /// passed their own construction checks.
    pub fn new(
        publication_key: ReviewPublicationKey,
        revision: ReviewedRevision,
        disposition: ReviewSubmissionDisposition,
        summary: impl Into<String>,
        findings: Vec<ReviewSubmissionFinding>,
    ) -> std::result::Result<Self, ModelError> {
        nonblank(revision.head_sha.clone(), "reviewed revision head sha")?;
        Ok(Self {
            publication_key,
            revision,
            disposition,
            summary: nonblank(summary, "review submission summary")?,
            findings,
        })
    }

    #[must_use]
    pub const fn publication_key(&self) -> &ReviewPublicationKey {
        &self.publication_key
    }

    #[must_use]
    pub const fn revision(&self) -> &ReviewedRevision {
        &self.revision
    }

    #[must_use]
    pub const fn disposition(&self) -> ReviewSubmissionDisposition {
        self.disposition
    }

    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    #[must_use]
    pub fn findings(&self) -> &[ReviewSubmissionFinding] {
        &self.findings
    }
}

impl TryFrom<SerializedReviewSubmission> for ReviewSubmission {
    type Error = ModelError;

    fn try_from(value: SerializedReviewSubmission) -> std::result::Result<Self, Self::Error> {
        Self::new(
            value.publication_key,
            value.revision,
            value.disposition,
            value.summary,
            value.findings,
        )
    }
}

impl From<ReviewSubmission> for SerializedReviewSubmission {
    fn from(value: ReviewSubmission) -> Self {
        Self {
            publication_key: value.publication_key,
            revision: value.revision,
            disposition: value.disposition,
            summary: value.summary,
            findings: value.findings,
        }
    }
}

fn nonblank(
    value: impl Into<String>,
    field: &'static str,
) -> std::result::Result<String, ModelError> {
    let value = value.into();
    if value.trim().is_empty() {
        return Err(ModelError::Empty { field });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding() -> ReviewSubmissionFinding {
        ReviewSubmissionFinding::new(
            "src/lib.rs",
            ReviewLine::new(17).expect("line"),
            ReviewDiffSide::Right,
            "Handle the error.",
        )
        .expect("finding")
    }

    #[test]
    fn submission_round_trips_through_its_public_format() {
        let submission = ReviewSubmission::new(
            ReviewPublicationKey::new("round-2:codex").expect("key"),
            ReviewedRevision {
                head_sha: "abc123".to_owned(),
            },
            ReviewSubmissionDisposition::ChangesRequested,
            "One finding.",
            vec![finding()],
        )
        .expect("submission");

        let encoded = serde_json::to_value(&submission).expect("serialize submission");
        assert_eq!(
            serde_json::from_value::<ReviewSubmission>(encoded).expect("deserialize submission"),
            submission
        );
    }

    #[test]
    fn publication_models_reject_blank_text_during_construction_and_deserialization() {
        assert_eq!(
            ReviewPublicationKey::new(" \n\t"),
            Err(ModelError::Empty {
                field: "review publication key"
            })
        );
        for (path, body, field) in [
            ("  ", "finding", "review finding path"),
            ("src/lib.rs", "\n", "review finding body"),
        ] {
            assert_eq!(
                ReviewSubmissionFinding::new(
                    path,
                    ReviewLine::new(1).expect("line"),
                    ReviewDiffSide::Right,
                    body,
                ),
                Err(ModelError::Empty { field })
            );
        }
        assert!(
            serde_json::from_value::<ReviewSubmissionFinding>(serde_json::json!({
                "path": " ",
                "line": 1,
                "side": "right",
                "body": "finding"
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<ReviewSubmissionFinding>(serde_json::json!({
                "path": "src/lib.rs",
                "line": 0,
                "side": "right",
                "body": "finding"
            }))
            .is_err()
        );
        assert!(serde_json::from_value::<ReviewLine>(serde_json::json!(0)).is_err());
        assert!(
            serde_json::from_value::<ReviewSubmission>(serde_json::json!({
                "publication_key": "round-2:codex",
                "revision": {"head_sha": " "},
                "disposition": "commented",
                "summary": "summary",
                "findings": []
            }))
            .is_err()
        );
        assert_eq!(
            ReviewSubmission::new(
                ReviewPublicationKey::new("round-2:codex").expect("key"),
                ReviewedRevision {
                    head_sha: "abc123".to_owned(),
                },
                ReviewSubmissionDisposition::Commented,
                "\t",
                Vec::new(),
            ),
            Err(ModelError::Empty {
                field: "review submission summary"
            })
        );
        assert_eq!(ReviewLine::new(0), Err(ModelError::InvalidNumber));
    }

    #[test]
    fn dismissed_is_not_a_publishable_disposition() {
        assert!(
            serde_json::from_value::<ReviewSubmissionDisposition>(serde_json::json!("dismissed"))
                .is_err()
        );
    }
}
