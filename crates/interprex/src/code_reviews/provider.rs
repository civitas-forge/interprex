use async_trait::async_trait;

use super::{
    ChangeRequest, ChangeRequestHead, ChangeRequestNumber, CheckOutcome, CheckRun,
    FindingResolution, FindingResolutionReply, ProviderTextRecord, ReviewCommentId,
    ReviewRequestTarget, ReviewRequestTargetInspection, ReviewThreadId, ReviewerApplication,
};
use crate::{Repository, Result};

#[async_trait]
pub trait CodeReviewsProvider: Send + Sync {
    /// Reads one complete observation of the change request.
    async fn change_request(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
    ) -> Result<ChangeRequest>;
    /// Reads the number of every open change request in `repository` that
    /// proposes `head`.
    ///
    /// A change request belongs to the repository it targets, and its head
    /// branch can live in a fork of that repository, so `repository` and
    /// `head.repository()` are stated separately and can differ. A caller
    /// working from a git checkout names the repository the change request
    /// targets and the branch it pushed, wherever that branch lives.
    ///
    /// A branch can be proposed by more than one open change request against
    /// different bases, so every match is returned; choosing among them is the
    /// caller's policy, made from `ChangeRequest::base_branch` after reading
    /// each candidate through `change_request`. Order carries no policy
    /// meaning, and an empty result means no open change request in
    /// `repository` proposes that head.
    ///
    /// A match proposes exactly `head`: both the repository holding the branch
    /// and the branch itself, so heads differing only by repository name are
    /// different heads. `ChangeRequest::head` reports the same fact for a
    /// change request read by number.
    async fn open_change_requests(
        &self,
        repository: &Repository,
        head: &ChangeRequestHead,
    ) -> Result<Vec<ChangeRequestNumber>>;
    async fn resolve_thread(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        thread_id: &ReviewThreadId,
    ) -> Result<()>;
    /// Records why a finding is complete, records its assessed severity and
    /// marks its platform thread resolved.
    ///
    /// `reply` contains validated visible explanatory text. Providers may add
    /// their own visible or machine-readable representation around it.
    /// Providers whose platforms require more than one request can return an
    /// error after a partial write; a later observation preserves the platform
    /// thread state and any valid resolution record independently.
    ///
    /// Repeating an already recorded resolution does not add another reply. If
    /// that record exists while the platform thread is open, the repeated call
    /// only resolves the thread.
    async fn resolve_finding(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        thread_id: &ReviewThreadId,
        resolution: FindingResolution,
        reply: &FindingResolutionReply,
    ) -> Result<()>;
    /// Asks the platform to add each target to the outstanding reviewer set.
    ///
    /// A target already present remains one request, so repeating the same call
    /// reaches the same observable state.
    ///
    /// Success reports the platform's acceptance, nothing more: a platform can
    /// accept a request and record nothing, as GitHub does for a bot it cannot
    /// assign. Whether a request stands recorded is a fact of the outstanding
    /// reviewer set, read through [`Self::change_request`]; what a target
    /// names is answered before requesting by
    /// [`ReviewTargetsProvider::inspect_review_request_target`], where a
    /// provider offers it.
    async fn request_reviewers(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        reviewers: &[ReviewRequestTarget],
    ) -> Result<()>;
    async fn mark_ready(&self, repository: &Repository, number: ChangeRequestNumber) -> Result<()>;
    /// Reads the current checks on one commit, completely paginated.
    ///
    /// A check name identifies no more than one run only within a run of
    /// checks that the platform grouped together, which GitHub calls a check
    /// suite. One commit can carry several runs of the same name, published by
    /// several applications or by one application whose workflow was
    /// triggered more than once, and every one of them is returned. Deciding
    /// which of them answers for that name is the caller's, using `via_app`,
    /// the status and the completion time; Interprex discards none of them.
    /// Within one such group a rerun does replace the run it repeated, so no
    /// superseded run is reported.
    ///
    /// A check that has not concluded is returned with the platform's own
    /// status rather than omitted, so a caller can tell a missing check from a
    /// running or a stalled one. Collection order carries no meaning.
    ///
    /// A platform can report less than it holds: GitHub answers from at most
    /// the 1,000 most recent check suites on a commit and gives no signal that
    /// it stopped there, so a commit past that limit is reported short.
    ///
    /// Which of these checks a merge requires comes from the repository's
    /// rulesets, and what a failing required check means for the change
    /// request is the caller's policy. Interprex performs neither step.
    ///
    /// A platform that also keeps a separate legacy commit-status mechanism,
    /// as GitHub does, does not report those statuses here.
    async fn checks(&self, repository: &Repository, head_sha: &str) -> Result<Vec<CheckRun>>;
    async fn publish_check(
        &self,
        repository: &Repository,
        app_name: &str,
        outcome: &CheckOutcome,
    ) -> Result<()>;
}

