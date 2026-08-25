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
use postel::{CodeHostingProvider, IssuesProvider, Repository};
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
