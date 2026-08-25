use bytes::Bytes;
use futures_util::{TryStreamExt, stream};
use postel::{
    AssetStreamError, AssetUpload, PullRequestNumber, PullRequestsProvider, Release, ReleaseId,
    ReleasesProvider, Repository, RepositoryFacts, RepositoryProvider, RepositorySettings,
    ReviewComment, ReviewThread, ReviewThreadId,
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

    let number = PullRequestNumber::new(3).expect("number");
    provider
        .request_reviewers(&repository, number, &["reviewer".to_owned()])
        .await
        .expect("request reviewers");
    assert_eq!(
        provider.requested_reviewers(&repository, number).await,
        ["reviewer"]
    );
}

#[tokio::test]
async fn consumer_reads_complete_review_conversations_through_the_contract() {
    let provider = FakeProvider::new();
    let repository = Repository::new("faictor", "sandbox").expect("repository");
    let number = PullRequestNumber::new(3).expect("number");
    let thread = ReviewThread {
        id: ReviewThreadId::new("thread-1").expect("thread id"),
        resolved: false,
        path: Some("src/lib.rs".to_owned()),
        line: Some(10),
        comments: vec![
            ReviewComment {
                body: "question".to_owned(),
                author: "reviewer".to_owned(),
            },
            ReviewComment {
                body: "answer".to_owned(),
                author: "author".to_owned(),
            },
        ],
    };
    provider
        .seed_review_threads(repository.clone(), number, vec![thread.clone()])
        .await;

    assert_eq!(
        provider
            .review_threads(&repository, number)
            .await
            .expect("review threads"),
        [thread]
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
            .review_threads(&repository, number)
            .await
            .expect("review threads")[0]
            .resolved
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
