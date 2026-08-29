use bytes::Bytes;
use futures_util::{TryStreamExt, stream};
use interprex::{
    AssetStreamError, AssetUpload, ChangeRequest, ChangeRequestHead, ChangeRequestNumber,
    ChangeRequestState, CheckConclusion, CheckRun, CheckStatus, CodeHostingProvider,
    CodeReviewsProvider, CommitRange, FindingResolution, FindingResolutionReason,
    FindingResolutionRecord, FindingResolutionReply, FindingSeverity, Mergeability, ProviderApp,
    ProviderAppId, ProviderError, Release, ReleaseId, ReleasesProvider, Repository,
    RepositoryFacts, RepositorySettings, Review, ReviewActor, ReviewActorId, ReviewActorKind,
    ReviewAnchor, ReviewAuthor, ReviewComment, ReviewCommentId, ReviewDiffSide, ReviewDisposition,
    ReviewFinding, ReviewId, ReviewLine, ReviewLineRange, ReviewLocation, ReviewRequestTarget,
    ReviewRequestTargetInspection, ReviewState, ReviewTarget, ReviewTargetsProvider, ReviewThread,
    ReviewThreadId, ReviewThreadStatus, ReviewedRevision, ReviewerApplication,
    ReviewerApplicationsProvider,
};

use crate::FakeProvider;

/// One open change request with no review data, for tests that care about a
/// few fields and not the rest. Adding a `ChangeRequest` field lands here
/// rather than at every seed in this file.
fn head(repository: &Repository, head_ref: &str) -> ChangeRequestHead {
    ChangeRequestHead::new(repository.clone(), head_ref).expect("head")
}

fn change_request(
    number: ChangeRequestNumber,
    title: &str,
    head: ChangeRequestHead,
) -> ChangeRequest {
    ChangeRequest {
        number,
        title: title.to_owned(),
        state: ChangeRequestState::Open,
        draft: false,
        commit_range: CommitRange {
            base_sha: "base".to_owned(),
            head_sha: "head".to_owned(),
        },
        base_branch: "main".to_owned(),
        head: Some(head),
        mergeability: Mergeability::Unknown,
        author: ReviewActor {
            id: ReviewActorId::new("actor-author").expect("actor id"),
            login: "author".to_owned(),
            kind: ReviewActorKind::User,
        },
        updated_at: "2026-08-25T10:00:00Z".parse().expect("timestamp"),
        reviews: Vec::new(),
        standalone_threads: Vec::new(),
        unanchored_comments: Vec::new(),
        outstanding_requests: Vec::new(),
    }
}

