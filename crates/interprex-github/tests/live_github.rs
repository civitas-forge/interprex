//! Sparse, opt-in checks against `civitas-forge/interprex-sandbox`.
//!
//! The test is read-only so concurrent branches cannot corrupt shared state.
//! Every request takes a machine-global file lock and observes a minimum delay;
//! the workflow adds repository-global GitHub Actions concurrency across
//! branches and runs. Octocrab's rate-limit-aware retry remains enabled above
//! both controls.

use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use fs2::FileExt;
use interprex::{
    ChangeRequestNumber, CodeHostingProvider, CodeReviewsProvider, IssuesProvider, Repository,
    ReviewAnchor, ReviewAuthor, ReviewTarget, ReviewThreadStatus,
};
use interprex_github::{GithubConfig, from_config};
use secrecy::SecretString;

const DEFAULT_REPOSITORY: &str = "civitas-forge/interprex-sandbox";
const DEFAULT_INTERVAL_SECONDS: u64 = 3;

struct GlobalThrottle {
    file: File,
}

impl GlobalThrottle {
    fn acquire() -> Self {
        let path = std::env::var_os("INTERPREX_E2E_THROTTLE_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("interprex-live-github.throttle"));
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)
            .expect("open global throttle file");
        file.lock_exclusive().expect("lock global throttle file");
        let interval = Duration::from_secs(
            std::env::var("INTERPREX_E2E_INTERVAL_SECONDS")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(DEFAULT_INTERVAL_SECONDS),
        );
        let mut previous = String::new();
        file.read_to_string(&mut previous)
            .expect("read throttle timestamp");
        let previous = previous.trim().parse::<u128>().unwrap_or(0);
        let now = now_millis();
        let remaining = interval
            .checked_sub(Duration::from_millis((now.saturating_sub(previous)) as u64))
            .unwrap_or_default();
        if !remaining.is_zero() {
            std::thread::sleep(remaining);
        }
        Self { file }
    }
}

impl Drop for GlobalThrottle {
    fn drop(&mut self) {
        self.file.set_len(0).expect("truncate throttle timestamp");
        self.file
            .seek(SeekFrom::Start(0))
            .expect("seek throttle timestamp");
        write!(self.file, "{}", now_millis()).expect("write throttle timestamp");
        self.file.sync_all().expect("sync throttle timestamp");
        FileExt::unlock(&self.file).expect("unlock global throttle file");
    }
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock after Unix epoch")
        .as_millis()
}

fn live_provider() -> (interprex_github::GithubProvider, Repository) {
    assert_eq!(
        std::env::var("INTERPREX_LIVE_GITHUB").as_deref(),
        Ok("1"),
        "set INTERPREX_LIVE_GITHUB=1 to acknowledge a real GitHub API test"
    );
    let token = std::env::var("INTERPREX_E2E_GH_TOKEN")
        .expect("INTERPREX_E2E_GH_TOKEN must contain the sandbox read token");
    let repository = std::env::var("INTERPREX_E2E_REPOSITORY")
        .unwrap_or_else(|_| DEFAULT_REPOSITORY.to_owned())
        .parse()
        .expect("INTERPREX_E2E_REPOSITORY must be owner/name");
    let provider = from_config(GithubConfig {
        gh_token: Some(SecretString::from(token)),
        ..GithubConfig::default()
    })
    .expect("construct provider without network access");
    (provider, repository)
}

#[tokio::test]
#[ignore = "contacts the real GitHub API; run only through the serialized live workflow"]
async fn sandbox_repository_and_label_reads_follow_the_real_consumer_path() {
    let (provider, repository) = live_provider();

    let _throttle = GlobalThrottle::acquire();
    let facts = provider
        .repository(&repository)
        .await
        .expect("read sandbox repository");
    assert_eq!(facts.repository, repository);
    assert!(!facts.default_branch.is_empty());
    drop(_throttle);

    let _throttle = GlobalThrottle::acquire();
    let labels = provider
        .labels(&repository)
        .await
        .expect("read sandbox labels");
    assert!(labels.iter().all(|label| !label.name.is_empty()));
}

