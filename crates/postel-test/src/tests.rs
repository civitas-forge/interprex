use bytes::Bytes;
use futures_util::{TryStreamExt, stream};
use postel::{
    AssetStreamError, AssetUpload, CodeHostingProvider, CodeReview, CodeReviewNumber,
    CodeReviewsProvider, CommitRange, OpenClosed, Release, ReleaseId, ReleasesProvider, Repository,
    RepositoryFacts, RepositorySettings, Review, ReviewActor, ReviewActorId, ReviewActorKind,
    ReviewComment, ReviewCommentId, ReviewDiffSide, ReviewDisposition, ReviewId, ReviewLine,
    ReviewLineRange, ReviewLocation, ReviewRelationship, ReviewRequestTarget, ReviewState,
    ReviewThread, ReviewThreadId, ReviewThreadStatus, ReviewedRevision,
};

use crate::FakeProvider;

#[tokio::test]
async fn consumer_observes_changes_through_the_same_contract() {
    let provider = FakeProvider::new();
    let repository = Repository::new("faictor", "sandbox").expect("repository");
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

    let number = CodeReviewNumber::new(3).expect("number");
    provider
        .seed_code_review(
            repository.clone(),
            CodeReview {
                number,
                title: "Review requests".to_owned(),
                state: OpenClosed::Open,
                draft: true,
                change: CommitRange {
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
                discussions: Vec::new(),
                conversation: Vec::new(),
                outstanding_requests: Vec::new(),
            },
        )
        .await;
    let targets = vec![
        ReviewRequestTarget::User("reviewer".to_owned()),
        ReviewRequestTarget::Team("faictor/maintainers".to_owned()),
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
        .code_review(&repository, number)
        .await
        .expect("read requested reviewers");
    assert_eq!(
        observed
            .outstanding_requests
            .iter()
            .filter_map(|request| request.target.request_target())
            .collect::<Vec<_>>(),
        targets
    );
}

#[tokio::test]
async fn consumer_reads_complete_review_conversations_through_the_contract() {
    let provider = FakeProvider::new();
    let repository = Repository::new("faictor", "sandbox").expect("repository");
    let number = CodeReviewNumber::new(3).expect("number");
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
    let code_review = CodeReview {
        number,
        title: "Review conversation".to_owned(),
        state: OpenClosed::Open,
        draft: false,
        change: range.clone(),
        author: author.clone(),
        updated_at: "2026-08-25T10:00:00Z".parse().expect("timestamp"),
        reviews: vec![Review {
            id: ReviewId::new("review-1").expect("review id"),
            author: reviewer.clone(),
            relationship_to_change: ReviewRelationship::Other,
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
                location: ReviewLocation::Lines {
                    path: "src/lib.rs".to_owned(),
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
            }],
        }],
        discussions: vec![ReviewThread {
            id: ReviewThreadId::new("thread-2").expect("thread id"),
            location: ReviewLocation::File {
                path: "README.lex".to_owned(),
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
        conversation: vec![ReviewComment {
            id: ReviewCommentId::new("comment-5").expect("comment id"),
            author,
            body: "Ready for review".to_owned(),
            created_at: "2026-08-25T08:50:00Z".parse().expect("timestamp"),
            updated_at: Some("2026-08-25T08:50:00Z".parse().expect("timestamp")),
        }],
        outstanding_requests: Vec::new(),
    };
    provider
        .seed_code_review(repository.clone(), code_review.clone())
        .await;

    assert_eq!(
        provider
            .code_review(&repository, number)
            .await
            .expect("review"),
        code_review
    );
    provider
        .resolve_thread(
            &repository,
            number,
            &ReviewThreadId::new("thread-1").expect("thread id"),
        )
        .await
        .expect("resolve thread");
    assert!(
        provider
            .code_review(&repository, number)
            .await
            .expect("review")
            .reviews[0]
            .findings[0]
            .status
            == ReviewThreadStatus::Resolved
    );
}

#[tokio::test]
async fn consumer_streams_release_assets_through_the_contract() {
    let provider = FakeProvider::new();
    let repository = Repository::new("faictor", "sandbox").expect("repository");
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
        .upload_asset(&repository, release_id, "postel.tar.gz", None, upload)
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