#[tokio::test]
async fn consumer_observes_changes_through_the_same_contract() {
    let provider = FakeProvider::new();
    let repository = Repository::new("civitas-forge", "sandbox").expect("repository");
    provider
        .seed_repository(
            RepositoryFacts {
                repository: repository.clone(),
                default_branch: "main".to_owned(),
                private: true,
                archived: false,
            },
            RepositorySettings {
                allow_squash_merge: true,
                allow_merge_commit: false,
                allow_rebase_merge: false,
                delete_branch_on_merge: true,
            },
        )
        .await;
    assert!(
        provider
            .settings(&repository)
            .await
            .expect("settings")
            .allow_squash_merge
    );

    let number = ChangeRequestNumber::new(3).expect("number");
    provider
        .seed_change_request(
            repository.clone(),
            ChangeRequest {
                draft: true,
                ..change_request(
                    number,
                    "Review requests",
                    head(&repository, "refs/heads/review-requests"),
                )
            },
        )
        .await;
    let targets = vec![
        ReviewRequestTarget::User("reviewer".to_owned()),
        ReviewRequestTarget::Team("civitas-forge/maintainers".to_owned()),
    ];
    provider
        .request_reviewers(&repository, number, &targets)
        .await
        .expect("request reviewers");
    provider
        .request_reviewers(&repository, number, &targets)
        .await
        .expect("requesting the same reviewers is idempotent");
    let observed = provider
        .change_request(&repository, number)
        .await
        .expect("read requested reviewers");
    assert_eq!(
        observed
            .outstanding_requests
            .iter()
            .filter_map(|request| request.request_target.clone())
            .collect::<Vec<_>>(),
        targets
    );

    let request_times = observed
        .outstanding_requests
        .iter()
        .map(|request| request.requested_at.expect("recorded request time"))
        .collect::<Vec<_>>();
    assert!(
        request_times[0] > observed.updated_at,
        "a request is made after the observation that provoked it"
    );
    assert!(
        request_times[1] > request_times[0],
        "requests made in one call remain ordered by when they were made"
    );

    let first_request_ids = observed
        .outstanding_requests
        .iter()
        .map(|request| request.id.clone())
        .collect::<Vec<_>>();
    let other_number = ChangeRequestNumber::new(4).expect("number");
    let mut other_change_request = observed.clone();
    other_change_request.number = other_number;
    other_change_request.outstanding_requests.clear();
    provider
        .seed_change_request(repository.clone(), other_change_request)
        .await;
    provider
        .request_reviewers(&repository, other_number, &targets)
        .await
        .expect("request same reviewers on another change request");
    let other_requests = provider
        .change_request(&repository, other_number)
        .await
        .expect("other change request")
        .outstanding_requests;
    assert_eq!(
        other_requests
            .iter()
            .map(|request| request.requested_at.expect("recorded request time"))
            .collect::<Vec<_>>(),
        request_times,
        "the fake reads request times from the seeded observation, not the clock"
    );
    let other_request_ids = other_requests
        .into_iter()
        .map(|request| request.id)
        .collect::<Vec<_>>();
    assert!(
        first_request_ids
            .iter()
            .all(|id| !other_request_ids.contains(id)),
        "fake review request ids must be scoped to their change request"
    );

    let other_repository = Repository::new("other", "sandbox").expect("repository");
    let mut other_repository_review = observed;
    other_repository_review.outstanding_requests.clear();
    provider
        .seed_change_request(other_repository.clone(), other_repository_review)
        .await;
    provider
        .request_reviewers(&other_repository, number, &targets)
        .await
        .expect("request same reviewers in another repository");
    let other_repository_ids = provider
        .change_request(&other_repository, number)
        .await
        .expect("other repository change request")
        .outstanding_requests
        .into_iter()
        .map(|request| request.id)
        .collect::<Vec<_>>();
    assert!(
        first_request_ids
            .iter()
            .all(|id| !other_repository_ids.contains(id)),
        "fake review request ids must be scoped to their repository"
    );
}

#[tokio::test]
async fn consumer_inspects_only_explicitly_seeded_review_targets() {
    let provider = FakeProvider::new();
    let repository = Repository::new("civitas-forge", "sandbox").expect("repository");
    let configured = ReviewRequestTarget::User("review-bot".to_owned());
    let observed = ReviewTarget::Actor(ReviewActor {
        id: ReviewActorId::new("actor-review-bot").expect("actor id"),
        login: "review-bot[bot]".to_owned(),
        kind: ReviewActorKind::Bot,
    });
    provider
        .seed_review_request_target(repository.clone(), configured.clone(), observed.clone())
        .await;

    assert_eq!(
        provider
            .inspect_review_request_target(&repository, &configured)
            .await
            .expect("seeded target inspection"),
        ReviewRequestTargetInspection::KindMismatch(observed)
    );
    assert_eq!(
        provider
            .inspect_review_request_target(
                &repository,
                &ReviewRequestTarget::Bot("review-bot".to_owned()),
            )
            .await
            .expect("unseeded target inspection"),
        ReviewRequestTargetInspection::NotResolvable,
        "the fake must not invent a bot observation from a bot request target"
    );
}

