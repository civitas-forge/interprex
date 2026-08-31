Interprex

    Interprex is a Rust library for working with development platforms through provider-neutral domain interfaces. Applications write against the domain interfaces rather than vendor APIs, so they stay platform independent, and one process can mix different providers across domains behind the same interfaces.

1. Installation

    Add the provider-neutral models and interfaces to a Cargo project by running `cargo add interprex`.

    Interprex does not select or construct a provider. The application chooses an implementation for each domain and passes it to the code that uses that domain.

2. Domains

    Code Hosting:
        Read repository facts and merge settings, apply settings, and write encrypted repository secrets.
    Source Code Configuration:
        Read and apply complete provider-specific rulesets, or read provider-neutral requirements applied to an exact repository, target branch, base commit and head commit.
    Tracker:
        Read issues and read or update labels.
    Code Review:
        Read change requests by number or by the head ref they propose, together with their mergeability, branch-update facts, reviews, findings, standalone threads, unanchored comments, outstanding review requests and checks; update an exact observed head, inspect the identity category at a review-request address, record finding resolutions and addressing severity, resolve threads, request reviewers, publish application-authored reviews, mark a change request ready and publish check results.
    Jobs:
        Dispatch jobs, read runs and cancel runs.
    Releases:
        Read and create releases, stream uploads and stream downloads.

3. Source Configuration and Code Review Data

    A change request carries its current base and head commits, the branch it targets, the head it proposes and every review record returned by the provider. Branches are named rather than left to be inferred from a commit sha, because branches share tips and advance between observations. The head is absent when the provider no longer identifies the repository holding the branch, as GitHub reports once a fork is deleted. Its state is open, closed without merging, or merged with the merge time the platform recorded. Reviews remain distinct when the same actor reviews the same revision more than once.

    A caller working from a git checkout reads the numbers of the open change requests that propose the branch it is on, then reads the observation for whichever number its own policy selects. A change request belongs to the repository it targets while its head branch can live in a fork of that repository, so a caller names both: the repository targeted and the head. A branch can be proposed by several open change requests against different bases, so every match is returned and none is picked for the caller, which tells them apart by the branch each targets.

    `ChangeRequestHead` holds that head, reading its branch from one ref spelling, `refs/heads/<branch>`. One spelling keeps every branch addressable, and a name git would refuse to create is refused here rather than sent as a query no change request could answer.

    A change request also carries its mergeability: mergeable, conflicted, or unknown while the platform has not finished computing the merge. Mergeability reports that merge computation alone. Required checks, approvals and branch rules are separate facts, so a mergeable change request can still be one the platform refuses to merge.

    `SourceCodeConfigurationProvider` reads and applies complete provider-native rulesets. The GitHub provider follows every page of repository ruleset summaries and reads each ruleset's detail before returning it. Reads preserve branch, tag, push and repository targets, inherited source identity, bypass actors, conditions, rules, parameters and unknown response fields. An omitted bypass-actor collection is an incomplete read, not an empty collection. Writes accept complete repository-owned branch, tag and push rulesets, send only GitHub's writable fields and read the accepted ruleset back before returning it. Inherited and unsupported native forms produce explicit errors rather than partial configuration.

    `AppliedSourceRequirementsProvider` reports whether the applied source configuration requires the head to contain the target-branch tip, the strongest required approval count, and one missing, pending, satisfied or failed answer for each native required check. Its observation names the exact repository, target branch, base commit and head commit it answers. Native check-run and commit-status matching belongs to the provider; the application decides what those answers mean for its policy. The GitHub provider reads rules GitHub has already selected for the target branch, combines them with classic branch protection, and answers checks from both check runs and legacy commit statuses at the stated head. An application-specific requirement accepts only a check run from that GitHub App; when a same-name commit status also exists, both native mechanisms must succeed. GitHub check contexts are matched case-insensitively when they contain only ASCII; a non-ASCII required or reported context is unrepresentable because GitHub does not define the Unicode case-folding behavior clients must reproduce.

    `BranchUpdatesProvider` reports whether the observed head contains the observed target-branch tip. Its observation retains the exact base and head revisions used for that answer. An update applies only to that observed head; if the head changes first, the provider reports a stale observation. The application combines requirement and freshness and decides whether and when to request the update.

    Each review records its author, the provider application that produced it when known, the reviewed head commit, its summary and its inline findings. Its state distinguishes a draft from a submitted review; a submitted review also carries its disposition and submission time.

    `ReviewPublishingProvider` publishes one complete review against the revision the caller supplies. A submission carries a caller-assigned publication key, summary, final disposition and inline findings in caller order. The key identifies one publication within one repository, change request and reviewer identity. That identity is the provider application ID and bot actor ID, so renamed applications and bot logins still identify the same reviewer while a different reviewer may reuse the same key.

    The GitHub provider authenticates with the named App entry whose configuration key matches the reviewer's application slug. It creates the complete pending review in one request, then submits it with the requested disposition. A retry with the same key adopts the submitted review or completes its pending review. `resume_review_publication` can do the same after the caller loses its submission, using the hidden record written with the pending review.

    `dismiss_review` withdraws the decision a published review carries, as the reviewer that published it, and records the caller's message as the visible reason for it. The review keeps its summary and its findings; only its disposition becomes dismissed, so the platform stops counting the review among the decisions on the change request. A platform withdraws a decision only from an approval or a changes-requested review, and a review already dismissed is the requested state.

    The review author is one of change author, another known actor or an actor whose relationship is unknown. The change-author variant refers to the change request's author instead of storing a second, independently writable copy. Unknown means the provider did not return enough identity information to compare the actors; it does not mean other. Interprex returns this fact and leaves decisions about independent review evidence to the caller.

    A thread attached to a review is one of that review's findings, including a self-review finding from the change author. A thread with no originating review is a standalone thread. An unanchored comment has no source location.

    Comment collections retain a stable, total order supplied by the provider. `ReviewCommentId` values support equality but no ordering relation because their representation has no provider-neutral ordering meaning. Consumers preserve collection order instead of sorting comment IDs.

    An outstanding review request records the actor or team asked to review, the address that can ask that target again, and when the platform recorded the request. A caller enforcing a review timeout reads that time from one observation instead of keeping its own record of when it asked. The time is absent when the provider cannot match the request to a request event, either because the platform no longer names the target or because the event has left the retained history, and no provider fills it with a nearby time.

    `ReviewTargetsProvider` is an optional capability separate from `CodeReviewsProvider`. Its singular inspection reports whether one configured address resolves to the requested user, bot or team category, resolves to an identity of another category, or cannot be resolved with the current credentials. An unresolved address may be absent or merely invisible to those credentials. A matching inspection does not promise that the identity can be assigned to the repository or that a later review request will be delivered.

    Checks are read by commit rather than by change request. Each check carries its name, that commit, its status, the application that published it, its published summary and where a person can read it. A check that has not finished carries the platform's own unfinished status, one of requested, queued, pending, waiting or in progress, and no conclusion; a completed check carries its conclusion and the time it finished. Publishing a check uses a narrower conclusion vocabulary than reading one, because a platform can report conclusions it refuses to accept from a client, such as GitHub's `stale`.

    A name identifies at most one run within one run of checks that the platform grouped together, which GitHub calls a check suite, and a commit carries as many of those groups as were triggered on it. Several runs on one commit can therefore share a name, and the read returns every one of them rather than choosing which answers for that name. Within one group a rerun does replace the run it repeated, so no superseded run is returned. GitHub answers from at most its 1,000 most recent check suites on a commit without signalling that it stopped there, so a commit past that limit is reported short.

    `CodeReviewsProvider::checks` reports native check runs without interpreting source policy. `AppliedSourceRequirementsProvider` separately matches the provider's required checks against both of its native mechanisms. A caller can therefore inspect check runs or consume the already-matched policy answers without reproducing provider rules.

    A finding resolution uses GitHub's three resolution reasons: `ADDRESSED` when the review comment was addressed, `INVALID` when the review comment is invalid and `WONT_FIX` when it will not be addressed. It also records the addressing user's severity assessment. This conclusion is separate from the platform thread's open or resolved status: manual and legacy resolutions may have no Interprex conclusion, while a partially completed write may leave a conclusion on a platform thread that is still open. Interprex does not infer rounds, stale reviewers, severity or next actions from unstructured review prose.

