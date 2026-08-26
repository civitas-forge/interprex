use bytes::Bytes;
use futures_util::{TryStreamExt, stream};
use interprex::{
    AssetStreamError, AssetUpload, ChangeRequest, ChangeRequestNumber, CodeHostingProvider,
    CodeReviewsProvider, CommitRange, FindingResolution, FindingResolutionReason, FindingSeverity,
    OpenClosed, Release, ReleaseId, ReleasesProvider, Repository, RepositoryFacts,
    RepositorySettings, Review, ReviewActor, ReviewActorId, ReviewActorKind, ReviewAnchor,
    ReviewAuthor, ReviewComment, ReviewCommentId, ReviewDiffSide, ReviewDisposition, ReviewId,
    ReviewLine, ReviewLineRange, ReviewLocation, ReviewRequestTarget, ReviewState, ReviewThread,
    ReviewThreadId, ReviewThreadStatus, ReviewedRevision,
};

use crate::FakeProvider;

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
                number,
                title: "Review requests".to_owned(),
                state: OpenClosed::Open,
                draft: true,
                commit_range: CommitRange {
                    base_sha: "base".to_owned(),
                    head_sha: "head".to_owned(),
                },
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
    let other_request_ids = provider
        .change_request(&repository, other_number)
        .await
        .expect("other change request")
        .outstanding_requests
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
        state: OpenClosed::Open,
        draft: false,
        commit_range: range.clone(),
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
            findings: vec![ReviewThread {
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
                resolution: None,
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
            resolution: None,
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
    provider
        .resolve_finding(
            &repository,
            number,
            &ReviewThreadId::new("thread-1").expect("thread id"),
            resolution,
            "Addressed by validating the range before indexing.",
        )
        .await
        .expect("resolve finding");
    let observed = provider
        .change_request(&repository, number)
        .await
        .expect("review");
    let finding = &observed.reviews[0].findings[0];
    assert_eq!(finding.status, ReviewThreadStatus::Resolved);
    assert_eq!(finding.resolution, Some(resolution));
    assert_eq!(
        finding.replies.last().map(|comment| comment.body.as_str()),
        Some("Addressed by validating the range before indexing.")
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