#[tokio::test]
async fn consumer_resolves_only_explicitly_seeded_reviewer_applications() {
    let provider = FakeProvider::new();
    let repository = Repository::new("civitas-forge", "sandbox").expect("repository");
    let application = ReviewerApplication::new(
        ProviderApp {
            id: ProviderAppId::new("4111233").expect("app id"),
            slug: "adr-codex-review".to_owned(),
            name: "ADR Codex Review".to_owned(),
        },
        ReviewActor {
            id: ReviewActorId::new("BOT_kgDOEZ_BKw").expect("actor id"),
            login: "adr-codex-review[bot]".to_owned(),
            kind: ReviewActorKind::Bot,
        },
    )
    .expect("reviewer application");
    provider
        .seed_reviewer_application(
            repository.clone(),
            "configured-reviewer".to_owned(),
            application.clone(),
        )
        .await;

    assert_eq!(
        provider
            .resolve_reviewer_application(&repository, "configured-reviewer")
            .await
            .expect("seeded application"),
        application
    );
    assert!(matches!(
        provider
            .resolve_reviewer_application(&repository, "adr-codex-review")
            .await,
        Err(ProviderError::NotFound { .. })
    ));
}

#[tokio::test]
async fn consumer_reads_complete_review_threads_through_the_contract() {
    let provider = FakeProvider::new();
    let repository = Repository::new("civitas-forge", "sandbox").expect("repository");
    let number = ChangeRequestNumber::new(3).expect("number");
    let reviewer = ReviewActor {
        id: ReviewActorId::new("actor-reviewer").expect("actor id"),
        login: "reviewer".to_owned(),
        kind: ReviewActorKind::Bot,
    };
    let author = ReviewActor {
        id: ReviewActorId::new("actor-author").expect("actor id"),
        login: "author".to_owned(),
        kind: ReviewActorKind::User,
    };
    let range = CommitRange {
        base_sha: "base".to_owned(),
        head_sha: "revision-1".to_owned(),
    };
    let change_request = ChangeRequest {
        number,
        title: "Review threads".to_owned(),
        state: ChangeRequestState::Open,
        draft: false,
        commit_range: range.clone(),
        base_branch: "main".to_owned(),
        head: Some(head(&repository, "refs/heads/review-threads")),
        mergeability: Mergeability::Conflicted,
        author: author.clone(),
        updated_at: "2026-08-25T10:00:00Z".parse().expect("timestamp"),
        reviews: vec![Review {
            id: ReviewId::new("review-1").expect("review id"),
            author: ReviewAuthor::Other(reviewer.clone()),
            via_app: None,
            revision: ReviewedRevision {
                head_sha: range.head_sha.clone(),
            },
            state: ReviewState::Submitted {
                disposition: ReviewDisposition::ChangesRequested,
                submitted_at: "2026-08-25T09:00:00Z".parse().expect("timestamp"),
            },
            summary: Some("One concern".to_owned()),
            findings: vec![ReviewFinding {
                thread: ReviewThread {
                    id: ReviewThreadId::new("thread-1").expect("thread id"),
                    location: ReviewLocation {
                        path: "src/lib.rs".to_owned(),
                        anchor: ReviewAnchor::Lines {
                            side: ReviewDiffSide::Right,
                            original: ReviewLineRange {
                                start: None,
                                end: ReviewLine::new(10).expect("line"),
                            },
                            current: Some(ReviewLineRange {
                                start: None,
                                end: ReviewLine::new(10).expect("line"),
                            }),
                        },
                    },
                    outdated: false,
                    status: ReviewThreadStatus::Open,
                    comment: ReviewComment {
                        id: ReviewCommentId::new("comment-1").expect("comment id"),
                        author: reviewer.clone(),
                        body: "question".to_owned(),
                        created_at: "2026-08-25T09:00:00Z".parse().expect("timestamp"),
                        updated_at: Some("2026-08-25T09:00:00Z".parse().expect("timestamp")),
                    },
                    replies: vec![ReviewComment {
                        id: ReviewCommentId::new("comment-2").expect("comment id"),
                        author: author.clone(),
                        body: "answer".to_owned(),
                        created_at: "2026-08-25T09:30:00Z".parse().expect("timestamp"),
                        updated_at: Some("2026-08-25T09:30:00Z".parse().expect("timestamp")),
                    }],
                },
                resolution: None,
            }],
        }],
        standalone_threads: vec![ReviewThread {
            id: ReviewThreadId::new("thread-2").expect("thread id"),
            location: ReviewLocation {
                path: "README.lex".to_owned(),
                anchor: ReviewAnchor::File,
            },
            outdated: false,
            status: ReviewThreadStatus::Open,
            comment: ReviewComment {
                id: ReviewCommentId::new("comment-3").expect("comment id"),
                author: author.clone(),
                body: "Can we clarify this?".to_owned(),
                created_at: "2026-08-25T09:10:00Z".parse().expect("timestamp"),
                updated_at: Some("2026-08-25T09:10:00Z".parse().expect("timestamp")),
            },
            replies: vec![ReviewComment {
                id: ReviewCommentId::new("comment-4").expect("comment id"),
                author: reviewer,
                body: "Yes".to_owned(),
                created_at: "2026-08-25T09:20:00Z".parse().expect("timestamp"),
                updated_at: Some("2026-08-25T09:20:00Z".parse().expect("timestamp")),
            }],
        }],
        unanchored_comments: vec![ReviewComment {
            id: ReviewCommentId::new("comment-5").expect("comment id"),
            author,
            body: "Ready for review".to_owned(),
            created_at: "2026-08-25T08:50:00Z".parse().expect("timestamp"),
            updated_at: Some("2026-08-25T08:50:00Z".parse().expect("timestamp")),
        }],
        outstanding_requests: Vec::new(),
    };
    provider
        .seed_change_request(repository.clone(), change_request.clone())
        .await;

    assert_eq!(
        provider
            .change_request(&repository, number)
            .await
            .expect("review"),
        change_request
    );
    let resolution = FindingResolution {
        reason: FindingResolutionReason::Addressed,
        addressing_severity: FindingSeverity::Major,
    };
    let explanation =
        FindingResolutionReply::new("Addressed by validating the range before indexing.")
            .expect("resolution explanation");
    provider
        .resolve_finding(
            &repository,
            number,
            &ReviewThreadId::new("thread-1").expect("thread id"),
            resolution,
            &explanation,
        )
        .await
        .expect("resolve finding");
    let observed = provider
        .change_request(&repository, number)
        .await
        .expect("review");
    let finding = &observed.reviews[0].findings[0];
    assert_eq!(finding.status, ReviewThreadStatus::Resolved);
    let record = finding.resolution.as_ref().expect("resolution record");
    assert_eq!(record.supported_resolution(), Some(resolution));
    assert_eq!(
        record.source_reply_id(),
        &finding.replies.last().expect("reply").id
    );
    let resolution_reply = finding.resolution_reply().expect("linked resolution reply");
    assert_eq!(resolution_reply.author.login, "fake-provider");
    assert!(resolution_reply.created_at > change_request.updated_at);
    assert_eq!(
        finding.replies.last().map(|comment| comment.body.as_str()),
        Some("Addressed by validating the range before indexing.")
    );

    let reply_count = finding.replies.len();
    provider
        .resolve_finding(
            &repository,
            number,
            &ReviewThreadId::new("thread-1").expect("thread id"),
            resolution,
            &FindingResolutionReply::new("A retry must not replace the recorded explanation.")
                .expect("resolution explanation"),
        )
        .await
        .expect("repeat identical resolution");
    let repeated = provider
        .change_request(&repository, number)
        .await
        .expect("review after retry");
    assert_eq!(repeated.reviews[0].findings[0].replies.len(), reply_count);

    let mut partial_write = repeated;
    partial_write.reviews[0].findings[0].status = ReviewThreadStatus::Open;
    provider
        .seed_change_request(repository.clone(), partial_write)
        .await;
    provider
        .resolve_finding(
            &repository,
            number,
            &ReviewThreadId::new("thread-1").expect("thread id"),
            resolution,
            &FindingResolutionReply::new(
                "A partial-write retry only resolves the existing thread.",
            )
            .expect("resolution explanation"),
        )
        .await
        .expect("recover partial resolution write");
    let recovered = provider
        .change_request(&repository, number)
        .await
        .expect("review after partial-write retry");
    assert_eq!(
        recovered.reviews[0].findings[0].status,
        ReviewThreadStatus::Resolved
    );
    assert_eq!(recovered.reviews[0].findings[0].replies.len(), reply_count);

    let mut unsupported = recovered.clone();
    let source_reply_id = unsupported.reviews[0].findings[0]
        .replies
        .last()
        .expect("resolution reply")
        .id
        .clone();
    unsupported.reviews[0].findings[0].resolution = Some(FindingResolutionRecord::Unsupported {
        metadata_format: "future:test-format".to_owned(),
        source_reply_id,
    });
    provider
        .seed_change_request(repository.clone(), unsupported)
        .await;

    let replacement = FindingResolution {
        reason: FindingResolutionReason::WontFix,
        addressing_severity: FindingSeverity::Minor,
    };
    let replacement_reply = FindingResolutionReply::new("The compatibility cost is not justified.")
        .expect("resolution explanation");
    let error = provider
        .resolve_finding(
            &repository,
            number,
            &ReviewThreadId::new("thread-1").expect("thread id"),
            replacement,
            &replacement_reply,
        )
        .await
        .expect_err("unsupported resolution format must not be overwritten");
    assert!(error.to_string().contains("future:test-format"));
    provider
        .seed_change_request(repository.clone(), recovered)
        .await;
    provider
        .resolve_finding(
            &repository,
            number,
            &ReviewThreadId::new("thread-1").expect("thread id"),
            replacement,
            &replacement_reply,
        )
        .await
        .expect("record changed resolution");
    let replaced = provider
        .change_request(&repository, number)
        .await
        .expect("review after changed resolution");
    let finding = &replaced.reviews[0].findings[0];
    assert_eq!(finding.replies.len(), reply_count + 1);
    assert_eq!(
        finding
            .resolution
            .as_ref()
            .and_then(interprex::FindingResolutionRecord::supported_resolution),
        Some(replacement)
    );
    assert_eq!(
        finding.replies[finding.replies.len() - 2].body,
        "Addressed by validating the range before indexing."
    );
}

