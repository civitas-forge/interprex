//! Code-review operations implemented with GitHub pull-request APIs.
//!
//! The read combines pull-request facts, formal reviews, inline threads,
//! unanchored comments and outstanding requests into one provider-neutral
//! observation. GitHub's REST review and issue-comment records identify
//! reviews, apps and unanchored comments; GraphQL supplies thread locations,
//! resolution, complete comment sequences and outstanding requests. The
//! provider joins them here so callers never correlate GitHub entities.
//!
//! Checks are read separately, per commit, from GitHub's check-runs endpoint,
//! which reports the current run of each check within each check suite on that
//! commit. Runs sharing a name across suites all remain. GitHub's legacy commit
//! statuses are a different mechanism with its own endpoint and are not read
//! here.

use async_trait::async_trait;
use interprex::{
    ChangeRequest, ChangeRequestHead, ChangeRequestNumber, CheckOutcome, CheckRun,
    CodeReviewsProvider, FindingResolution, FindingResolutionRecord, FindingResolutionReply,
    ProviderError, Repository, Result, ReviewRequestTarget, ReviewThreadId, ReviewThreadStatus,
};
use octocrab::Page;
use serde::Deserialize;
use serde_json::json;

use crate::{GithubProvider, client::external};

mod actors;
mod change_requests;
mod checks;
mod finding_resolutions;
mod review_requests;
mod review_threads;

use change_requests::{
    GithubPullRequestNumber, head_filter, normalize_change_request, number, observed_head,
    thread_references_missing_review,
};
use checks::{conclusion, normalize_check_run};
use finding_resolutions::{
    ADD_THREAD_REPLY, AddThreadReplyData, RESOLVE_THREAD, ResolveThreadData,
    github_resolution_reply,
};
use review_requests::REQUEST_REVIEWS_BY_LOGIN;

const MARK_READY: &str = r#"
mutation MarkReady($pullRequestId: ID!) {
  markPullRequestReadyForReview(input: {pullRequestId: $pullRequestId}) {
    pullRequest { id isDraft }
  }
}"#;

#[derive(Default, Deserialize, PartialEq)]
struct PageInfo {
    #[serde(rename = "hasNextPage")]
    has_next_page: bool,
    #[serde(rename = "endCursor")]
    end_cursor: Option<String>,
}

fn continuation_cursor(
    page_info: &PageInfo,
    operation: &'static str,
    collection: &str,
) -> Result<Option<String>> {
    if !page_info.has_next_page {
        return Ok(None);
    }
    page_info
        .end_cursor
        .clone()
        .map(Some)
        .ok_or_else(|| ProviderError::External {
            provider: "github",
            operation,
            message: format!("GitHub reported another {collection} page without an end cursor"),
        })
}

#[async_trait]
impl CodeReviewsProvider for GithubProvider {
    async fn change_request(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
    ) -> Result<ChangeRequest> {
        let pull_request = self.github_pull_request(repository, number).await?;
        let mut reviews = self.github_reviews(repository, number).await?;
        let mut threads = self.github_review_threads(repository, number).await?;
        if thread_references_missing_review(&reviews, &threads) {
            reviews = self.github_reviews(repository, number).await?;
            threads = self.github_review_threads(repository, number).await?;
        }
        let requests = self.github_review_requests(repository, number).await?;
        // The timeline is a whole paginated read on its own and the request
        // times it carries describe outstanding requests only, so it is read
        // when at least one outstanding request names a reviewer to match.
        let request_events = if requests
            .iter()
            .any(|request| request.target_key().is_some())
        {
            self.github_review_request_events(repository, number)
                .await?
        } else {
            Vec::new()
        };
        let unanchored_comments = self.github_unanchored_comments(repository, number).await?;
        normalize_change_request(
            pull_request,
            reviews,
            threads,
            requests,
            request_events,
            unanchored_comments,
        )
    }

