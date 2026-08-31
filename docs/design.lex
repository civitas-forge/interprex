Design

    Interprex is a set of Rust libraries between callers and development-platform providers. Its public models and asynchronous traits use domain language;
    providers own authentication, endpoint selection, pagination and
    response normalization.

1. Shape

    `interprex` defines provider-neutral values and asynchronous provider traits. It depends on no provider. `interprex-github` implements those traits with GitHub REST and GraphQL. Provider-native configuration values belong to the provider crate; transport response types remain private. `interprex-test` implements provider traits with state held in memory so consumer rules use the public interface without a network or third-party account.

    `interprex-bucket` is independent from the development-platform crates. It provides create-only records over an injected `ObjectStore`; its guaranteed behavior is [./contracts/records.lex].

    A consumer links the crates it needs and constructs providers at its composition root. Interprex owns no process, schedule, command line or orchestration policy. Git operations such as commits, refs, branches and pushes remain Git operations rather than Interprex domains.

2. Domains

    Provider-selected domains can use different providers in one process. Selection of a tracker provider does not select the code-review or jobs provider.

    `ProviderSelections::from_lookup` reads each selection independently from `INTERPREX_CODE_HOSTING_PROVIDER`, `INTERPREX_TRACKER_PROVIDER`, `INTERPREX_CODE_REVIEWS_PROVIDER`, `INTERPREX_JOBS_PROVIDER` and `INTERPREX_RELEASES_PROVIDER`. An unset or blank selection defaults to `github`. These selections name providers; callers still construct the corresponding implementations.

    Code Hosting:
        Repository facts, merge settings and repository secrets.
    Source Code Configuration:
        Complete provider-native rulesets and provider-neutral requirements applied to exact source revisions.
    Tracker:
        Issues and labels.
    Code Review:
        Change requests, their mergeability, branch-update facts and exact-head updates, published reviews and their findings, standalone threads, unanchored comments, finding resolutions, outstanding review requests, review-request target inspection, the checks on a commit, published check results and the draft-to-ready transition.
    Jobs:
        Dispatch, run observation and cancellation.
    Releases:
        Releases and streaming assets.

    A provider returns Interprex values or a structured error for unrepresentable data. It does not return vendor response types or fill a required fact with an approximation. A caller request that contradicts itself returns `InvalidInput` for the caller to correct; transport and operation failures return `External`.

3. Provider Construction

    `interprex-github::from_config` accepts typed configuration directly. `interprex-github::from_project` reads `<project-root>/.interprex.toml`; the caller supplies the project root. The forms do not merge.

    A GitHub provider may hold a user token and several named app installations. User operations use `GH_TOKEN`. An app-only operation names the configured app installation it requires. Construction performs no network request, and the first operation needing an absent credential returns a structured error. Secrets are redacted from debug output and errors.