#[tokio::test]
async fn consumer_observes_seeded_checks_per_commit_through_the_contract() {
    let provider = FakeProvider::new();
    let repository = Repository::new("civitas-forge", "sandbox").expect("repository");
    assert!(
        provider
            .checks(&repository, "head")
            .await
            .expect("checks")
            .is_empty(),
        "a commit with no seeded checks has none"
    );

    let runs = vec![
        CheckRun {
            name: "quality".to_owned(),
            head_sha: "head".to_owned(),
            via_app: Some(ProviderApp {
                id: ProviderAppId::new("1042").expect("app id"),
                slug: "quality-app".to_owned(),
                name: "Quality App".to_owned(),
            }),
            status: CheckStatus::Completed {
                conclusion: CheckConclusion::Failure,
                completed_at: "2026-08-25T09:40:00Z".parse().expect("timestamp"),
            },
            summary: Some("clippy reported one warning".to_owned()),
            html_url: Some("https://github.invalid/runs/1".to_owned()),
        },
        CheckRun {
            name: "integration".to_owned(),
            head_sha: "head".to_owned(),
            via_app: None,
            status: CheckStatus::InProgress,
            summary: None,
            html_url: None,
        },
        CheckRun {
            name: "quality".to_owned(),
            head_sha: "other-head".to_owned(),
            via_app: None,
            status: CheckStatus::Queued,
            summary: None,
            html_url: None,
        },
    ];
    provider
        .seed_check_runs(repository.clone(), runs.clone())
        .await;

    assert_eq!(
        provider.checks(&repository, "head").await.expect("checks"),
        runs[..2],
        "a commit observes the runs that name it"
    );
    assert_eq!(
        provider
            .checks(&repository, "other-head")
            .await
            .expect("checks"),
        runs[2..],
        "a run seeded in the same call belongs to the commit it names"
    );
    assert!(
        provider
            .checks(
                &Repository::new("other", "sandbox").expect("repository"),
                "head"
            )
            .await
            .expect("checks")
            .is_empty(),
        "checks belong to the repository they were seeded in"
    );
}

