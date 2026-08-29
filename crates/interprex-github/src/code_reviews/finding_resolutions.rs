use interprex::{
    FindingResolution, FindingResolutionReason, FindingResolutionRecord, FindingSeverity,
    ProviderTextRecord, ReviewComment,
};
use serde::Deserialize;

pub(super) const RESOLVE_THREAD: &str = r#"
mutation ResolveReviewThread($threadId: ID!) {
  resolveReviewThread(input: {threadId: $threadId}) { thread { id isResolved } }
}"#;

pub(super) const ADD_THREAD_REPLY: &str = r#"
mutation AddPullRequestReviewThreadReply($threadId: ID!, $body: String!) {
  addPullRequestReviewThreadReply(
    input: {pullRequestReviewThreadId: $threadId, body: $body}
  ) { comment { id } }
}"#;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ResolveThreadData {
    pub(super) resolve_review_thread: ResolveThreadPayload,
}

#[derive(Deserialize)]
pub(super) struct ResolveThreadPayload {
    pub(super) thread: ResolvedThread,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ResolvedThread {
    pub(super) id: String,
    pub(super) is_resolved: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AddThreadReplyData {
    pub(super) add_pull_request_review_thread_reply: AddThreadReplyPayload,
}

#[derive(Deserialize)]
pub(super) struct AddThreadReplyPayload {
    pub(super) comment: AddedThreadReply,
}

#[derive(Deserialize)]
pub(super) struct AddedThreadReply {
    pub(super) id: String,
}

const FINDING_RESOLUTION_NAMESPACE: &str = "interprex";
const FINDING_RESOLUTION_NAME: &str = "finding-resolution";
const FINDING_RESOLUTION_META_START: &str = "<!-- interprex:finding-resolution\n";
const FINDING_RESOLUTION_META_END: &str = "\n-->";
const FINDING_RESOLUTION_META_VERSION: u8 = 1;

#[derive(Deserialize)]
struct GithubFindingResolution {
    resolution_reason: FindingResolutionReason,
    addressing_severity: FindingSeverity,
}

fn severity_badge(severity: FindingSeverity) -> (&'static str, &'static str, &'static str) {
    match severity {
        FindingSeverity::Critical => ("critical", "Critical", "b60205"),
        FindingSeverity::Major => ("major", "Major", "d93f0b"),
        FindingSeverity::Minor => ("minor", "Minor", "fbca04"),
        FindingSeverity::Nit => ("nit", "Nit", "c5def5"),
    }
}

fn resolution_label(reason: FindingResolutionReason) -> &'static str {
    match reason {
        FindingResolutionReason::Addressed => "Addressed",
        FindingResolutionReason::Invalid => "Invalid",
        FindingResolutionReason::WontFix => "Won't fix",
    }
}

pub(super) fn github_resolution_reply(resolution: FindingResolution, reply: &str) -> String {
    let (severity, severity_label, color) = severity_badge(resolution.addressing_severity);
    let resolution_label = resolution_label(resolution.reason);
    let visible =
        format!("**Resolution:** {resolution_label}  \n**Addressing severity:** {severity_label}");
    let badge = format!(
        "![Interprex severity: {severity}](https://img.shields.io/badge/severity-{severity}-{color})"
    );
    let visible = [visible, badge, reply.to_owned()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let metadata = ProviderTextRecord::new(
        FINDING_RESOLUTION_NAMESPACE,
        FINDING_RESOLUTION_NAME,
        serde_json::json!({
            "version": FINDING_RESOLUTION_META_VERSION,
            "resolution_reason": resolution.reason,
            "addressing_severity": resolution.addressing_severity,
        }),
    )
    .expect("the fixed finding-resolution record is valid");
    super::text_records::embed_record(&visible, &metadata)
}

#[derive(Debug, Eq, PartialEq)]
enum ParsedFindingResolution {
    Absent,
    Supported(FindingResolution),
    UnsupportedVersion(u64),
}

fn finding_resolution(body: &str) -> ParsedFindingResolution {
    let body = body.trim_end();
    let Some(marker_start) = body.rfind(FINDING_RESOLUTION_META_START) else {
        return ParsedFindingResolution::Absent;
    };
    let metadata_start = marker_start + FINDING_RESOLUTION_META_START.len();
    let Some(metadata_end) = body[metadata_start..]
        .find(FINDING_RESOLUTION_META_END)
        .map(|offset| metadata_start + offset)
    else {
        return ParsedFindingResolution::Absent;
    };
    let trailing = &body[metadata_end + FINDING_RESOLUTION_META_END.len()..];
    if !super::text_records::contains_only_records(trailing) {
        return ParsedFindingResolution::Absent;
    }
    let Some(metadata) = body.get(metadata_start..metadata_end) else {
        return ParsedFindingResolution::Absent;
    };
    let Ok(metadata) = serde_json::from_str::<serde_json::Value>(metadata) else {
        return ParsedFindingResolution::Absent;
    };
    let Some(version) = metadata.get("version").and_then(serde_json::Value::as_u64) else {
        return ParsedFindingResolution::Absent;
    };
    if version != u64::from(FINDING_RESOLUTION_META_VERSION) {
        return ParsedFindingResolution::UnsupportedVersion(version);
    }
    let Ok(metadata) = serde_json::from_value::<GithubFindingResolution>(metadata) else {
        return ParsedFindingResolution::Absent;
    };
    ParsedFindingResolution::Supported(FindingResolution {
        reason: metadata.resolution_reason,
        addressing_severity: metadata.addressing_severity,
    })
}

pub(super) fn latest_finding_resolution(
    replies: &[ReviewComment],
) -> Option<FindingResolutionRecord> {
    for comment in replies.iter().rev() {
        match finding_resolution(&comment.body) {
            ParsedFindingResolution::Absent => {}
            ParsedFindingResolution::Supported(resolution) => {
                return Some(FindingResolutionRecord::Supported {
                    resolution,
                    source_reply_id: comment.id.clone(),
                });
            }
            ParsedFindingResolution::UnsupportedVersion(metadata_version) => {
                return Some(FindingResolutionRecord::Unsupported {
                    metadata_format: format!(
                        "github:interprex-finding-resolution:v{metadata_version}"
                    ),
                    source_reply_id: comment.id.clone(),
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use interprex::{
        FindingResolution, FindingResolutionReason, FindingResolutionRecord, FindingSeverity,
        ProviderTextRecord,
    };

    use super::super::{
        change_requests::{GithubPullRequest, GithubReview, normalize_change_request},
        review_threads::ThreadsData,
    };
    use super::*;

    #[test]
    fn github_reply_keeps_visible_labels_and_hidden_canonical_metadata() {
        for (reason, label) in [
            (FindingResolutionReason::Addressed, "Addressed"),
            (FindingResolutionReason::Invalid, "Invalid"),
            (FindingResolutionReason::WontFix, "Won't fix"),
        ] {
            let expected = FindingResolution {
                reason,
                addressing_severity: FindingSeverity::Minor,
            };

            let body = github_resolution_reply(expected, "The addressing explanation.");

            assert!(body.contains(&format!("**Resolution:** {label}")));
            assert!(body.contains("**Addressing severity:** Minor"));
            assert!(body.contains("https://img.shields.io/badge/severity-minor-fbca04"));
            assert!(body.contains("<!-- interprex:finding-resolution"));
            assert_eq!(
                finding_resolution(&body),
                ParsedFindingResolution::Supported(expected)
            );
        }
    }
    #[test]
    fn parser_distinguishes_malformed_and_unsupported_resolution_metadata() {
        assert_eq!(
            finding_resolution(
                "<!-- interprex:finding-resolution\n{\"version\":2,\"resolution_reason\":\"ADDRESSED\",\"addressing_severity\":\"major\"}\n-->"
            ),
            ParsedFindingResolution::UnsupportedVersion(2)
        );
        assert_eq!(
            finding_resolution("<!-- interprex:finding-resolution\nnot json\n-->"),
            ParsedFindingResolution::Absent
        );

        let valid = github_resolution_reply(
            FindingResolution {
                reason: FindingResolutionReason::Invalid,
                addressing_severity: FindingSeverity::Nit,
            },
            "Valid record before malformed trailing metadata.",
        );
        let body = format!("{valid}\n\n<!-- interprex:finding-resolution\nnot json\n-->");
        assert_eq!(finding_resolution(&body), ParsedFindingResolution::Absent);
        assert_eq!(
            finding_resolution(&format!("{valid}\n\nordinary trailing text")),
            ParsedFindingResolution::Absent
        );

        let appended = ProviderTextRecord::new(
            "comitia",
            "loop-event",
            serde_json::json!({"version": 1, "kind": "round-finished"}),
        )
        .expect("record");
        let body = super::super::text_records::embed_record(&valid, &appended);
        assert_eq!(
            finding_resolution(&body),
            ParsedFindingResolution::Supported(FindingResolution {
                reason: FindingResolutionReason::Invalid,
                addressing_severity: FindingSeverity::Nit,
            })
        );
    }
    #[test]
    fn unsupported_newer_resolution_does_not_resurrect_an_older_record() {
        let pull_request: GithubPullRequest =
            serde_json::from_str(include_str!("../../tests/fixtures/pull_request.json"))
                .expect("pull request fixture");
        let reviews: Vec<GithubReview> = serde_json::from_str(include_str!(
            "../../tests/fixtures/code_review_reviews.json"
        ))
        .expect("review fixture");
        let mut threads: ThreadsData =
            serde_json::from_str(include_str!("../../tests/fixtures/review_threads.json"))
                .expect("thread fixture");
        let resolution = FindingResolution {
            reason: FindingResolutionReason::Addressed,
            addressing_severity: FindingSeverity::Major,
        };
        let comments = &mut threads.repository.pull_request.review_threads.nodes[0]
            .comments
            .nodes;
        comments[1].body = github_resolution_reply(resolution, "Version one resolution.");
        let mut future = comments[1].clone();
        future.id = "PRRC_future_resolution".to_owned();
        future.body = github_resolution_reply(resolution, "Future resolution.")
            .replace("\"version\":1", "\"version\":2")
            .replace(
                "\"resolution_reason\":\"ADDRESSED\"",
                "\"resolution_reason\":\"SUPERSEDED\"",
            );
        comments.push(future);

        let change_request = normalize_change_request(
            pull_request,
            reviews,
            threads.repository.pull_request.review_threads.nodes,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
        .expect("unsupported future resolution metadata remains observable as replies");

        let finding = &change_request.reviews[0].findings[0];
        assert!(matches!(
            &finding.resolution,
            Some(FindingResolutionRecord::Unsupported {
                metadata_format,
                source_reply_id,
            }) if metadata_format == "github:interprex-finding-resolution:v2"
                && source_reply_id.as_str() == "PRRC_future_resolution"
        ));
        assert_eq!(
            finding.resolution,
            latest_finding_resolution(&finding.replies)
        );
    }
}
