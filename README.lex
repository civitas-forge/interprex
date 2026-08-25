Postel

    Postel is a Rust library for working with development platforms through
    provider-neutral domain interfaces. Callers use code hosting, issue
    tracking, code review, CI jobs and releases without depending on GitHub
    response types or endpoint names.

    Postel is linked into a caller. It has no binary, server or command line.
    A caller constructs providers at its composition root and may select a
    different provider for each domain. The included GitHub provider implements
    all five domains, and the in-memory provider implements the same interfaces
    for consumer tests.

1. Domains

    code hosting:
        Read repository facts and merge settings, apply settings, read and
        update rulesets, and write encrypted repository secrets.
    tracker:
        Read issues and read or update labels.
    code review:
        Read proposed changes, reviews, findings, independent discussions,
        general conversation and outstanding review requests;
        resolve threads, request reviewers, mark a change ready and publish
        check results.
    jobs:
        Dispatch jobs, read runs and cancel runs.
    releases:
        Read and create releases, stream uploads and stream downloads.

2. Code Review Data

    A code review contains the proposed change's current base and head commits
    and every review record returned by the provider. Reviews remain distinct
    when the same actor reviews the same revision more than once.

    Each review records its author, the application that produced it when
    known, the reviewed head commit, its summary and its inline findings. Its
    state distinguishes a draft from a submitted review; a submitted review
    also carries its disposition and submission time.

    The review author is one of change author, another known actor or an actor
    whose relationship is unknown. The change-author variant refers to the
    proposed change's author instead of storing a second, independently
    writable copy. Unknown means the provider did not return enough identity
    information to compare the actors; it does not mean other. Postel returns
    this fact and leaves decisions about independent review evidence to the
    caller.

    A thread attached to a review is one of that review's findings, including a
    self-review finding from the change author. A thread with no originating
    review is an independent discussion. General conversation has no source
    location. Postel does not derive rounds, stale reviewers, severity or next
    actions from these records.

3. Crates

    `postel`:
        Provider-neutral models, errors and asynchronous domain interfaces.
    `postel-github`:
        The GitHub adapter and its typed configuration.
    `postel-test`:
        A stateful in-memory provider for consumer tests.
    `postel-bucket`:
        An independent create-only record client over `ObjectStore`.

4. Configuration

    Construct the GitHub provider directly with `from_config`, or pass a
    project root to `from_project` to read `.postel.toml`. The file form uses
    `[provider.github]` for `GH_TOKEN` and
    `[provider.github.apps.<name>]` for an app's `APP_ID`, `INSTALLATION_ID` and
    `PRIVATE_KEY`. Missing credentials are reported when an operation first
    needs them, and credential values do not appear in debug output or errors.

    `ProviderSelections::from_lookup` reads independent provider names from
    `POSTEL_CODE_HOSTING_PROVIDER`, `POSTEL_TRACKER_PROVIDER`,
    `POSTEL_CODE_REVIEWS_PROVIDER`, `POSTEL_JOBS_PROVIDER` and
    `POSTEL_RELEASES_PROVIDER`. An unset or blank value selects `github`.

5. Documentation

    [./GLOSSARY.lex]:
        The vocabulary used by the public models and documentation.
    [./docs/design.lex]:
        Domain ownership, provider construction and the complete code-review
        model.
    [./docs/contracts/records.lex]:
        The behavior guaranteed by the create-only record client.

6. Development

    Run `scripts/quality` for formatting, Lex validation, Clippy, tests and
    doctests. The pre-commit hook and GitHub Actions run the same command.