#[tokio::test]
#[ignore = "contacts the real GitHub API; run only through the serialized live workflow"]
async fn configured_change_request_observation_matches_current_provider_data() {
    let (provider, repository) = live_provider();
    let number = std::env::var("INTERPREX_E2E_CODE_REVIEW_NUMBER")
        .expect("INTERPREX_E2E_CODE_REVIEW_NUMBER must name an existing change request")
        .parse()
        .expect("INTERPREX_E2E_CODE_REVIEW_NUMBER must be a positive integer");
    let number = ChangeRequestNumber::new(number).expect("positive change request number");

    let _throttle = GlobalThrottle::acquire();
    let change_request = provider
        .change_request(&repository, number)
        .await
        .expect("read configured change request");

    assert_eq!(change_request.number, number);
    assert!(!change_request.commits.base_sha.is_empty());
    assert!(!change_request.commits.head_sha.is_empty());
    assert!(!change_request.reviews.is_empty());
    assert!(!change_request.author.id.as_str().is_empty());
    assert!(change_request.reviews.iter().all(|item| {
        !item
            .author
            .actor(&change_request.author)
            .id
            .as_str()
            .is_empty()
    }));
    assert!(
        change_request
            .reviews
            .iter()
            .all(|item| match &item.author {
                ReviewAuthor::ChangeAuthor => true,
                ReviewAuthor::Other(actor) => actor.id != change_request.author.id,
                ReviewAuthor::Unknown(_) => true,
            })
    );
    assert!(
        change_request
            .reviews
            .iter()
            .all(|submitted| !submitted.revision.head_sha.is_empty())
    );
    for thread in change_request
        .reviews
        .iter()
        .flat_map(|submitted| submitted.findings.iter())
        .chain(change_request.standalone_threads.iter())
    {
        assert!(!thread.id.as_str().is_empty());
        assert!(!thread.comment.id.as_str().is_empty());
        assert!(!thread.location.path.is_empty());
        match &thread.location.anchor {
            ReviewAnchor::File => {}
            ReviewAnchor::Lines {
                original, current, ..
            } => {
                assert!(original.end.get() > 0);
                assert!(current.as_ref().is_none_or(|range| range.end.get() > 0));
            }
        }
    }
    for comment in &change_request.unanchored_comments {
        assert!(!comment.id.as_str().is_empty());
        assert!(!comment.author.id.as_str().is_empty());
    }
    for request in &change_request.outstanding_requests {
        assert!(!request.id.as_str().is_empty());
        match &request.target {
            ReviewTarget::Actor(actor) => assert!(!actor.login.is_empty()),
            ReviewTarget::Team(team) => {
                assert!(!team.id.as_str().is_empty());
                assert!(!team.slug.is_empty());
                assert!(!team.name.is_empty());
            }
            ReviewTarget::Unavailable => {}
        }
    }
    let author_review_count = change_request
        .reviews
        .iter()
        .filter(|item| item.author.relationship() == interprex::ReviewRelationship::ChangeAuthor)
        .count();
    let other_review_count = change_request
        .reviews
        .iter()
        .filter(|item| item.author.relationship() == interprex::ReviewRelationship::Other)
        .count();
    let unknown_review_count = change_request
        .reviews
        .iter()
        .filter(|item| item.author.relationship() == interprex::ReviewRelationship::Unknown)
        .count();
    let draft_review_count = change_request
        .reviews
        .iter()
        .filter(|item| item.state == interprex::ReviewState::Draft)
        .count();
    let findings = change_request
        .reviews
        .iter()
        .flat_map(|item| item.findings.iter())
        .chain(change_request.standalone_threads.iter())
        .collect::<Vec<_>>();
    let open_finding_count = findings
        .iter()
        .filter(|thread| thread.status == ReviewThreadStatus::Open)
        .count();
    let resolved_finding_count = findings
        .iter()
        .filter(|thread| thread.status == ReviewThreadStatus::Resolved)
        .count();
    assert_eq!(open_finding_count + resolved_finding_count, findings.len());
    eprintln!(
        "change request {}: {} reviews ({} author, {} other, {} unknown, {} draft), {} review threads ({} open, {} resolved), {} stand-alone threads, {} unanchored comments, {} outstanding requests",
        number.get(),
        change_request.reviews.len(),
        author_review_count,
        other_review_count,
        unknown_review_count,
        draft_review_count,
        findings.len(),
        open_finding_count,
        resolved_finding_count,
        change_request.standalone_threads.len(),
        change_request.unanchored_comments.len(),
        change_request.outstanding_requests.len()
    );
}