#[tokio::test]
async fn consumer_streams_release_assets_through_the_contract() {
    let provider = FakeProvider::new();
    let repository = Repository::new("civitas-forge", "sandbox").expect("repository");
    let release_id = ReleaseId::new(1).expect("release id");
    provider
        .seed_release(
            repository.clone(),
            Release {
                id: release_id,
                tag: "v0.1.0".to_owned(),
                name: None,
                body: None,
                draft: true,
                prerelease: false,
                assets: Vec::new(),
            },
        )
        .await;
    let upload = AssetUpload::new(
        11,
        stream::iter([
            Ok::<_, AssetStreamError>(Bytes::from_static(b"hello ")),
            Ok(Bytes::from_static(b"world")),
        ]),
    );
    let asset = provider
        .upload_asset(&repository, release_id, "interprex.tar.gz", None, upload)
        .await
        .expect("upload asset");
    let chunks: Vec<Bytes> = provider
        .download_asset(&repository, asset.id)
        .await
        .expect("download stream")
        .try_collect()
        .await
        .expect("download chunks");

    assert_eq!(
        chunks,
        [Bytes::from_static(b"hello "), Bytes::from_static(b"world")]
    );
}

#[tokio::test]
async fn fake_lists_every_open_change_request_proposing_a_head() {
    let provider = FakeProvider::new();
    let target = Repository::new("civitas-forge", "sandbox").expect("repository");
    let fork = Repository::new("contributor", "sandbox").expect("repository");
    for (targeted, number, seeded_head, base_branch, state) in [
        (
            &target,
            1,
            head(&target, "refs/heads/feature"),
            "main",
            ChangeRequestState::Open,
        ),
        (
            &target,
            2,
            head(&target, "refs/heads/feature"),
            "release/1.1",
            ChangeRequestState::Open,
        ),
        (
            &target,
            3,
            head(&target, "refs/heads/feature"),
            "main",
            ChangeRequestState::Closed,
        ),
        (
            &target,
            7,
            head(&target, "refs/heads/feature"),
            "main",
            ChangeRequestState::Merged {
                merged_at: "2026-08-24T10:00:00Z".parse().expect("timestamp"),
            },
        ),
        (
            &target,
            4,
            head(&target, "refs/heads/release/1.1"),
            "main",
            ChangeRequestState::Open,
        ),
        (
            &target,
            5,
            head(&fork, "refs/heads/feature"),
            "main",
            ChangeRequestState::Open,
        ),
        (
            &fork,
            6,
            head(&fork, "refs/heads/feature"),
            "main",
            ChangeRequestState::Open,
        ),
    ] {
        let number = ChangeRequestNumber::new(number).expect("number");
        provider
            .seed_change_request(
                targeted.clone(),
                ChangeRequest {
                    state,
                    base_branch: base_branch.to_owned(),
                    ..change_request(
                        number,
                        &format!("Change request {}", number.get()),
                        seeded_head,
                    )
                },
            )
            .await;
    }
    let numbers = async |targeted: &Repository, head: ChangeRequestHead| {
        provider
            .open_change_requests(targeted, &head)
            .await
            .expect("open change requests")
            .into_iter()
            .map(ChangeRequestNumber::get)
            .collect::<Vec<_>>()
    };

    assert_eq!(
        numbers(&target, head(&target, "refs/heads/no-such-branch")).await,
        Vec::<u64>::new()
    );
    assert_eq!(
        numbers(&target, head(&target, "refs/heads/release/1.1")).await,
        [4]
    );
    let both = numbers(&target, head(&target, "refs/heads/feature")).await;
    assert_eq!(
        both,
        [1, 2],
        "3 is closed, 7 is merged, and 5 proposes the fork's branch of the same name"
    );
    let mut bases = Vec::new();
    for number in both {
        let number = ChangeRequestNumber::new(number).expect("number");
        bases.push(
            provider
                .change_request(&target, number)
                .await
                .expect("observation")
                .base_branch,
        );
    }
    assert_eq!(
        bases,
        ["main", "release/1.1"],
        "two change requests proposing one branch are told apart by the branch each targets"
    );
    assert_eq!(
        numbers(&target, head(&fork, "refs/heads/feature")).await,
        [5],
        "a change request targeting this repository from a fork is found by naming the fork's head"
    );
    assert_eq!(numbers(&fork, head(&fork, "refs/heads/feature")).await, [6]);
}
