//! Sparse, opt-in checks against `faictor/postel-sandbox`.
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
use postel::{
    CodeHostingProvider, CodeReviewNumber, CodeReviewsProvider, IssuesProvider, Repository,
    ReviewLocation, ReviewTarget,
};
use postel_github::{GithubConfig, from_config};
use secrecy::SecretString;

const DEFAULT_REPOSITORY: &str = "faictor/postel-sandbox";
const DEFAULT_INTERVAL_SECONDS: u64 = 3;

struct GlobalThrottle {
    file: File,
}

impl GlobalThrottle {
    fn acquire() -> Self {
        let path = std::env::var_os("POSTEL_E2E_THROTTLE_FILE")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("postel-live-github.throttle"));
        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path)
            .expect("open global throttle file");
        file.lock_exclusive().expect("lock global throttle file");
        let interval = Duration::from_secs(
            std::env::var("POSTEL_E2E_INTERVAL_SECONDS")
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

fn live_provider() -> (postel_github::GithubProvider, Repository) {
    assert_eq!(
        std::env::var("POSTEL_LIVE_GITHUB").as_deref(),
        Ok("1"),
        "set POSTEL_LIVE_GITHUB=1 to acknowledge a real GitHub API test"
    );
    let token = std::env::var("POSTEL_E2E_GH_TOKEN")
        .expect("POSTEL_E2E_GH_TOKEN must contain the sandbox read token");
    let repository = std::env::var("POSTEL_E2E_REPOSITORY")
        .unwrap_or_else(|_| DEFAULT_REPOSITORY.to_owned())
        .parse()
        .expect("POSTEL_E2E_REPOSITORY must be owner/name");
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
async fn configured_code_review_observation_matches_current_provider_data() {
    let (provider, repository) = live_provider();
    let number = std::env::var("POSTEL_E2E_CODE_REVIEW_NUMBER")
        .expect("POSTEL_E2E_CODE_REVIEW_NUMBER must name an existing code review")
        .parse()
        .expect("POSTEL_E2E_CODE_REVIEW_NUMBER must be a positive integer");
    let number = CodeReviewNumber::new(number).expect("positive code review number");

    let _throttle = GlobalThrottle::acquire();
    let review = provider
        .code_review(&repository, number)
        .await
        .expect("read configured code review");

    assert_eq!(review.number, number);
    assert!(!review.change.base_sha.is_empty());
    assert!(!review.change.head_sha.is_empty());
    assert!(!review.reviews.is_empty());
    assert!(!review.author.id.as_str().is_empty());
    assert!(
        review
            .reviews
            .iter()
            .all(|item| !item.author.id.as_str().is_empty())
    );
    assert!(
        review
            .reviews
            .iter()
            .all(|item| match item.relationship_to_change {
                postel::ReviewRelationship::ChangeAuthor => item.author.id == review.author.id,
                postel::ReviewRelationship::Other => item.author.id != review.author.id,
                postel::ReviewRelationship::Unknown => true,
            })
    );
    assert!(
        review
            .reviews
            .iter()
            .all(|submitted| !submitted.revision.head_sha.is_empty())
    );
    for thread in review
        .reviews
        .iter()
        .flat_map(|submitted| submitted.findings.iter())
        .chain(review.discussions.iter())
    {
        assert!(!thread.id.as_str().is_empty());
        assert!(!thread.comment.id.as_str().is_empty());
        match &thread.location {
            ReviewLocation::File { path } => assert!(!path.is_empty()),
            ReviewLocation::Lines {
                path,
                original,
                current,
                ..
            } => {
                assert!(!path.is_empty());
                assert!(original.end.get() > 0);
                assert!(current.as_ref().is_none_or(|range| range.end.get() > 0));
            }
        }
    }
    for comment in &review.conversation {
        assert!(!comment.id.as_str().is_empty());
        assert!(!comment.author.id.as_str().is_empty());
    }
    for request in &review.outstanding_requests {
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
    let author_review_count = review
        .reviews
        .iter()
        .filter(|item| item.relationship_to_change == postel::ReviewRelationship::ChangeAuthor)
        .count();
    let other_review_count = review
        .reviews
        .iter()
        .filter(|item| item.relationship_to_change == postel::ReviewRelationship::Other)
        .count();
    let unknown_review_count = review
        .reviews
        .iter()
        .filter(|item| item.relationship_to_change == postel::ReviewRelationship::Unknown)
        .count();
    let draft_review_count = review
        .reviews
        .iter()
        .filter(|item| item.state == postel::ReviewState::Draft)
        .count();
    eprintln!(
        "code review {}: {} reviews ({} author, {} other, {} unknown, {} draft), {} findings, {} discussions, {} conversation comments, {} outstanding requests",
        number.get(),
        review.reviews.len(),
        author_review_count,
        other_review_count,
        unknown_review_count,
        draft_review_count,
        review
            .reviews
            .iter()
            .map(|submitted| submitted.findings.len())
            .sum::<usize>(),
        review.discussions.len(),
        review.conversation.len(),
        review.outstanding_requests.len()
    );
}
