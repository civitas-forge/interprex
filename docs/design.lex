Design

    Interprex is a set of Rust libraries between callers and development-platform
    providers. Its public models and asynchronous traits use domain language;
    providers own authentication, endpoint selection, pagination and
    response normalization.

1. Shape

    `interprex` defines provider-neutral values and five domain interfaces. It
    depends on no provider. `interprex-github` implements those interfaces with
    GitHub REST and GraphQL. GitHub identifiers and response types remain
    private to that crate. `interprex-test` implements the same interfaces with
    state held in memory so consumer rules use the public interface without a
    network or third-party account.

    `interprex-bucket` is independent from the development-platform crates. It
    provides create-only records over an injected `ObjectStore`; its guaranteed
    behavior is [./contracts/records.lex].

    A consumer links the crates it needs and constructs providers at its
    composition root. Interprex owns no process, schedule, command line or
    orchestration policy. Git operations such as commits, refs, branches and
    pushes remain Git operations rather than Interprex domains.

2. Domains

    The five domain interfaces can use different providers in one process.
    Selection of a tracker provider does not select the code-review or jobs
    provider.

    `ProviderSelections::from_lookup` reads each selection independently from
    `INTERPREX_CODE_HOSTING_PROVIDER`, `INTERPREX_TRACKER_PROVIDER`,
    `INTERPREX_CODE_REVIEWS_PROVIDER`, `INTERPREX_JOBS_PROVIDER` and
    `INTERPREX_RELEASES_PROVIDER`. An unset or blank selection defaults to
    `github`. These selections name providers; callers still construct the
    corresponding implementations.

    code hosting:
        Repository facts, merge settings, rulesets and repository secrets.
    tracker:
        Issues and labels.
    code review:
        Change requests, reviews and their findings, standalone threads,
        unanchored comments, finding resolutions, outstanding review requests,
        check results and the draft-to-ready transition.
    jobs:
        Dispatch, run observation and cancellation.
    releases:
        Releases and streaming assets.

    A provider returns Interprex values or a structured error for
    unrepresentable data. It does not return vendor response types or fill a
    required fact with an approximation. A caller request that contradicts
    itself returns `InvalidInput` for the caller to correct; transport and
    operation failures return `External`.

3. Provider Construction

    `interprex-github::from_config` accepts typed configuration directly.
    `interprex-github::from_project` reads `<project-root>/.interprex.toml`; the
    caller supplies the project root. The forms do not merge.

    A GitHub provider may hold a user token and several named app
    installations. User operations use `GH_TOKEN`. An app-only operation names
    the configured app installation it requires. Construction performs no
    network request, and the first operation needing an absent credential
    returns a structured error. Secrets are redacted from debug output and
    errors.

