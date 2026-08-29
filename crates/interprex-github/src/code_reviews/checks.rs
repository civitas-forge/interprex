use std::collections::BTreeSet;

use interprex::{
    CheckConclusion, CheckRun, CheckStatus, ProviderError, PublishedCheckConclusion, Repository,
    Result,
};
use serde::Deserialize;

use crate::GithubProvider;

use super::actors::{GithubApp, normalize_app};

/// The page size every check-runs request asks for, and the size a short page
/// is measured against.
const CHECK_RUNS_PER_PAGE: usize = 100;

/// One page of GitHub's check-runs envelope.
///
/// GitHub returns the runs under `check_runs` beside the complete
/// `total_count`, not as a bare array. Octocrab's `Page` recognizes a fixed
/// set of envelope keys that does not include this one, so this read pages by
/// number instead of following `Link` headers through `all_pages`.
#[derive(Deserialize)]
struct GithubCheckRunPage {
    total_count: u64,
    check_runs: Vec<GithubCheckRun>,
}

#[derive(Deserialize)]
struct GithubCheckSuitePage {
    total_count: u64,
    check_suites: Vec<GithubCheckSuite>,
}

#[derive(Deserialize)]
struct GithubCheckSuite {
    id: u64,
    head_sha: String,
}

#[derive(Deserialize)]
struct GithubCheckSuiteReference {
    id: u64,
}

#[derive(Deserialize)]
pub(crate) struct GithubCheckRun {
    #[serde(default)]
    id: Option<u64>,
    name: String,
    head_sha: String,
    status: String,
    conclusion: Option<String>,
    completed_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The app that published the run. It carries the same identifier a
    /// ruleset reports as `integration_id`.
    app: Option<GithubApp>,
    #[serde(default)]
    check_suite: Option<GithubCheckSuiteReference>,
    html_url: Option<String>,
    output: Option<GithubCheckRunOutput>,
}

#[derive(Deserialize)]
struct GithubCheckRunOutput {
    summary: Option<String>,
}

fn normalize_check_conclusion(value: &str) -> Result<CheckConclusion> {
    match value {
        "success" => Ok(CheckConclusion::Success),
        "failure" => Ok(CheckConclusion::Failure),
        "neutral" => Ok(CheckConclusion::Neutral),
        "cancelled" => Ok(CheckConclusion::Cancelled),
        "timed_out" => Ok(CheckConclusion::TimedOut),
        "action_required" => Ok(CheckConclusion::ActionRequired),
        "skipped" => Ok(CheckConclusion::Skipped),
        "stale" => Ok(CheckConclusion::Stale),
        other => Err(ProviderError::Unrepresentable {
            provider: "github",
            fact: format!("unknown check run conclusion {other}"),
        }),
    }
}

pub(crate) fn normalize_check_run(value: GithubCheckRun) -> Result<CheckRun> {
    let unfinished = match value.status.as_str() {
        "requested" => Some(CheckStatus::Requested),
        "queued" => Some(CheckStatus::Queued),
        "pending" => Some(CheckStatus::Pending),
        "waiting" => Some(CheckStatus::Waiting),
        "in_progress" => Some(CheckStatus::InProgress),
        "completed" => None,
        other => {
            return Err(ProviderError::Unrepresentable {
                provider: "github",
                fact: format!("unknown check run status {other}"),
            });
        }
    };
    let status = if let Some(unfinished) = unfinished {
        if let Some(conclusion) = value.conclusion {
            return Err(ProviderError::Unrepresentable {
                provider: "github",
                fact: format!(
                    "check run {} is {} and has conclusion {conclusion}",
                    value.name, value.status
                ),
            });
        }
        if value.completed_at.is_some() {
            return Err(ProviderError::Unrepresentable {
                provider: "github",
                fact: format!(
                    "check run {} is {} and has a completion time",
                    value.name, value.status
                ),
            });
        }
        unfinished
    } else {
        let conclusion = value
            .conclusion
            .ok_or_else(|| ProviderError::Unrepresentable {
                provider: "github",
                fact: format!("completed check run {} has no conclusion", value.name),
            })?;
        let completed_at = value
            .completed_at
            .ok_or_else(|| ProviderError::Unrepresentable {
                provider: "github",
                fact: format!("completed check run {} has no completion time", value.name),
            })?;
        CheckStatus::Completed {
            conclusion: normalize_check_conclusion(&conclusion)?,
            completed_at,
        }
    };
    Ok(CheckRun {
        name: value.name,
        head_sha: value.head_sha,
        via_app: value.app.map(normalize_app).transpose()?,
        status,
        summary: value
            .output
            .and_then(|output| output.summary)
            .filter(|summary| !summary.trim().is_empty()),
        html_url: value.html_url,
    })
}