    async fn open_change_requests(
        &self,
        repository: &Repository,
        head: &ChangeRequestHead,
    ) -> Result<Vec<ChangeRequestNumber>> {
        let filter = head_filter(head);
        let page: Page<GithubPullRequestNumber> = self
            .user()?
            .get(
                format!("/repos/{repository}/pulls"),
                Some(&[
                    ("head", filter.as_str()),
                    ("state", "open"),
                    ("per_page", "100"),
                ]),
            )
            .await
            .map_err(|error| external("list open change requests", error))?;
        let listed = self
            .user()?
            .all_pages(page)
            .await
            .map_err(|error| external("list open change requests", error))?;
        let mut numbers = Vec::new();
        for pull_request in listed {
            match observed_head(&pull_request.head)? {
                Some(observed) if &observed == head => numbers.push(number(pull_request.number)?),
                // The filter addresses an owner and a branch, so another
                // repository of the same owner can answer it.
                Some(_) => {}
                None => {
                    return Err(ProviderError::Unrepresentable {
                        provider: "github",
                        fact: format!(
                            "change request {} proposes branch {} from a repository GitHub no longer identifies, so whether it proposes {} cannot be established",
                            pull_request.number,
                            pull_request.head.branch,
                            head.repository()
                        ),
                    });
                }
            }
        }
        Ok(numbers)
    }

    async fn resolve_thread(
        &self,
        _repository: &Repository,
        _number: ChangeRequestNumber,
        thread_id: &ReviewThreadId,
    ) -> Result<()> {
        let response: ResolveThreadData = self
            .user()?
            .graphql(&json!({
                "query": RESOLVE_THREAD,
                "variables": { "threadId": thread_id.as_str() }
            }))
            .await
            .map_err(|error| external("resolve review thread", error))?;
        let resolved = response.resolve_review_thread.thread;
        if resolved.id == thread_id.as_str() && resolved.is_resolved {
            Ok(())
        } else {
            Err(ProviderError::External {
                provider: "github",
                operation: "resolve review thread",
                message: format!(
                    "GitHub returned thread {} with isResolved={} for requested thread {}",
                    resolved.id,
                    resolved.is_resolved,
                    thread_id.as_str()
                ),
            })
        }
    }

    async fn resolve_finding(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        thread_id: &ReviewThreadId,
        resolution: FindingResolution,
        reply: &FindingResolutionReply,
    ) -> Result<()> {
        let change_request = self.change_request(repository, number).await?;
        let finding = change_request
            .reviews
            .iter()
            .flat_map(|review| &review.findings)
            .find(|finding| &finding.id == thread_id)
            .ok_or_else(|| ProviderError::NotFound {
                entity: format!("finding thread {}", thread_id.as_str()),
            })?;
        if let Some(FindingResolutionRecord::Unsupported {
            metadata_format, ..
        }) = &finding.resolution
        {
            return Err(ProviderError::Unrepresentable {
                provider: "github",
                fact: format!(
                    "finding thread {} contains unsupported resolution metadata format {metadata_format}",
                    thread_id.as_str()
                ),
            });
        }
        if matches!(
            &finding.resolution,
            Some(FindingResolutionRecord::Supported {
                resolution: recorded,
                ..
            }) if *recorded == resolution
        ) {
            return if finding.status == ReviewThreadStatus::Resolved {
                Ok(())
            } else {
                self.resolve_thread(repository, number, thread_id).await
            };
        }
        let already_resolved = finding.status == ReviewThreadStatus::Resolved;
        let body = github_resolution_reply(resolution, reply.as_str());
        let response: AddThreadReplyData = self
            .user()?
            .graphql(&json!({
                "query": ADD_THREAD_REPLY,
                "variables": { "threadId": thread_id.as_str(), "body": body }
            }))
            .await
            .map_err(|error| external("record finding resolution", error))?;
        if response
            .add_pull_request_review_thread_reply
            .comment
            .id
            .is_empty()
        {
            return Err(ProviderError::External {
                provider: "github",
                operation: "record finding resolution",
                message: "GitHub returned an empty reply identifier".to_owned(),
            });
        }
        if already_resolved {
            Ok(())
        } else {
            self.resolve_thread(repository, number, thread_id).await
        }
    }