4. Workspace Crates

    `interprex`:
        The published crate containing provider-neutral models, errors and asynchronous domain interfaces.
    `interprex-github`:
        The repository's GitHub provider, its credential configuration, complete GitHub ruleset reads and writes, native ruleset values and exact-revision applied requirements.
    `interprex-test`:
        The repository's stateful in-memory provider for consumer tests.
    `interprex-bucket`:
        The repository's independent create-only record client over `ObjectStore`.

5. Configuration

    Construct the GitHub provider directly with `from_config`, or pass a project root to `from_project` to read `.interprex.toml`. The file form uses `[provider.github]` for `GH_TOKEN` and `[provider.github.apps.<name>]` for an app's `APP_ID`, `INSTALLATION_ID` and `PRIVATE_KEY`. Missing credentials are reported when an operation first needs them, and credential values do not appear in debug output or errors.

    `ProviderSelections::from_lookup` reads independent provider names from `INTERPREX_CODE_HOSTING_PROVIDER`, `INTERPREX_TRACKER_PROVIDER`, `INTERPREX_CODE_REVIEWS_PROVIDER`, `INTERPREX_JOBS_PROVIDER` and `INTERPREX_RELEASES_PROVIDER`. An unset or blank value selects `github`.

6. Documentation

    Glossary [https://github.com/civitas-forge/interprex/blob/main/GLOSSARY.lex]:
        The vocabulary used by the public models and documentation.
    Design [https://github.com/civitas-forge/interprex/blob/main/docs/design.lex]:
        Domain ownership, provider construction and the complete code-review
        model.
    Records [https://github.com/civitas-forge/interprex/blob/main/docs/contracts/records.lex]:
        The behavior guaranteed by the create-only record client.
    Changelog [https://github.com/civitas-forge/interprex/blob/main/CHANGELOG.md]:
        User-visible changes in each published version.

7. Development

    Run `scripts/quality` for formatting, Lex validation, Clippy, tests and doctests. The pre-commit hook and GitHub Actions run the same command.

8. License

    Interprex is available under the MIT License [https://github.com/civitas-forge/interprex/blob/main/LICENSE].