4. Code Review Observation

    `CodeReviewsProvider::change_request` returns one complete observation of
    the collections declared by `ChangeRequest`. Every collection is fully
    paginated. GitHub does not provide a transaction across its pull-request,
    review, thread, unanchored-comment and request endpoints, so values may
    have changed between those reads. When a thread names a review absent from the first review
    response, the provider rereads reviews and threads once. A relationship
    that remains inconsistent is returned as unrepresentable data instead of
    being deleted or guessed.

    The change request carries its current base and head commits. A review
    carries only the reviewed head commit because GitHub does not retain the
    historical base commit for each review. Interprex does not pair a historical
    head with the current base and present it as a historical range.

    Every review record remains independent, including repeated reviews by the
    same actor against the same head and reviews without findings. Collection
    order carries no policy meaning. A draft review has `ReviewState::Draft`. A
    submitted review has
    `ReviewState::Submitted`, which contains its disposition and submission
    time. The review body becomes its optional summary in either state.

    `ReviewAuthor` stores the author and the relationship that the provider can
    establish without allowing contradictory combinations:

    change author:
        The provider returned stable actor identifiers that match. This variant
        refers to the change request's author rather than duplicating it.
    other:
        The provider returned stable actor identifiers that differ, and the
        variant contains the other actor.
    unknown:
        At least one stable actor identifier was unavailable, so Interprex cannot
        compare them. The variant contains the observed or placeholder actor.

    `ReviewAuthor::relationship` returns the category and
    `ReviewAuthor::actor` returns the actor, using the change request's author
    for the change-author variant. `via_app` separately attributes the
    reviewing application. Neither the author nor the app is the
    authentication identity.

    A caller may decide that only `other` reviews count as independent evidence.
    Interprex does not make that policy decision, and `unknown` never becomes
    `other` merely because unavailable actors receive distinct placeholder
    identifiers.

    A review thread retains its initial comment, ordered replies, open or
    resolved platform status, optional finding resolution and outdated status.
    `ReviewLocation` stores the file path once and an anchor. A line anchor
    retains its original range, diff side and current mapped range when GitHub
    supplies one. A file anchor does not invent line data.

    A thread whose initial comment names a review is nested under that review as
    a finding. This includes a change author's self-review and a draft review.
    A thread with no originating review remains a standalone thread. Replies
    do not move a thread or create another review. Unanchored comments remain
    separate because they have no source location.

    `FindingResolutionReason` has the same variants and serialized spellings as
    GitHub's `PullRequestReviewThreadResolutionReason`: `ADDRESSED` means the
    review comment was addressed, `INVALID` means the comment is invalid and
    `WONT_FIX` means it will not be addressed. `FindingResolution` records that
    reason with the addressing user's severity assessment. It does not replace
    `ReviewThreadStatus`: a manually resolved or legacy thread can have no
    finding resolution, and an interrupted provider operation can record a
    finding resolution before the platform thread becomes resolved.

    `CodeReviewsProvider::resolve_finding` takes the conclusion, addressing
    severity and visible explanatory reply. A successful operation records the
    reply and marks the platform thread resolved. Providers may need multiple
    platform requests, so an error can follow a partial write; a later
    observation preserves the recorded conclusion and platform status as
    separate facts. Before adding a reply, the GitHub adapter reads the finding.
    Repeating the same recorded conclusion does not add another reply; if the
    matching record exists while the thread remains open, the repeated call
    only resolves the thread.

    GitHub stores the canonical finding resolution in a versioned JSON envelope
    inside an HTML comment in the reply body. The same reply shows text labels
    and a severity badge for people reading the thread. The badge is redundant
    presentation: the adapter never fetches or interprets its image URL. GitHub
    currently has no generally applicable field that both accepts and returns a
    finding resolution. The adapter reads raw reply bodies, ignores malformed or
    unknown metadata versions and uses the latest valid record.

    Outstanding review requests preserve their actor or team target, the
    provider address that can request that target again when available, and
    whether GitHub requested the target as a code owner. The address is not
    inferred from actor or team category: an observed organization team may
    lack an address, while an enterprise team may have one on another provider.
    Unavailable targets remain present. A request describes current state and
    is not proof that a review exists.

    Interprex returns these observations without assigning review rounds,
    choosing a previous review, deciding that a reviewer is stale, classifying
    finding severity from prose or recommending a next action. The caller
    explicitly supplies an addressing severity when resolving a finding and
    derives other policy answers from its own configuration.

5. Provider and Caller Ownership

    Providers own transport behavior shared by every caller: authentication,
    pagination, provider retries, request encoding, response normalization,
    secret encryption and asset streaming. The GitHub provider uses Octocrab
    but exposes no Octocrab type through a domain interface.

    Callers own why an operation occurs and what follows from its result. Interprex
    can request a reviewer, record and resolve a finding, resolve a thread or
    publish a check; it does not decide when those operations should happen or
    which conclusion and severity are correct. The same distinction keeps
    review rounds and convergence rules outside the library while keeping all
    facts needed to implement them in the returned observation.