    async fn request_reviewers(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        reviewers: &[ReviewRequestTarget],
    ) -> Result<()> {
        let pull_request = self.github_pull_request(repository, number).await?;
        let mut user_logins = Vec::new();
        let mut bot_logins = Vec::new();
        let mut team_slugs = Vec::new();
        for reviewer in reviewers {
            match reviewer {
                ReviewRequestTarget::User(login) => user_logins.push(login.as_str()),
                ReviewRequestTarget::Bot(login) => bot_logins.push(if login.ends_with("[bot]") {
                    login.clone()
                } else {
                    format!("{login}[bot]")
                }),
                ReviewRequestTarget::Team(identifier) => team_slugs.push(identifier.as_str()),
            }
        }
        let _: serde_json::Value = self
            .user()?
            .graphql(&json!({
                "query": REQUEST_REVIEWS_BY_LOGIN,
                "variables": {
                    "pullRequestId": pull_request.node_id,
                    "userLogins": user_logins,
                    "botLogins": bot_logins,
                    "teamSlugs": team_slugs,
                }
            }))
            .await
            .map_err(|error| external("request code reviewers", error))?;
        let outstanding = self.github_review_requests(repository, number).await?;
        let (confirmed, missing): (Vec<_>, Vec<_>) = reviewers.iter().partition(|target| {
            outstanding
                .iter()
                .any(|request| request.matches_request_target(target))
        });
        if missing.is_empty() {
            Ok(())
        } else {
            let missing = missing
                .into_iter()
                .map(review_request_target_label)
                .collect::<Vec<_>>()
                .join(", ");
            let confirmed = if confirmed.is_empty() {
                "none".to_owned()
            } else {
                confirmed
                    .into_iter()
                    .map(review_request_target_label)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            Err(ProviderError::External {
                provider: "github",
                operation: "request code reviewers",
                message: format!(
                    "GitHub did not record outstanding review requests for: {missing}; confirmed outstanding review requests for: {confirmed}"
                ),
            })
        }
    }

    async fn mark_ready(&self, repository: &Repository, number: ChangeRequestNumber) -> Result<()> {
        let pull_request = self.github_pull_request(repository, number).await?;
        let _: serde_json::Value = self
            .user()?
            .graphql(&json!({
                "query": MARK_READY,
                "variables": { "pullRequestId": pull_request.node_id }
            }))
            .await
            .map_err(|error| external("mark change request ready", error))?;
        Ok(())
    }

    async fn checks(&self, repository: &Repository, head_sha: &str) -> Result<Vec<CheckRun>> {
        self.github_check_runs(repository, head_sha)
            .await?
            .into_iter()
            .map(normalize_check_run)
            .collect()
    }

    async fn publish_check(
        &self,
        repository: &Repository,
        app_name: &str,
        outcome: &CheckOutcome,
    ) -> Result<()> {
        let _: serde_json::Value = self
            .app(app_name)?
            .post(
                format!("/repos/{repository}/check-runs"),
                Some(&json!({
                    "name": outcome.name,
                    "head_sha": outcome.head_sha,
                    "status": "completed",
                    "conclusion": conclusion(&outcome.conclusion),
                    "output": { "title": outcome.name, "summary": outcome.summary }
                })),
            )
            .await
            .map_err(|error| external("publish change request check", error))?;
        Ok(())
    }
}

fn review_request_target_label(target: &ReviewRequestTarget) -> String {
    match target {
        ReviewRequestTarget::User(login) => format!("user `{login}`"),
        ReviewRequestTarget::Bot(login) => format!("bot `{login}`"),
        ReviewRequestTarget::Team(identifier) => format!("team `{identifier}`"),
    }
}
