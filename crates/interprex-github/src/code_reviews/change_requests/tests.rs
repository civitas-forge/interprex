use interprex::{
    ChangeRequestHead, ChangeRequestState, FindingResolution, FindingResolutionReason,
    FindingSeverity, Mergeability, ProviderError, Repository, ReviewActorKind, ReviewAnchor,
    ReviewAuthor, ReviewLocation, ReviewRequestTarget, ReviewTarget, ReviewTeamKind,
    ReviewThreadStatus,
};

use super::super::{
    finding_resolutions::github_resolution_reply,
    review_requests::{ReviewRequestsData, TimelineData},
    review_threads::{CommentReview, ThreadsData},
};
use super::*;

fn review_request_timeline() -> TimelineData {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/review_request_timeline.json"
    ))
    .expect("review request timeline fixture")
}

fn requested_at(time: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    Some(time.parse().expect("request timestamp"))
}

fn unanchored_comment(id: u64, node_id: &str) -> GithubUnanchoredComment {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "node_id": node_id,
        "user": {
            "node_id": "U_comment_author",
            "login": "comment-author",
            "type": "User"
        },
        "body": format!("Comment {id}"),
        "created_at": "2026-08-29T10:00:00Z",
        "updated_at": "2026-08-29T10:00:00Z"
    }))
    .expect("unanchored comment")
}

#[test]
fn equal_time_comments_follow_numeric_github_order_not_node_id_text() {
    let change_request = normalize_change_request(
        serde_json::from_str(include_str!("../../../tests/fixtures/pull_request.json"))
            .expect("pull request fixture"),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        vec![
            unanchored_comment(20, "A_lexically_first"),
            unanchored_comment(30, "M_lexically_middle"),
            unanchored_comment(10, "Z_lexically_last"),
        ],
    )
    .expect("normalizes");

    assert_eq!(
        change_request
            .unanchored_comments
            .iter()
            .map(|comment| comment.id.as_str())
            .collect::<Vec<_>>(),
        [
            "Z_lexically_last",
            "A_lexically_first",
            "M_lexically_middle"
        ]
    );
}

