use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{ProviderApp, ReviewActor, ReviewFinding, ReviewId};

/// The exact code revision attached to a review.
///
/// Some providers, including GitHub, retain the reviewed head commit but not
/// the base commit as it existed when a historical review was submitted.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ReviewedRevision {
    pub head_sha: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDisposition {
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
}

/// What the provider can establish about a review author's relationship to
/// the change request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewRelationship {
    ChangeAuthor,
    Other,
    Unknown,
}

/// The author of a review and the provider's knowledge of that actor's
/// relationship to the change request.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewAuthor {
    ChangeAuthor,
    Other(ReviewActor),
    Unknown(ReviewActor),
}

impl ReviewAuthor {
    #[must_use]
    pub const fn relationship(&self) -> ReviewRelationship {
        match self {
            Self::ChangeAuthor => ReviewRelationship::ChangeAuthor,
            Self::Other(_) => ReviewRelationship::Other,
            Self::Unknown(_) => ReviewRelationship::Unknown,
        }
    }

    #[must_use]
    pub fn actor<'a>(&'a self, change_author: &'a ReviewActor) -> &'a ReviewActor {
        match self {
            Self::ChangeAuthor => change_author,
            Self::Other(actor) | Self::Unknown(actor) => actor,
        }
    }
}

/// Whether a review is still a draft or has been submitted.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Draft,
    Submitted {
        disposition: ReviewDisposition,
        submitted_at: DateTime<Utc>,
    },
}

/// One platform review, including drafts and reviews by the change author.
///
/// Multiple reviews by the same actor remain independent. Relationship is an
/// observed fact, not a decision about whether the review counts as independent
/// evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Review {
    pub id: ReviewId,
    pub author: ReviewAuthor,
    pub via_app: Option<ProviderApp>,
    pub revision: ReviewedRevision,
    pub state: ReviewState,
    pub summary: Option<String>,
    pub findings: Vec<ReviewFinding>,
}