/// Optional provider capability for inspecting review-request targets.
///
/// This trait is separate from [`CodeReviewsProvider`] so providers that
/// cannot inspect target identities do not advertise the operation and
/// existing implementations remain source compatible.
#[async_trait]
pub trait ReviewTargetsProvider: Send + Sync {
    /// Inspects one target using the credentials and provider context for
    /// `repository`.
    ///
    /// The result reports the identity and category the provider actually
    /// observed. A matching result is not an assignability check and does not
    /// promise that a later request will be recorded or delivered. Callers
    /// that need to validate several targets can inspect them individually
    /// before persisting any of them.
    async fn inspect_review_request_target(
        &self,
        repository: &Repository,
        target: &ReviewRequestTarget,
    ) -> Result<ReviewRequestTargetInspection>;
}

/// Optional provider capability for embedding structured records in provider
/// text and reading them back.
///
/// This trait owns only the text carrier. [`ProviderTextRecord::value`] has no
/// provider-defined meaning, and callers decide which namespaces, record names
/// and protocol versions they understand.
pub trait TextRecordsProvider: Send + Sync {
    /// Returns `text` with one hidden carrier containing `record`.
    ///
    /// The supplied text remains visible without alteration when the provider
    /// renders the returned value. The carrier itself may be visible through a
    /// raw-text interface. A valid [`ProviderTextRecord`] is always encodable.
    fn embed_record(&self, text: &str, record: &ProviderTextRecord) -> String;

    /// Extracts valid records from `text` in source order.
    ///
    /// Malformed carriers are ordinary text and are omitted. Records with a
    /// namespace, name or positive protocol version unknown to the caller are
    /// returned unchanged.
    fn extract_records(&self, text: &str) -> Vec<ProviderTextRecord>;
}

/// Optional provider capability for creating unanchored change-request
/// comments.
///
/// This trait is separate from [`CodeReviewsProvider`] so an implementation
/// can support code-review observation without supporting comment creation.
#[async_trait]
pub trait ChangeRequestCommentsProvider: Send + Sync {
    /// Creates exactly one unanchored comment with `body` and returns the
    /// provider identifier assigned to that comment.
    ///
    /// The provider receives the body exactly as supplied. A provider that
    /// rejects the body returns [`crate::ProviderError::InvalidInput`]; a
    /// transport or platform failure returns
    /// [`crate::ProviderError::External`].
    async fn create_unanchored_comment(
        &self,
        repository: &Repository,
        number: ChangeRequestNumber,
        body: &str,
    ) -> Result<ReviewCommentId>;
}

/// Optional provider capability for resolving reviewer applications.
///
/// This trait is separate from [`CodeReviewsProvider`] because a provider can
/// observe and request reviewers without supporting application lookup.
#[async_trait]
pub trait ReviewerApplicationsProvider: Send + Sync {
    /// Resolves `slug`, using the credentials selected for `repository`, to
    /// the application and bot actor the provider observes.
    ///
    /// The result contains the provider application's identifier, slug and
    /// name beside the bot's identifier, login and actor kind. It does not say
    /// that the application is installed for `repository`, that the bot can
    /// appear in the platform's outstanding reviewer set, that a review will
    /// arrive, or that a delivered review will attribute the application
    /// separately from its bot author.
    ///
    /// # Errors
    ///
    /// Returns [`crate::ProviderError::NotFound`] when either identity cannot
    /// be observed, [`crate::ProviderError::Unrepresentable`] when observed
    /// provider data cannot construct a [`ReviewerApplication`], and
    /// [`crate::ProviderError::External`] when the provider operation fails.
    async fn resolve_reviewer_application(
        &self,
        repository: &Repository,
        slug: &str,
    ) -> Result<ReviewerApplication>;
}