pub(super) fn conclusion(value: &PublishedCheckConclusion) -> &'static str {
    match value {
        PublishedCheckConclusion::Success => "success",
        PublishedCheckConclusion::Failure => "failure",
        PublishedCheckConclusion::Neutral => "neutral",
        PublishedCheckConclusion::Cancelled => "cancelled",
        PublishedCheckConclusion::TimedOut => "timed_out",
        PublishedCheckConclusion::ActionRequired => "action_required",
        PublishedCheckConclusion::Skipped => "skipped",
    }
}

impl GithubProvider {
    pub(crate) async fn github_check_runs(
        &self,
        repository: &Repository,
        head_sha: &str,
    ) -> Result<Vec<GithubCheckRun>> {
        let mut collected: Vec<GithubCheckRun> = Vec::new();
        let mut page = 1_u32;
        loop {
            let response: GithubCheckRunPage = self
                .user()?
                .get(
                    format!("/repos/{repository}/commits/{head_sha}/check-runs"),
                    Some(&[
                        ("per_page", CHECK_RUNS_PER_PAGE.to_string()),
                        ("page", page.to_string()),
                        // GitHub's default, sent explicitly: within each check
                        // suite on the commit, its current run of each check.
                        // Suites remain separate, so runs can share a name.
                        ("filter", "latest".to_owned()),
                    ]),
                )
                .await
                .map_err(|error| {
                    crate::client::read_error(
                        "read check runs",
                        format!("commit {head_sha} in {repository}"),
                        error,
                    )
                })?;
            let received = response.check_runs.len();
            collected.extend(response.check_runs);
            if received < CHECK_RUNS_PER_PAGE || collected.len() as u64 >= response.total_count {
                return Ok(collected);
            }
            page += 1;
        }
    }

    pub(crate) async fn complete_github_check_runs(
        &self,
        repository: &Repository,
        head_sha: &str,
    ) -> Result<Vec<GithubCheckRun>> {
        let suites = self.github_check_suites(repository, head_sha).await?;
        validate_check_suites(&suites, head_sha)?;
        let mut runs = Vec::new();
        let mut run_ids = BTreeSet::new();
        for suite in suites {
            for run in self
                .github_check_runs_for_suite(repository, suite.id)
                .await?
            {
                let run_id = run.id.ok_or_else(|| {
                    unrepresentable(format!("check run in suite {} has no id", suite.id))
                })?;
                if run_id == 0 || !run_ids.insert(run_id) {
                    return Err(unrepresentable(format!(
                        "check run id {run_id} is zero or repeated"
                    )));
                }
                let observed_suite = run.check_suite.as_ref().ok_or_else(|| {
                    unrepresentable(format!("check run {run_id} has no check-suite identity"))
                })?;
                if observed_suite.id != suite.id {
                    return Err(unrepresentable(format!(
                        "check run {run_id} names suite {} instead of {}",
                        observed_suite.id, suite.id
                    )));
                }
                if run.head_sha != head_sha {
                    return Err(unrepresentable(format!(
                        "check run {run_id} names revision {} instead of {head_sha}",
                        run.head_sha
                    )));
                }
                runs.push(run);
            }
        }
        Ok(runs)
    }

    async fn github_check_suites(
        &self,
        repository: &Repository,
        head_sha: &str,
    ) -> Result<Vec<GithubCheckSuite>> {
        let mut collected = Vec::new();
        let mut page = 1_u32;
        let mut expected_total = None;
        loop {
            let response: GithubCheckSuitePage = self
                .user()?
                .get(
                    format!("/repos/{repository}/commits/{head_sha}/check-suites"),
                    Some(&[
                        ("per_page", CHECK_RUNS_PER_PAGE.to_string()),
                        ("page", page.to_string()),
                    ]),
                )
                .await
                .map_err(|error| {
                    crate::client::read_error(
                        "read check suites",
                        format!("commit {head_sha} in {repository}"),
                        error,
                    )
                })?;
            match expected_total {
                Some(total) if total != response.total_count => {
                    return Err(unrepresentable(format!(
                        "check-suite total changed from {total} to {} while paging",
                        response.total_count
                    )));
                }
                None => expected_total = Some(response.total_count),
                Some(_) => {}
            }
            let received = response.check_suites.len();
            collected.extend(response.check_suites);
            let expected_total = expected_total.unwrap_or(response.total_count);
            if collected.len() as u64 > expected_total {
                return Err(unrepresentable(format!(
                    "check suites reported {expected_total} records but returned at least {}",
                    collected.len()
                )));
            }
            if collected.len() as u64 >= expected_total {
                return Ok(collected);
            }
            if received < CHECK_RUNS_PER_PAGE {
                return Err(unrepresentable(format!(
                    "check suites reported {expected_total} records but returned only {}",
                    collected.len()
                )));
            }
            page += 1;
        }
    }

