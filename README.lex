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
        Read change requests, reviews, findings, standalone threads, unanchored comments and outstanding review requests; record finding resolutions and addressing severity, resolve threads, request reviewers, mark a change request ready and
        publish check results.
    jobs:
        Dispatch jobs, read runs and cancel runs.
    releases:
        Read and create releases, stream uploads and stream downloads.

3. Code Review Data

    A change request carries its current base and head commits and every review record returned by the provider. Its state is open, closed without merging, or merged with the merge time the platform recorded. Reviews remain distinct when the same actor reviews the same revision more than once.

    Each review records its author, the reviewing application when known, the reviewed head commit, its summary and its inline findings. Its state distinguishes a draft from a submitted review; a submitted review also carries its disposition and submission time.

    The review author is one of change author, another known actor or an actor whose relationship is unknown. The change-author variant refers to the change request's author instead of storing a second, independently writable copy. Unknown means the provider did not return enough identity information to compare the actors; it does not mean other. Interprex returns this fact and leaves decisions about independent review evidence to the caller.

    A thread attached to a review is one of that review's findings, including a self-review finding from the change author. A thread with no originating review is a standalone thread. An unanchored comment has no source location.

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
