Interprex

    Interprex is a Rust library for working with development platforms through provider-neutral domain interfaces. It allows applications to be written both at higher level and be platform independent. It enables a stack that mixes different providers between domains to present as a unified api.

1. Domains

    Code Hosting:
        Read repository facts and merge settings, apply settings, read and update rulesets, and write encrypted repository secrets.
    Tracker:
        Read issues and read or update labels.
    Code Review:
        Read change requests, reviews, findings, stand-alone threads, unanchored comments and outstanding review requests; resolve threads, request reviewers, mark a change request ready and
        publish check results.
    Jobs:
        Dispatch jobs, read runs and cancel runs.
    Releases:
        Read and create releases, stream uploads and stream downloads.

2. Code Review Data

    A change request carries its current base and head commits and every review record returned by the provider. Reviews remain distinct when the same actor reviews the same revision more than once.

    Each review records its author, the reviewing application when known, the reviewed head commit, its summary and its inline findings. Its state distinguishes a draft from a submitted review; a submitted review also carries its disposition and submission time.

    The review author is one of change author, another known actor or an actor whose relationship is unknown. The change-author variant refers to the change request's author instead of storing a second, independently writable copy. Unknown means the provider did not return enough identity information to compare the actors; it does not mean other. Interprex returns this fact and leaves decisions about independent review evidence to the caller.

    A thread attached to a review is one of that review's findings, including a self-review finding from the change author. A thread with no originating review is a stand-alone thread. An unanchored comment has no source location. Interprex does not derive rounds, stale reviewers, severity or next actions from these records.

3. Crates

    `interprex`:
        Provider-neutral models, errors and asynchronous domain interfaces.
    `interprex-github`:
        The GitHub provider and its typed configuration.
    `interprex-test`:
        A stateful in-memory provider for consumer tests.
    `interprex-bucket`:
        An independent create-only record client over `ObjectStore`.

4. Configuration

    Construct the GitHub provider directly with `from_config`, or pass a project root to `from_project` to read `.interprex.toml`. The file form uses `[provider.github]` for `GH_TOKEN` and `[provider.github.apps.<name>]` for an app's `APP_ID`, `INSTALLATION_ID` and `PRIVATE_KEY`. Missing credentials are reported when an operation first needs them, and credential values do not appear in debug output or errors.

    `ProviderSelections::from_lookup` reads independent provider names from `INTERPREX_CODE_HOSTING_PROVIDER`, `INTERPREX_TRACKER_PROVIDER`, `INTERPREX_CODE_REVIEWS_PROVIDER`, `INTERPREX_JOBS_PROVIDER` and `INTERPREX_RELEASES_PROVIDER`. An unset or blank value selects `github`.

5. Documentation

    [./GLOSSARY.lex]:
        The vocabulary used by the public models and documentation.
    [./docs/design.lex]:
        Domain ownership, provider construction and the complete code-review
        model.
    [./docs/contracts/records.lex]:
        The behavior guaranteed by the create-only record client.

6. Development

    Run `scripts/quality` for formatting, Lex validation, Clippy, tests and doctests. The pre-commit hook and GitHub Actions run the same command.
