use serde::{Deserialize, Serialize};

use super::{ReviewComment, ReviewCommentId, ReviewThread};
use crate::ModelError;

/// The addressing user's assessment of a finding's effect on the change.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Critical,
    Major,
    Minor,
    Nit,
}

/// Why the addressing user considers a finding complete.
///
/// The variants and serialized spellings match GitHub's
/// `PullRequestReviewThreadResolutionReason` enum.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FindingResolutionReason {
    /// The review comment was addressed.
    Addressed,
    /// The review comment is invalid.
    Invalid,
    /// The review comment will not be addressed.
    WontFix,
}

/// The addressing user's recorded conclusion for one finding.
///
/// This is distinct from
/// [`ReviewThreadStatus`](super::ReviewThreadStatus). A platform thread can have no
/// Interprex resolution because it was resolved outside this interface, and a
/// failed multi-request provider operation can leave a recorded conclusion on
/// a thread the platform still reports as open.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FindingResolution {
    pub reason: FindingResolutionReason,
    /// The severity assigned by the user addressing the finding. It need not
    /// match a severity stated by the reviewer.
    pub addressing_severity: FindingSeverity,
}

/// The nonblank visible explanation attached to a finding resolution.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "String", into = "String")]
pub struct FindingResolutionReply(String);

impl FindingResolutionReply {
    pub fn new(value: impl Into<String>) -> std::result::Result<Self, ModelError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ModelError::Empty {
                field: "finding resolution reply",
            });
        }
        Ok(Self(value))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for FindingResolutionReply {
    type Error = ModelError;

    fn try_from(value: String) -> std::result::Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<FindingResolutionReply> for String {
    fn from(value: FindingResolutionReply) -> Self {
        value.0
    }
}

/// One observed finding resolution and the reply that recorded it.
///
/// The source reply identifier links to the addressing actor, explanation and
/// platform timestamps in the containing thread's replies.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "compatibility")]
pub enum FindingResolutionRecord {
    Supported {
        resolution: FindingResolution,
        source_reply_id: ReviewCommentId,
    },
    Unsupported {
        metadata_format: String,
        source_reply_id: ReviewCommentId,
    },
}

impl FindingResolutionRecord {
    #[must_use]
    pub fn supported_resolution(&self) -> Option<FindingResolution> {
        match self {
            Self::Supported { resolution, .. } => Some(*resolution),
            Self::Unsupported { .. } => None,
        }
    }

    #[must_use]
    pub fn source_reply_id(&self) -> &ReviewCommentId {
        match self {
            Self::Supported {
                source_reply_id, ..
            }
            | Self::Unsupported {
                source_reply_id, ..
            } => source_reply_id,
        }
    }
}

/// One inline thread attached to the review in which it originated.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewFinding {
    #[serde(flatten)]
    pub thread: ReviewThread,
    pub resolution: Option<FindingResolutionRecord>,
}

impl std::ops::Deref for ReviewFinding {
    type Target = ReviewThread;

    fn deref(&self) -> &Self::Target {
        &self.thread
    }
}

impl std::ops::DerefMut for ReviewFinding {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.thread
    }
}

impl ReviewFinding {
    /// Returns the reply that records this finding's resolution.
    #[must_use]
    pub fn resolution_reply(&self) -> Option<&ReviewComment> {
        let reply_id = self.resolution.as_ref()?.source_reply_id();
        self.replies.iter().find(|reply| &reply.id == reply_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finding_resolution_reasons_use_githubs_enum_spellings() {
        for (reason, expected) in [
            (FindingResolutionReason::Addressed, "ADDRESSED"),
            (FindingResolutionReason::Invalid, "INVALID"),
            (FindingResolutionReason::WontFix, "WONT_FIX"),
        ] {
            let resolution = FindingResolution {
                reason,
                addressing_severity: FindingSeverity::Major,
            };

            assert_eq!(
                serde_json::to_value(resolution).expect("serializes resolution"),
                serde_json::json!({
                    "reason": expected,
                    "addressing_severity": "major"
                })
            );
        }
    }

    #[test]
    fn finding_resolution_replies_require_visible_explanatory_text() {
        assert!(FindingResolutionReply::new("\n\t").is_err());
        let reply = FindingResolutionReply::new("Addressed in the current revision.")
            .expect("visible explanation");
        assert_eq!(reply.as_str(), "Addressed in the current revision.");
    }
}