#[test]
fn a_head_whose_repository_github_dropped_is_absent_rather_than_guessed() {
    let mut pull_request: GithubPullRequest =
        serde_json::from_str(include_str!("../../../tests/fixtures/pull_request.json"))
            .expect("pull request fixture");
    pull_request.head.repo = None;
    let change_request = normalize_change_request(
        pull_request,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .expect("normalizes");

    assert_eq!(change_request.head, None);
    assert_eq!(
        change_request.base_branch, "main",
        "the targeted branch survives a head whose repository is gone"
    );
}

#[test]
fn head_filter_names_the_repository_holding_the_branch() {
    let upstream = Repository::new("civitas-forge", "interprex").expect("repository");
    let fork = Repository::new("contributor", "interprex").expect("repository");
    assert_eq!(
        head_filter(
            &ChangeRequestHead::new(upstream, "refs/heads/feat/open-request").expect("head")
        ),
        "civitas-forge:feat/open-request"
    );
    assert_eq!(
        head_filter(&ChangeRequestHead::new(fork, "refs/heads/feat/open-request").expect("head")),
        "contributor:feat/open-request",
        "a fork head keeps its own owner rather than the targeted repository's"
    );
}

#[test]
fn github_fixtures_preserve_reviews_findings_standalone_threads_and_unanchored_comments() {
    let pull_request: GithubPullRequest =
        serde_json::from_str(include_str!("../../../tests/fixtures/pull_request.json"))
            .expect("pull request fixture");
    let reviews: Vec<GithubReview> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/code_review_reviews.json"
    ))
    .expect("review fixture");
    let mut threads: ThreadsData =
        serde_json::from_str(include_str!("../../../tests/fixtures/review_threads.json"))
            .expect("thread fixture");
    let expected_resolution = FindingResolution {
        reason: FindingResolutionReason::Addressed,
        addressing_severity: FindingSeverity::Major,
    };
    threads.repository.pull_request.review_threads.nodes[0]
        .comments
        .nodes[1]
        .body = github_resolution_reply(expected_resolution, "Addressed in the current revision.");
    let requests: ReviewRequestsData =
        serde_json::from_str(include_str!("../../../tests/fixtures/review_requests.json"))
            .expect("review request fixture");
    let unanchored_comments: Vec<GithubUnanchoredComment> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/unanchored_comments.json"
    ))
    .expect("unanchored comments fixture");
    let timeline = review_request_timeline();
    let change_request = normalize_change_request(
        pull_request,
        reviews,
        threads.repository.pull_request.review_threads.nodes,
        requests.repository.pull_request.review_requests.nodes,
        timeline.repository.pull_request.timeline_items.nodes,
        unanchored_comments,
    )
    .expect("normalizes");

    assert_eq!(
        change_request.base_branch, "main",
        "the targeted branch is a named fact, not inferred from base_sha"
    );
    assert_eq!(
        change_request.head,
        Some(
            ChangeRequestHead::new(
                Repository::new("contributor", "interprex-sandbox").expect("repository"),
                "refs/heads/feature"
            )
            .expect("head")
        ),
        "a fork head is observed as the fork's branch, not the targeted repository's"
    );
    assert_eq!(change_request.reviews.len(), 11);
    assert_eq!(
        change_request.reviews[1].revision,
        change_request.reviews[3].revision
    );
    assert_ne!(change_request.reviews[1].id, change_request.reviews[3].id);
    assert!(change_request.reviews[0].id.as_str().starts_with("PRR_"));
    let finding = &change_request.reviews[0].findings[0];
    assert_eq!(
        finding.location,
        ReviewLocation {
            path: "docs/dev/architecture.lex".to_owned(),
            anchor: ReviewAnchor::Lines {
                side: interprex::ReviewDiffSide::Right,
                original: interprex::ReviewLineRange {
                    start: Some(interprex::ReviewLine::new(177).expect("line")),
                    end: interprex::ReviewLine::new(181).expect("line"),
                },
                current: Some(interprex::ReviewLineRange {
                    start: Some(interprex::ReviewLine::new(184).expect("line")),
                    end: interprex::ReviewLine::new(188).expect("line"),
                }),
            },
        }
    );
    assert!(finding.comment.id.as_str().starts_with("PRRC_"));
    assert_eq!(finding.replies.len(), 1);
    assert_eq!(finding.replies[0].author.login, "arthur-debert");
    assert_eq!(finding.status, ReviewThreadStatus::Resolved);
    let record = finding.resolution.as_ref().expect("resolution record");
    assert_eq!(record.supported_resolution(), Some(expected_resolution));
    assert_eq!(record.source_reply_id(), &finding.replies[0].id);
    assert_eq!(
        finding
            .resolution_reply()
            .expect("linked resolution reply")
            .author
            .login,
        "arthur-debert"
    );
    assert_eq!(
        change_request.reviews[0]
            .via_app
            .as_ref()
            .map(|app| app.slug.as_str()),
        Some("adr-review")
    );
    assert!(
        change_request
            .reviews
            .last()
            .expect("last review")
            .findings
            .is_empty()
    );
    let author_review = change_request
        .reviews
        .iter()
        .find(|item| item.author == ReviewAuthor::ChangeAuthor)
        .expect("author review");
    assert_eq!(
        author_review.author.relationship(),
        interprex::ReviewRelationship::ChangeAuthor
    );
    assert_eq!(
        author_review.author.actor(&change_request.author).login,
        "arthur-debert"
    );
    assert!(matches!(
        author_review.state,
        interprex::ReviewState::Submitted { .. }
    ));
    assert_eq!(author_review.findings.len(), 1);
    let draft_review = change_request
        .reviews
        .iter()
        .find(|item| item.author.actor(&change_request.author).login == "draft-reviewer")
        .expect("draft review");
    assert_eq!(draft_review.state, interprex::ReviewState::Draft);
    assert_eq!(
        draft_review.summary.as_deref(),
        Some("This draft was never submitted.")
    );
    let unavailable = change_request
        .reviews
        .iter()
        .filter(|item| item.author.relationship() == interprex::ReviewRelationship::Unknown)
        .collect::<Vec<_>>();
    assert_eq!(unavailable.len(), 2);
    assert_ne!(
        unavailable[0].author.actor(&change_request.author).id,
        unavailable[1].author.actor(&change_request.author).id
    );
    assert_eq!(
        change_request
            .reviews
            .iter()
            .map(|submitted| submitted.findings.len())
            .sum::<usize>()
            + change_request.standalone_threads.len(),
        4
    );
    let author_thread = author_review.findings.first().expect("author finding");
    assert_eq!(author_thread.comment.author.login, "arthur-debert");
    assert_eq!(author_thread.replies[0].author.login, "adr-agy-review");
    assert_eq!(
        author_thread.location,
        ReviewLocation {
            path: "src/lib.rs".to_owned(),
            anchor: ReviewAnchor::File,
        }
    );
    assert_eq!(change_request.outstanding_requests.len(), 6);
    assert!(matches!(
        &change_request.outstanding_requests[0].target,
        ReviewTarget::Actor(actor)
            if actor.kind == ReviewActorKind::Bot
                && actor.login == "copilot-pull-request-reviewer"
    ));
    assert!(change_request.outstanding_requests[1].as_code_owner);
    assert!(matches!(
        &change_request.outstanding_requests[2].target,
        ReviewTarget::Team(team)
            if team.slug == "maintainers"
                && team.kind == ReviewTeamKind::Organization
    ));
    assert_eq!(
        change_request.outstanding_requests[2].request_target,
        Some(ReviewRequestTarget::Team(
            "civitas-forge/maintainers".to_owned()
        ))
    );
    assert!(matches!(
        &change_request.outstanding_requests[3].target,
        ReviewTarget::Actor(actor) if actor.kind == ReviewActorKind::Placeholder
    ));
    assert!(matches!(
        &change_request.outstanding_requests[4].target,
        ReviewTarget::Team(team) if team.kind == interprex::ReviewTeamKind::Enterprise
    ));
    assert_eq!(
        change_request.outstanding_requests[5].target,
        ReviewTarget::Unavailable
    );
    assert_eq!(
        change_request
            .outstanding_requests
            .iter()
            .map(|request| request.requested_at)
            .collect::<Vec<_>>(),
        [
            requested_at("2026-06-23T09:00:00Z"),
            requested_at("2026-06-23T09:35:00Z"),
            requested_at("2026-06-23T09:15:00Z"),
            requested_at("2026-06-23T09:20:00Z"),
            requested_at("2026-06-23T09:25:00Z"),
            None,
        ]
    );
    assert_eq!(change_request.unanchored_comments.len(), 1);
    assert!(change_request.unanchored_comments[0].updated_at.is_some());
}