4. Code Review Observation

    `CodeReviewsProvider::change_request` returns one complete observation of the collections declared by `ChangeRequest`. Every collection is fully paginated. GitHub does not provide a transaction across its pull-request, review, thread, unanchored-comment and request endpoints, so values may have changed between those reads. When a thread names a review absent from the first review response, the provider rereads reviews and threads once. A relationship that remains inconsistent is returned as unrepresentable data instead of being deleted or guessed.

    `CodeReviewsProvider::open_change_requests` answers the reverse question: given the repository a change request targets and the head it proposes, which open change requests are those. It returns their numbers, so the caller reads the observation it wants through `change_request`. A branch can be proposed by more than one open change request against different bases; choosing among them is caller policy, made from the branch each targets, so every match is returned, order carries no policy meaning and an empty result means no open change request proposes that head.

    A change request belongs to the repository it targets, while its head branch can live in a fork of that repository. `ChangeRequestHead` keeps those two repositories separate: the caller states the repository targeted and the head, and a provider infers neither from the other. GitHub filters pull requests by a head written `owner:branch`, which names where the branch lives, so the provider reads the targeted repository's pull requests and writes the head's own repository into the filter. That filter addresses an owner and a branch, not a repository, so another repository of the same owner answers it; the provider compares each result's observed head against
    the one asked for rather than trusting the filter to have been exact.

    `ChangeRequestHead::new` reads the branch from one ref spelling, `refs/heads/<branch>`. Accepting a bare branch name as well would leave a branch named `refs/heads/main` unaddressable, because the same string also qualifies branch `main`. Construction applies the rules `git check-ref-format` applies to a branch: the forbidden characters, `HEAD`, a leading `-`, a trailing `.`, `..`, `@{`, the name `@`, and empty, dot-prefixed or `.lock` path components. A name git would refuse to create is `InvalidHeadRef` where the caller writes it, and every provider receives a head it can address.

    `ChangeRequestState` reports whether the change request is open, closed without merging or merged, and the merged variant carries the merge time the platform recorded. Whether a change landed is an observed fact, so no caller reads the vendor API to recover it. GitHub reports a merge as a closed pull request with `merged` set and a merge time; any other combination of those three fields, and any state Interprex does not model, is returned as unrepresentable data.

    The change request carries its current base and head commits, names the branch it targets and carries the head it proposes. Branches are named because a sha cannot identify one: branches share tips and advance between observations. Two open change requests proposing the same head differ by the branch each targets, so the choice `open_change_requests` leaves to the caller is one the observation can answer, and a change request read by number reports the same head that would have found it. A head is absent when the provider no longer identifies the repository holding the branch, as GitHub reports once a fork is deleted; a branch name without its repository is not a head, so it is not paired with the targeted repository instead.

    A review carries only the reviewed head commit because GitHub does not retain the historical base commit for each review. Interprex does not pair a historical head with the current base and present it as a historical range.

    `ChangeRequest::mergeability` carries the platform's answer for the current source and target: mergeable, conflicted, or unknown. GitHub starts computing the merge when a read arrives and reports `null` until that computation finishes, so unknown is an observed platform state rather than a read that failed or a value Interprex chose in place of one.

    GitHub's `mergeable_state` string is absent from the model because it combines mergeability, checks, approvals, branch freshness and the draft flag into one provider verdict. Interprex returns those facts separately instead of claiming they reconstruct that verdict.

    `AppliedSourceRequirementsProvider` observes the strongest required approval count, the branch-update requirement and one answer for every native required check at an exact repository, target branch, base commit and head commit. The provider matches native check runs and commit statuses to native requirements. `BranchUpdatesProvider` separately observes whether the head contains the target-branch tip and retains the base and head revisions used for the comparison. Requirement, freshness and mergeability remain separate: a mergeable head can be behind without an applicable rule requiring an update. The caller decides whether and when to update.

    A branch update names the exact observed head. The provider refuses a stale observation instead of applying the request to a newer head. GitHub compares the observed base and head commits, then uses its native branch-update operation with the expected head.

    `CodeReviewsProvider::checks` reads the current checks on one commit, completely paginated. A caller reads them for whichever commit it cares about, usually the change request's current head. The request sends GitHub's `filter=latest` explicitly, which scopes the answer to the current run of each check within each check suite. A rerun inside a suite replaces the run it repeated, so no superseded run is reported and a caller that wants the earlier runs cannot get them here.

    A name identifies at most one run inside a suite, and a commit carries as many suites as were triggered on it. Several runs on one commit can therefore share a name: two applications publishing the same check, or one application whose workflow ran twice. Interprex returns every one of them. Collapsing them would mean choosing which run answers for that name, and that choice belongs to the caller, which has `via_app`, the status and the completion time to make it. GitHub's own required-status-check rules face the same ambiguity and resolve it by context and `integration_id`.

    GitHub also answers from at most the 1,000 most recent check suites on a commit and gives no signal that it stopped there, so a commit past that limit is reported short; the trait documents that limit alongside the read.

    `CheckStatus` holds the conclusion inside its completed variant, following `ReviewState`: a check that has not finished has no conclusion to report, so no combination of the returned facts can contradict itself. `CheckStatus::conclusion` returns that conclusion, or nothing while the check is unfinished, for a caller that needs only this much.

    The variants before `Completed` are GitHub's own unfinished statuses, one each for `requested`, `queued`, `pending`, `waiting` and `in_progress`. They stay distinct because they do not mean the same thing to a person waiting on the check: queued is progress, while a run held back by a concurrency limit or an unsatisfied deployment protection rule is not, and Interprex does not decide which of those distinctions a caller reports on. `Pending` therefore names the one state GitHub calls `pending`, not every unfinished one.

    `CheckConclusion` covers the conclusions a check run reports, so a read never discards one. It holds one value GitHub's documented response enum omits, `stale`, which GitHub sets on a run itself. It omits `startup_failure`: GitHub reports that for a check suite that failed before its runs began and states that it does not apply to check runs, so a run reporting it is unrepresentable data rather than a modelled state, and the jobs domain keeps `RunConclusion::StartupFailure`, where it is observable.

    The observed `CheckRun` stays separate from the written `CheckOutcome`, which always carries a conclusion because Interprex publishes only finished results. Their conclusion vocabularies are separate for the same reason `ReviewRequestTarget` is narrower than `ReviewTarget`: GitHub sets `stale` on a check run itself and refuses that conclusion from a client, so `CheckOutcome` uses `PublishedCheckConclusion`, which has no such variant. A request GitHub would reject is therefore not constructible rather than checked at the boundary.

    A check run also carries the application that published it, as `via_app`, and where a person can read it, as `html_url`. An applied required-check answer names the provider application required by the native rule as an opaque `ProviderAppId`. The provider compares that identity with check-run publishers and commit-status creators; consumers do not parse it as a GitHub integer or repeat the native matching rules.

    A status GitHub adds later, a conclusion Interprex does not model, a completed check missing its conclusion or completion time, and a running check that reports either, are all returned as unrepresentable data.

    GitHub returns check runs in an envelope keyed `check_runs` beside a `total_count` rather than as a bare array. Octocrab's `Page` recognizes a fixed set of envelope keys that excludes this one, so this read pages by number instead of following `Link` headers, and stops at a short page or once it holds the reported total.

    GitHub's legacy commit statuses are a separate mechanism from check runs, with their own endpoint. This domain reads check runs only, so a caller that needs commit statuses cannot obtain them here.

    Interprex reports the answer to each applied native requirement but does not decide that a change request is ready or answer whether the platform would accept the merge. Those are caller decisions over the returned facts.

    Every review record remains independent, including repeated reviews by the same actor against the same head and reviews without findings. Collection order carries no policy meaning. A draft review has `ReviewState::Draft`. A submitted review has `ReviewState::Submitted`, which contains its disposition and submission time. The review body becomes its optional summary in either state.

    `ReviewPublishingProvider` publishes one complete review against the revision the caller supplies. The GitHub provider selects the named app whose configuration key matches the reviewer's app slug and refuses to write when its configured App ID differs from the resolved reviewer. It creates the summary, hidden publication record and inline findings together as a pending review, submits that review, then reads it back through the same app installation. A repeated publication key adopts the matching review or submits its pending review. A failed create response causes a read. When that read finds the publication, the provider finishes it. When it finds instead a pending review the same reviewer app left under another publication key — GitHub accepts one pending review per author per change request, and refuses every create made while such a review stands — the provider deletes that review and creates once more. It deletes no review that is submitted or authored by anyone else, and no other condition produces a second create request.

    The same interface withdraws the decision one of those reviews carries, recording the caller's message as its visible reason. GitHub numbers a review for the dismissal request while callers hold its node ID, so the provider reads the review before it writes. It refuses a review another reviewer identity published and one that carries no decision, and it answers a review already dismissed with success. After the write it reads the review again: a dismissal GitHub applied before its response was lost reconciles to the same success as an acknowledged one, and every other result keeps the write failure.

    `ReviewAuthor` stores the author and the relationship that the provider can establish without allowing contradictory combinations:

    change author:
        The provider returned stable actor identifiers that match. This variant refers to the change request's author rather than duplicating it.
    other:
        The provider returned stable actor identifiers that differ, and the variant contains the other actor.
    unknown:
        At least one stable actor identifier was unavailable, so Interprex cannot compare them. The variant contains the observed or placeholder actor.
    `ReviewAuthor::relationship` returns the category and `ReviewAuthor::actor` returns the actor, using the change request's author for the change-author variant. `via_app` separately attributes the provider application that produced the review. Neither the author nor the app is the authentication identity.

    A caller may decide that only `other` reviews count as independent evidence. Interprex does not make that policy decision, and `unknown` never becomes `other` merely because unavailable actors receive distinct placeholder identifiers.

    `ReviewThread` retains the facts shared by inline threads: its initial comment, ordered replies, open or resolved platform status and outdated status. `ReviewFinding` combines those facts with an optional finding resolution. `ReviewLocation` stores the file path once and an anchor. A line anchor retains its original range, diff side and current mapped range when GitHub supplies one. A file anchor does not invent line data.

    A thread whose initial comment names a review is nested under that review as a finding. This includes a change author's self-review and a draft review. A thread with no originating review remains a standalone thread. Replies do not move a thread or create another review. Unanchored comments remain separate because they have no source location.

    `FindingResolutionReason` has the same variants and serialized spellings as GitHub's `PullRequestReviewThreadResolutionReason`: `ADDRESSED` means the review comment was addressed, `INVALID` means the comment is invalid and `WONT_FIX` means it will not be addressed. `FindingResolution` contains that reason and the addressing user's severity assessment. `FindingResolutionRecord` links to the reply that identifies the actor, explanation and timestamps. Its supported variant contains the understood conclusion; its unsupported variant preserves a provider-defined opaque metadata-format identifier without treating an older record as current. It does not replace `ReviewThreadStatus`: a manually resolved or legacy thread can have no finding resolution, and an interrupted provider operation can record a finding resolution before the platform thread becomes resolved. Standalone `ReviewThread` values have no finding-resolution field.

    `CodeReviewsProvider::resolve_finding` takes the conclusion, addressing severity and a `FindingResolutionReply`, whose constructor rejects blank explanations. A successful operation records the reply and marks the platform thread resolved. Providers may need multiple platform requests, so an error can follow a partial write; a later observation preserves the recorded conclusion and platform status as separate facts. Before adding a reply, the GitHub adapter reads the finding. Repeating the same recorded conclusion does not add another reply; if the matching record exists while the thread remains open, the repeated call only resolves the thread.

    GitHub stores the canonical finding resolution in a versioned JSON envelope inside an HTML comment in the reply body. The same reply shows text labels and a severity badge for people reading the thread. The badge is redundant presentation: the adapter never fetches or interprets its image URL. GitHub currently has no generally applicable field that both accepts and returns a finding resolution. The adapter reads a terminal metadata envelope from raw reply bodies. Malformed envelopes are ordinary text. An unsupported newer version stops fallback to an older record, and the current adapter refuses to write over that newer format.

    Outstanding review requests preserve their actor or team target, the provider address that can request that target again when available, and whether GitHub requested the target as a code owner. The address is not inferred from actor or team category: an observed organization team may lack an address, while an enterprise team may have one on another provider. Unavailable targets remain present. A request describes current state and is not proof that a review exists.

    Providers may implement `ReviewTargetsProvider` to inspect what one `ReviewRequestTarget` names before a caller requests a review. `CodeReviewsProvider::request_reviewers` writes the request and reports the platform's acceptance, nothing more: a platform can accept a request and record nothing, as GitHub does for a bot it cannot assign. Whether a request stands recorded is a fact of the outstanding reviewer set, read through `change_request`, and a caller that needs it reads it there.

    A request also carries the time the platform recorded it, when the provider can observe one. GitHub's outstanding-request records carry no time, so the GitHub adapter reads the change request's review-requested and review-request-removed timeline events, completely paginated, and matches each outstanding request to the latest request event for the same reviewer identifier that no later removal discarded. A reviewer requested, removed and requested again therefore reports the latest request. User, bot and mannequin identifiers are matched separately from team identifiers, so a team and a user never take each other's time however their slug and login are spelled. A request whose target GitHub no longer names has no identifier to match and reports no time, as does a request whose event has left the retained timeline. The adapter never substitutes a nearby timestamp, so a caller measuring how long a request has stood reads an absent time as no measurement rather than an approximate one.

    The outstanding-request list and the timeline are separate reads with no transaction across them, so the time reported for a reviewer is the one on that reviewer's latest surviving request event at the moment the timeline was read. A reviewer re-requested between the two reads reports the newer request's time beside the older request record, and a reviewer whose request was withdrawn between them reports no time at all. Interprex does not read either collection twice to close that window: a later observation reports the settled state, and the absence of a cross-collection snapshot already governs every other collection in this observation.

    The timeline is a paginated read of its own, and the times it carries
    describe outstanding requests only. The adapter therefore reads it when at
    least one outstanding request names a reviewer to match and skips it
    entirely otherwise, so an observation with no outstanding reviewer costs
    no additional round trip.

    Interprex returns these observations without assigning review rounds, choosing a previous review, deciding that a reviewer is stale, classifying finding severity from prose or recommending a next action. The caller explicitly supplies an addressing severity when resolving a finding and derives other policy answers from its own configuration.

5. Provider and Caller Ownership

    Providers own transport behavior shared by every caller: authentication,
    pagination, provider retries, request encoding, response normalization,
    secret encryption and asset streaming. The GitHub provider uses Octocrab
    but exposes no Octocrab type through a domain interface.

    Callers own why an operation occurs and what follows from its result. Interprex can request a reviewer, record and resolve a finding, resolve a thread or publish a check; it does not decide when those operations should happen or which conclusion and severity are correct. The same distinction keeps review rounds and convergence rules outside the library while keeping all facts needed to implement them in the returned observation.
