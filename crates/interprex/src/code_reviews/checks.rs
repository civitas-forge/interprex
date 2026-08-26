use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::ProviderApp;

/// The conclusion an observed check reached once it finished.
///
/// The variants cover the conclusions GitHub reports for a check run, so a
/// read never has to discard one. That set is wider by one than the set a
/// client may write, because GitHub sets `stale` itself. Publishing uses the
/// narrower [`PublishedCheckConclusion`].
///
/// `startup_failure` is absent deliberately: GitHub reports it for a check
/// suite that failed before its runs began and states that it does not apply
/// to check runs. The jobs domain models it as
/// `RunConclusion::StartupFailure`, where it is observable.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckConclusion {
    Success,
    Failure,
    Neutral,
    Cancelled,
    TimedOut,
    ActionRequired,
    Skipped,
    Stale,
}

/// The conclusion a published check can report.
///
/// This is narrower than the observed [`CheckConclusion`] by one variant:
/// GitHub sets `stale` on a check run itself and refuses it from a client, so
/// no value here can produce that request.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PublishedCheckConclusion {
    Success,
    Failure,
    Neutral,
    Cancelled,
    TimedOut,
    ActionRequired,
    Skipped,
}

/// Where an observed check stands, and what it concluded once it has
/// finished.
///
/// A check that has not finished has no conclusion, so the two facts stay in
/// one value rather than in an optional field that could contradict a status.
/// The variants before `Completed` are the platform's own, one for each status
/// GitHub reports on a check run, because a stalled check and a running one
/// call for different reporting and Interprex does not decide which
/// distinctions a caller needs.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// The check exists and has not been queued yet.
    Requested,
    /// The check is queued to run.
    Queued,
    /// The check is held back, on GitHub because a concurrency limit is
    /// reached.
    Pending,
    /// The check is held back until a deployment protection rule is
    /// satisfied.
    Waiting,
    /// The check is running.
    InProgress,
    Completed {
        conclusion: CheckConclusion,
        completed_at: DateTime<Utc>,
    },
}

impl CheckStatus {
    /// The conclusion this check reached, and `None` while it has not
    /// finished.
    #[must_use]
    pub const fn conclusion(&self) -> Option<CheckConclusion> {
        match self {
            Self::Completed { conclusion, .. } => Some(*conclusion),
            Self::Requested | Self::Queued | Self::Pending | Self::Waiting | Self::InProgress => {
                None
            }
        }
    }
}

/// One check the platform recorded against a commit.
///
/// A required-check rule names the check it requires by `name`, which is
/// `RequiredCheck::context` on the code-hosting side, and may also name the
/// application that must publish it, which is `RequiredCheck::integration_id`.
/// `via_app` carries that application as the platform reported it. The two
/// identifiers hold the same GitHub app identifier in different types: an
/// integer in the rule, and its decimal spelling in `ProviderAppId`, which is
/// opaque because other providers need not use integers. A caller comparing
/// them today compares `via_app.id.as_str()` against
/// `integration_id.to_string()`. Interprex performs no part of that
/// comparison.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckRun {
    pub name: String,
    pub head_sha: String,
    /// The application that published the check, when the platform names one.
    pub via_app: Option<ProviderApp>,
    pub status: CheckStatus,
    /// The check's published summary text, when it published nonblank text.
    pub summary: Option<String>,
    /// Where a person can read the check on the platform, when it published a
    /// location.
    pub html_url: Option<String>,
}

/// A finished check result to publish.
///
/// This write shape is deliberately narrower than the observed [`CheckRun`]:
/// Interprex publishes only a result that has concluded, so the conclusion and
/// the summary are both required, and only conclusions a client may set are
/// representable.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckOutcome {
    pub name: String,
    pub head_sha: String,
    pub conclusion: PublishedCheckConclusion,
    pub summary: String,
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn every_status_before_completion_carries_no_conclusion() {
        for status in [
            CheckStatus::Requested,
            CheckStatus::Queued,
            CheckStatus::Pending,
            CheckStatus::Waiting,
            CheckStatus::InProgress,
        ] {
            assert_eq!(status.conclusion(), None);
        }
    }

    #[test]
    fn an_observed_check_carries_a_conclusion_only_once_it_has_completed() {
        let running = CheckRun {
            name: "quality".to_owned(),
            head_sha: "head".to_owned(),
            via_app: None,
            status: CheckStatus::InProgress,
            summary: None,
            html_url: None,
        };
        let completed = CheckRun {
            status: CheckStatus::Completed {
                conclusion: CheckConclusion::TimedOut,
                completed_at: Utc.timestamp_opt(3, 0).single().expect("timestamp"),
            },
            summary: Some("The job exceeded its limit.".to_owned()),
            ..running.clone()
        };

        assert_eq!(running.status.conclusion(), None);
        assert_eq!(
            completed.status.conclusion(),
            Some(CheckConclusion::TimedOut)
        );
        assert_eq!(
            serde_json::to_value(&running.status).expect("serializes running status"),
            serde_json::json!("in_progress")
        );
        assert_eq!(
            serde_json::to_value(&completed.status).expect("serializes completed status"),
            serde_json::json!({
                "completed": {
                    "conclusion": "timed_out",
                    "completed_at": "1970-01-01T00:00:03Z"
                }
            })
        );
    }

    #[test]
    fn check_conclusions_cover_every_conclusion_a_check_run_reports() {
        for (conclusion, expected) in [
            (CheckConclusion::Success, "success"),
            (CheckConclusion::Failure, "failure"),
            (CheckConclusion::Neutral, "neutral"),
            (CheckConclusion::Cancelled, "cancelled"),
            (CheckConclusion::TimedOut, "timed_out"),
            (CheckConclusion::ActionRequired, "action_required"),
            (CheckConclusion::Skipped, "skipped"),
            (CheckConclusion::Stale, "stale"),
        ] {
            assert_eq!(
                serde_json::to_value(conclusion).expect("serializes conclusion"),
                serde_json::json!(expected)
            );
        }
    }
}