#[test]
fn unknown_change_request_states_are_unrepresentable() {
    let mut pull_request: GithubPullRequest =
        serde_json::from_str(include_str!("../../../tests/fixtures/pull_request.json"))
            .expect("pull request fixture");
    pull_request.state = "reopening".to_owned();

    let error = normalize_change_request(
        pull_request,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .expect_err("unknown state must be unrepresentable");
    assert!(matches!(
        error,
        ProviderError::Unrepresentable { fact, .. } if fact.contains("unknown change request state")
    ));
}

fn pull_request_with_merge_facts(
    state: &str,
    merged: bool,
    merged_at: Option<&str>,
) -> GithubPullRequest {
    let mut pull_request: GithubPullRequest =
        serde_json::from_str(include_str!("../../../tests/fixtures/pull_request.json"))
            .expect("pull request fixture");
    pull_request.state = state.to_owned();
    pull_request.merged = merged;
    pull_request.merged_at = merged_at.map(|value| value.parse().expect("merge time"));
    pull_request
}

#[test]
fn a_merged_change_request_is_distinct_from_one_closed_without_merging() {
    let closed = normalize_change_request(
        pull_request_with_merge_facts("closed", false, None),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .expect("normalizes a change request closed without merging");
    assert_eq!(closed.state, ChangeRequestState::Closed);

    let merged = normalize_change_request(
        pull_request_with_merge_facts("closed", true, Some("2026-08-24T11:00:00Z")),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .expect("normalizes a merged change request");
    assert_eq!(
        merged.state,
        ChangeRequestState::Merged {
            merged_at: "2026-08-24T11:00:00Z".parse().expect("merge time"),
        }
    );
}

#[test]
fn contradictory_merge_facts_are_unrepresentable() {
    for (state, merged, merged_at, expected) in [
        (
            "open",
            true,
            Some("2026-08-24T11:00:00Z"),
            "change request 5 is open and merged",
        ),
        ("open", true, None, "change request 5 is open and merged"),
        (
            "closed",
            true,
            None,
            "merged change request 5 has no merge time",
        ),
        (
            "closed",
            false,
            Some("2026-08-24T11:00:00Z"),
            "change request 5 has a merge time but is not merged",
        ),
        (
            "open",
            false,
            Some("2026-08-24T11:00:00Z"),
            "change request 5 has a merge time but is not merged",
        ),
    ] {
        let error = normalize_change_request(
            pull_request_with_merge_facts(state, merged, merged_at),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect_err("contradictory merge facts must be unrepresentable");
        assert!(
            matches!(&error, ProviderError::Unrepresentable { fact, .. } if fact == expected),
            "{error}"
        );
    }
}

#[test]
fn submitted_reviews_require_a_submission_time() {
    let pull_request: GithubPullRequest =
        serde_json::from_str(include_str!("../../../tests/fixtures/pull_request.json"))
            .expect("pull request fixture");
    let mut reviews: Vec<GithubReview> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/code_review_reviews.json"
    ))
    .expect("review fixture");
    reviews[0].submitted_at = None;

    let error = normalize_change_request(
        pull_request,
        reviews,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .expect_err("submitted review without time must be unrepresentable");
    assert!(matches!(
        error,
        ProviderError::Unrepresentable { fact, .. } if fact.contains("has no submission time")
    ));
}

#[test]
fn review_threads_require_an_initial_comment() {
    let pull_request: GithubPullRequest =
        serde_json::from_str(include_str!("../../../tests/fixtures/pull_request.json"))
            .expect("pull request fixture");
    let reviews: Vec<GithubReview> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/code_review_reviews.json"
    ))
    .expect("review fixture");
    let mut threads: ThreadsData =
        serde_json::from_str(include_str!("../../../tests/fixtures/review_threads.json"))
            .expect("thread fixture");
    threads.repository.pull_request.review_threads.nodes[0]
        .comments
        .nodes
        .clear();

    let error = normalize_change_request(
        pull_request,
        reviews,
        threads.repository.pull_request.review_threads.nodes,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .expect_err("thread without an initial comment must be unrepresentable");
    assert!(matches!(
        error,
        ProviderError::Unrepresentable { fact, .. } if fact.contains("has no comments")
    ));
}

#[test]
fn missing_thread_review_is_not_misclassified_as_a_standalone_thread() {
    let pull_request: GithubPullRequest =
        serde_json::from_str(include_str!("../../../tests/fixtures/pull_request.json"))
            .expect("pull request fixture");
    let reviews: Vec<GithubReview> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/code_review_reviews.json"
    ))
    .expect("review fixture");
    let mut threads: ThreadsData =
        serde_json::from_str(include_str!("../../../tests/fixtures/review_threads.json"))
            .expect("thread fixture");
    threads.repository.pull_request.review_threads.nodes[0]
        .comments
        .nodes[0]
        .pull_request_review = Some(CommentReview {
        id: "PRR_missing".to_owned(),
    });

    let error = normalize_change_request(
        pull_request,
        reviews,
        threads.repository.pull_request.review_threads.nodes,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .expect_err("missing originating submission must be unrepresentable");
    assert!(matches!(
        error,
        ProviderError::Unrepresentable { fact, .. } if fact.contains("references missing review")
    ));
}

#[test]
fn thread_without_an_originating_review_becomes_a_standalone_thread() {
    let pull_request: GithubPullRequest =
        serde_json::from_str(include_str!("../../../tests/fixtures/pull_request.json"))
            .expect("pull request fixture");
    let reviews: Vec<GithubReview> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/code_review_reviews.json"
    ))
    .expect("review fixture");
    let mut threads: ThreadsData =
        serde_json::from_str(include_str!("../../../tests/fixtures/review_threads.json"))
            .expect("thread fixture");
    let thread = threads
        .repository
        .pull_request
        .review_threads
        .nodes
        .first_mut()
        .expect("captured thread");
    let expected_id = thread.id.clone();
    thread
        .comments
        .nodes
        .first_mut()
        .expect("initial comment")
        .pull_request_review = None;
    thread.comments.nodes[1].body = github_resolution_reply(
        FindingResolution {
            reason: FindingResolutionReason::Addressed,
            addressing_severity: FindingSeverity::Minor,
        },
        "Marker text on a standalone thread.",
    );

    let change_request = normalize_change_request(
        pull_request,
        reviews,
        threads.repository.pull_request.review_threads.nodes,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .expect("normalizes standalone thread");

    assert_eq!(change_request.standalone_threads.len(), 1);
    assert_eq!(
        change_request.standalone_threads[0].id.as_str(),
        expected_id
    );
    assert_eq!(
        change_request
            .reviews
            .iter()
            .map(|item| item.findings.len())
            .sum::<usize>()
            + change_request.standalone_threads.len(),
        4
    );
}