    async fn github_check_runs_for_suite(
        &self,
        repository: &Repository,
        suite_id: u64,
    ) -> Result<Vec<GithubCheckRun>> {
        let mut collected = Vec::new();
        let mut page = 1_u32;
        let mut expected_total = None;
        loop {
            let response: GithubCheckRunPage = self
                .user()?
                .get(
                    format!("/repos/{repository}/check-suites/{suite_id}/check-runs"),
                    Some(&[
                        ("per_page", CHECK_RUNS_PER_PAGE.to_string()),
                        ("page", page.to_string()),
                        ("filter", "latest".to_owned()),
                    ]),
                )
                .await
                .map_err(|error| {
                    crate::client::read_error(
                        "read check runs in suite",
                        format!("check suite {suite_id} in {repository}"),
                        error,
                    )
                })?;
            match expected_total {
                Some(total) if total != response.total_count => {
                    return Err(unrepresentable(format!(
                        "check-run total for suite {suite_id} changed from {total} to {} while paging",
                        response.total_count
                    )));
                }
                None => expected_total = Some(response.total_count),
                Some(_) => {}
            }
            let received = response.check_runs.len();
            collected.extend(response.check_runs);
            let expected_total = expected_total.unwrap_or(response.total_count);
            if collected.len() as u64 > expected_total {
                return Err(unrepresentable(format!(
                    "check suite {suite_id} reported {expected_total} runs but returned at least {}",
                    collected.len()
                )));
            }
            if collected.len() as u64 >= expected_total {
                return Ok(collected);
            }
            if received < CHECK_RUNS_PER_PAGE {
                return Err(unrepresentable(format!(
                    "check suite {suite_id} reported {expected_total} runs but returned only {}",
                    collected.len()
                )));
            }
            page += 1;
        }
    }
}

fn validate_check_suites(suites: &[GithubCheckSuite], expected_head: &str) -> Result<()> {
    let mut ids = BTreeSet::new();
    for suite in suites {
        if suite.id == 0 || !ids.insert(suite.id) {
            return Err(unrepresentable(format!(
                "check suite id {} is zero or repeated",
                suite.id
            )));
        }
        if suite.head_sha != expected_head {
            return Err(unrepresentable(format!(
                "check suite {} names revision {} instead of {expected_head}",
                suite.id, suite.head_sha
            )));
        }
    }
    Ok(())
}

