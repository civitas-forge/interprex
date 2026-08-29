use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{ReviewActor, ReviewCommentId, ReviewThreadId};

platform_number!(ReviewLine);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewThreadStatus {
    Open,
    Resolved,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewComment {
    pub id: ReviewCommentId,
    pub author: ReviewActor,
    pub body: String,
    pub created_at: DateTime<Utc>,
    /// The last known edit time, when the provider supplies one.
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewLineRange {
    pub start: Option<ReviewLine>,
    pub end: ReviewLine,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewDiffSide {
    Left,
    Right,
}

/// The stable source anchor within the file containing an inline review
/// thread. A line range records the location at which the thread began.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewAnchor {
    File,
    Lines {
        side: ReviewDiffSide,
        original: ReviewLineRange,
        current: Option<ReviewLineRange>,
    },
}

/// The file and anchor of an inline review thread.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewLocation {
    pub path: String,
    pub anchor: ReviewAnchor,
}

/// The facts shared by findings and standalone inline threads.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReviewThread {
    pub id: ReviewThreadId,
    pub location: ReviewLocation,
    pub outdated: bool,
    pub status: ReviewThreadStatus,
    pub comment: ReviewComment,
    /// Replies in stable, total provider order from earliest to latest.
    pub replies: Vec<ReviewComment>,
}