#[test]
fn deleted_change_author_remains_an_unavailable_actor() {
    let mut pull_request: GithubPullRequest =
        serde_json::from_str(include_str!("../../../tests/fixtures/pull_request.json"))
            .expect("pull request fixture");
    pull_request.user = None;
    let reviews: Vec<GithubReview> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/code_review_reviews.json"
    ))
    .expect("review fixture");

    let change_request = normalize_change_request(
        pull_request,
        reviews,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .expect("deleted author remains readable");
    assert_eq!(change_request.author.kind, ReviewActorKind::Placeholder);
    assert_eq!(change_request.author.login, "ghost");
    assert!(
        change_request
            .reviews
            .iter()
            .all(|item| { item.author.relationship() == interprex::ReviewRelationship::Unknown })
    );
}

#[test]
fn draft_reviews_with_a_submission_time_are_unrepresentable() {
    let pull_request: GithubPullRequest =
        serde_json::from_str(include_str!("../../../tests/fixtures/pull_request.json"))
            .expect("pull request fixture");
    let mut reviews: Vec<GithubReview> = serde_json::from_str(include_str!(
        "../../../tests/fixtures/code_review_reviews.json"
    ))
    .expect("review fixture");
    reviews[10].submitted_at = Some("2026-06-23T22:10:00Z".parse().expect("submission time"));

    let error = normalize_change_request(
        pull_request,
        reviews,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    )
    .expect_err("draft review with a submission time must be unrepresentable");
    assert!(matches!(
        error,
        ProviderError::Unrepresentable { fact, .. } if fact.contains("has a submission time")
    ));
}

#[test]
fn an_uncomputed_merge_stays_distinct_from_a_conflicted_one() {
    let mergeability = |mergeable: Option<bool>| {
        let mut pull_request: GithubPullRequest =
            serde_json::from_str(include_str!("../../../tests/fixtures/pull_request.json"))
                .expect("pull request fixture");
        pull_request.mergeable = mergeable;
        normalize_change_request(
            pull_request,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("normalizes")
        .mergeability
    };

    assert_eq!(mergeability(Some(true)), Mergeability::Mergeable);
    assert_eq!(mergeability(Some(false)), Mergeability::Conflicted);
    assert_eq!(mergeability(None), Mergeability::Unknown);
}