fn unrepresentable(fact: impl Into<String>) -> ProviderError {
    ProviderError::Unrepresentable {
        provider: "github",
        fact: fact.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use interprex::{
        CheckConclusion, CheckOutcome, CheckStatus, CodeReviewsProvider, ConfigurationSource,
        ProviderError, PublishedCheckConclusion, Repository, Result,
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
    };

    use super::{
        GithubCheckRun, GithubCheckRunPage, GithubCheckSuite, conclusion, normalize_check_run,
        validate_check_suites,
    };
    use crate::{GithubProvider, client::ConfiguredApp};

    fn check_run(status: &str, conclusion: Option<&str>) -> GithubCheckRun {
        GithubCheckRun {
            id: None,
            name: "quality".to_owned(),
            head_sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
            status: status.to_owned(),
            conclusion: conclusion.map(str::to_owned),
            completed_at: conclusion
                .is_some()
                .then(|| "2026-08-24T10:04:00Z".parse().expect("completion time")),
            app: None,
            check_suite: None,
            html_url: None,
            output: None,
        }
    }

    #[test]
    fn check_suite_validation_has_no_false_one_thousand_suite_limit() {
        for count in [1_000_u64, 1_001] {
            let suites = (1..=count)
                .map(|id| GithubCheckSuite {
                    id,
                    head_sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
                })
                .collect::<Vec<_>>();
            validate_check_suites(&suites, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                .expect("every suite is accounted for independently of count");
        }
    }

    #[test]
    fn check_suite_validation_rejects_wrong_revisions_and_repeated_ids() {
        for suites in [
            vec![
                GithubCheckSuite {
                    id: 7,
                    head_sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
                },
                GithubCheckSuite {
                    id: 7,
                    head_sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
                },
            ],
            vec![GithubCheckSuite {
                id: 7,
                head_sha: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned(),
            }],
        ] {
            assert!(matches!(
                validate_check_suites(&suites, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
                Err(ProviderError::Unrepresentable { .. })
            ));
        }
    }
    #[test]
    fn check_run_fixture_keeps_a_running_check_separate_from_a_completed_one() {
        let page: GithubCheckRunPage =
            serde_json::from_str(include_str!("../../tests/fixtures/check_runs.json"))
                .expect("check run fixture");
        assert_eq!(page.total_count, 3);
        let runs = page
            .check_runs
            .into_iter()
            .map(normalize_check_run)
            .collect::<Result<Vec<_>>>()
            .expect("normalizes");

        assert_eq!(
            runs.iter().map(|run| run.name.as_str()).collect::<Vec<_>>(),
            ["quality", "integration", "docs"]
        );
        assert_eq!(
            runs[0].status,
            CheckStatus::Completed {
                conclusion: CheckConclusion::Failure,
                completed_at: "2026-08-24T10:04:00Z".parse().expect("completion time"),
            }
        );
        assert_eq!(
            runs[0].summary.as_deref(),
            Some("clippy reported one warning")
        );
        let publisher = runs[0].via_app.as_ref().expect("publishing app");
        assert_eq!(publisher.id.as_str(), "1042");
        assert_eq!(publisher.slug, "quality-app");
        assert_eq!(
            runs[0].html_url.as_deref(),
            Some("https://github.com/civitas-forge/interprex-sandbox/runs/41")
        );
        assert_eq!(runs[1].status, CheckStatus::InProgress);
        assert_eq!(runs[1].summary, None);
        assert_eq!(
            runs[2].status,
            CheckStatus::Completed {
                conclusion: CheckConclusion::Skipped,
                completed_at: "2026-08-24T10:01:00Z".parse().expect("completion time"),
            }
        );
        assert_eq!(runs[2].summary, None, "blank output text is not a summary");
        assert_eq!(runs[2].via_app, None);
        assert_eq!(runs[2].html_url, None);
    }
    #[test]
    fn every_status_github_reports_before_completion_keeps_its_own_meaning() {
        for (reported, expected) in [
            ("requested", CheckStatus::Requested),
            ("queued", CheckStatus::Queued),
            ("pending", CheckStatus::Pending),
            ("waiting", CheckStatus::Waiting),
            ("in_progress", CheckStatus::InProgress),
        ] {
            assert_eq!(
                normalize_check_run(check_run(reported, None))
                    .expect("normalizes")
                    .status,
                expected
            );
        }
    }
    #[test]
    fn a_check_suite_conclusion_is_not_a_check_run_conclusion() {
        assert!(matches!(
            normalize_check_run(check_run("completed", Some("startup_failure")))
                .expect_err("a check suite conclusion must be unrepresentable on a run"),
            ProviderError::Unrepresentable { fact, .. }
                if fact.contains("unknown check run conclusion startup_failure")
        ));
    }
    #[test]
    fn every_check_conclusion_github_reports_is_representable() {
        for (reported, expected) in [
            ("success", CheckConclusion::Success),
            ("failure", CheckConclusion::Failure),
            ("neutral", CheckConclusion::Neutral),
            ("cancelled", CheckConclusion::Cancelled),
            ("timed_out", CheckConclusion::TimedOut),
            ("action_required", CheckConclusion::ActionRequired),
            ("skipped", CheckConclusion::Skipped),
            ("stale", CheckConclusion::Stale),
        ] {
            let run = normalize_check_run(check_run("completed", Some(reported)))
                .expect("normalizes every reported conclusion");
            assert!(matches!(
                run.status,
                CheckStatus::Completed { conclusion, .. } if conclusion == expected
            ));
        }
    }
    #[test]
    fn unknown_check_statuses_and_conclusions_are_unrepresentable() {
        let unknown_status = normalize_check_run(check_run("paused", None))
            .expect_err("unknown status must be unrepresentable");
        assert!(matches!(
            unknown_status,
            ProviderError::Unrepresentable { fact, .. } if fact.contains("unknown check run status paused")
        ));

        let unknown_conclusion = normalize_check_run(check_run("completed", Some("abandoned")))
            .expect_err("unknown conclusion must be unrepresentable");
        assert!(matches!(
            unknown_conclusion,
            ProviderError::Unrepresentable { fact, .. }
                if fact.contains("unknown check run conclusion abandoned")
        ));
    }
    #[test]
    fn a_check_run_contradicting_its_own_status_is_unrepresentable() {
        let mut missing_conclusion = check_run("completed", Some("success"));
        missing_conclusion.conclusion = None;
        assert!(matches!(
            normalize_check_run(missing_conclusion)
                .expect_err("completed run without a conclusion must be unrepresentable"),
            ProviderError::Unrepresentable { fact, .. } if fact.contains("has no conclusion")
        ));

        let mut missing_time = check_run("completed", Some("success"));
        missing_time.completed_at = None;
        assert!(matches!(
            normalize_check_run(missing_time)
                .expect_err("completed run without a completion time must be unrepresentable"),
            ProviderError::Unrepresentable { fact, .. } if fact.contains("has no completion time")
        ));

        assert!(matches!(
            normalize_check_run(check_run("in_progress", Some("success")))
                .expect_err("running check run with a conclusion must be unrepresentable"),
            ProviderError::Unrepresentable { fact, .. }
                if fact.contains("is in_progress and has conclusion success")
        ));

        let mut completed_while_queued = check_run("queued", None);
        completed_while_queued.completed_at =
            Some("2026-08-24T10:04:00Z".parse().expect("completion time"));
        assert!(matches!(
            normalize_check_run(completed_while_queued)
                .expect_err("queued check run with a completion time must be unrepresentable"),
            ProviderError::Unrepresentable { fact, .. }
                if fact.contains("is queued and has a completion time")
        ));
    }
    #[test]
    fn every_publishable_conclusion_maps_to_a_value_github_accepts_from_a_client() {
        for (publishable, expected) in [
            (PublishedCheckConclusion::Success, "success"),
            (PublishedCheckConclusion::Failure, "failure"),
            (PublishedCheckConclusion::Neutral, "neutral"),
            (PublishedCheckConclusion::Cancelled, "cancelled"),
            (PublishedCheckConclusion::TimedOut, "timed_out"),
            (PublishedCheckConclusion::ActionRequired, "action_required"),
            (PublishedCheckConclusion::Skipped, "skipped"),
        ] {
            let written = conclusion(&publishable);
            assert_eq!(written, expected);
            assert_ne!(
                written, "stale",
                "GitHub sets stale itself and refuses it from a client"
            );
        }
    }
    #[tokio::test]
    async fn app_only_check_uses_the_named_app_client() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let address = listener.local_addr().expect("address");
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let count = stream.read(&mut buffer).await.expect("read request");
                request.extend_from_slice(&buffer[..count]);
                if count == 0 || String::from_utf8_lossy(&request).contains("\r\n\r\n{") {
                    break;
                }
            }
            stream
                .write_all(b"HTTP/1.1 201 Created\r\ncontent-type: application/json\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}")
                .await
                .expect("write response");
            sender.send(String::from_utf8(request).expect("UTF-8")).ok();
        });
        let client = octocrab::Octocrab::builder()
            .base_uri(format!("http://{address}"))
            .expect("base URI")
            .personal_token("app-installation-token")
            .build()
            .expect("client");
        let provider = GithubProvider {
            user: None,
            streaming_user: None,
            apps: BTreeMap::from([(
                "automation".to_owned(),
                ConfiguredApp {
                    app_id: 12,
                    read: Arc::new(client.clone()),
                    write: Arc::new(client),
                    source: ConfigurationSource::Direct,
                },
            )]),
        };
        let repository = Repository::new("civitas-forge", "interprex-sandbox").expect("repository");
        provider
            .publish_check(
                &repository,
                "automation",
                &CheckOutcome {
                    name: "reviewer".to_owned(),
                    head_sha: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
                    conclusion: PublishedCheckConclusion::Success,
                    summary: "settled".to_owned(),
                },
            )
            .await
            .expect("publish check");
        let request = receiver.await.expect("captured request");
        assert!(request.starts_with("POST /repos/civitas-forge/interprex-sandbox/check-runs "));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer app-installation-token")
        );
        assert!(request.contains("\"conclusion\":\"success\""));
    }
}
