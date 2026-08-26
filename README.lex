Interprex

    Interprex is a Rust library for working with development platforms through provider-neutral domain interfaces. Applications write against the domain interfaces rather than vendor APIs, so they stay platform independent, and one process can mix different providers across domains behind the same interfaces.

1. Installation

    Add the provider-neutral models and interfaces to a Cargo project by running `cargo add interprex`.

    Interprex does not select or construct a provider. The application chooses an implementation for each domain and passes it to the code that uses that domain.

2. Domains

    code hosting:
        Read repository facts and merge settings, apply settings, read and update rulesets, and write encrypted repository secrets.
    tracker:
        Read issues and read or update labels.
    code review:
        Read change requests by number or by the head ref they propose, together with their reviews, findings, standalone threads, unanchored comments and outstanding review requests; record finding resolutions and addressing severity, resolve threads, request reviewers, mark a change request ready and
        publish check results.
    jobs:
        Dispatch jobs, read runs and cancel runs.
    releases:
        Read and create releases, stream uploads and stream downloads.

3. Code Review Data

    A change request carries its current base and head commits, the branch it targets, the head it proposes and every review record returned by the provider. Branches are named rather than left to be inferred from a commit sha, because branches share tips and advance between observations. The head is absent when the provider no longer identifies the repository holding the branch, as GitHub reports once a fork is deleted. Its state is open, closed without merging, or merged with the merge time the platform recorded. Reviews remain distinct when the same actor reviews the same revision more than once.

    A caller working from a git checkout reads the numbers of the open change requests that propose the branch it is on, then reads the observation for whichever number its own policy selects. A change request belongs to the repository it targets while its head branch can live in a fork of that repository, so a caller names both: the repository targeted and the head. A branch can be proposed by several open change requests against different bases, so every match is returned and none is picked for the caller, which tells them apart by the branch each targets.

    `ChangeRequestHead` holds that head, reading its branch from one ref spelling, `refs/heads/<branch>`. One spelling keeps every branch addressable, and a name git would refuse to create is refused here rather than sent as a query no change request could answer.

    Each review records its author, the reviewing application when known, the reviewed head commit, its summary and its inline findings. Its state distinguishes a draft from a submitted review; a submitted review also carries its disposition and submission time.

    The review author is one of change author, another known actor or an actor whose relationship is unknown. The change-author variant refers to the change request's author instead of storing a second, independently writable copy. Unknown means the provider did not return enough identity information to compare the actors; it does not mean other. Interprex returns this fact and leaves decisions about independent review evidence to the caller.

    A thread attached to a review is one of that review's findings, including a self-review finding from the change author. A thread with no originating review is a standalone thread. An unanchored comment has no source location.

    An outstanding review request records the actor or team asked to review, the address that can ask that target again, and when the platform recorded the request. A caller enforcing a review timeout reads that time from one observation instead of keeping its own record of when it asked. The time is absent when the provider cannot match the request to a request event, either because the platform no longer names the target or because the event has left the retained history, and no provider fills it with a nearby time.

    A finding resolution uses GitHub's three resolution reasons: `ADDRESSED` when the review comment was addressed, `INVALID` when the review comment is invalid and `WONT_FIX` when it will not be addressed. It also records the addressing user's severity assessment. This conclusion is separate from the platform thread's open or resolved status: manual and legacy resolutions may have no Interprex conclusion, while a partially completed write may leave a conclusion on a platform thread that is still open. Interprex does not infer rounds, stale reviewers, severity or next actions from unstructured review prose.

4. Workspace Crates

    `interprex`:
        The published crate containing provider-neutral models, errors and asynchronous domain interfaces.
    `interprex-github`:
        The repository's GitHub provider and its typed configuration.
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

7. Development

    Run `scripts/quality` for formatting, Lex validation, Clippy, tests and doctests. The pre-commit hook and GitHub Actions run the same command.

8. License

    Interprex is available under the MIT License [https://github.com/civitas-forge/interprex/blob/main/LICENSE].
