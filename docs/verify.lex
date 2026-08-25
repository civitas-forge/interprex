Verify

    Every line is a checkable assertion; the reasoning lives in the doc the
    line points at. An agent verifying postel walks this file top to bottom,
    checks each line against the code, and reports the lines that do not
    hold.

1. Interface

    - The repo builds as crates only; no target produces a binary
      ([./210-crates.lex]).
    - The contracts crate declares the five domain traits — repo, tracker,
      pr, jobs, releases — and depends on no provider ([./architecture.lex]).
    - Each provider crate implements all five domains against its one
      system ([./210-crates.lex]).
    - Each provider crate owns its typed configuration and reads its own table
      from `.postel.toml`.
    - The bucket client is its own crate; there is no secret-store crate.
    - Every subsystem crate states its functions in api.rs.
    - No crate shells out to gh ([./200-stack.lex]).

2. Configuration

    - A provider is constructed from one source: typed configuration supplied
      directly or `<project-root>/.postel.toml` ([./interface.lex]).
    - The consumer supplies the project root; no provider discovers it from
      the process working directory.
    - Each provider exposes file and direct construction in its `api.rs`.
    - The file and direct forms do not merge and produce the same typed
      provider configuration.
    - One selection variable per domain, declared by the contracts crate;
      unset means github.
    - Github reads user authentication from `[provider.github].GH_TOKEN`.
    - Github reads named app authentication from
      `[provider.github.apps.<identity>]`: `APP_ID`, `INSTALLATION_ID` and
      `PRIVATE_KEY`.
    - The direct github configuration carries the same user and named-app
      fields as the file form.
    - A missing credential raises a structured error naming its identity and
      kind at the first operation that needs it.
    - A missing, unreadable or malformed project configuration raises a
      structured configuration error when the file form is read.
    - Credential values appear in no debug output or error and are never
      persisted by postel.
    - Provider construction reaches no network.

3. Test Tiers

    - Rules that read a contract run against a fake provider: no network, no
      third-party account, nothing left behind in a real repo
      ([./interface.lex]).
    - The fake implements all five domain traits and records observable domain
      outcomes rather than expectations about consumer implementation.
    - Provider tests construct equivalent clients from project and direct
      configuration.
    - Github provider tests prove user operations use `GH_TOKEN`, app-only
      operations use the named app installation, and neither can substitute
      for the other.
    - Captured Github responses exercise normalization without network access;
      unknown vendor fields do not enter the model.
    - Local transport tests run the real Octocrab adapter against a loopback
      server and assert the method, path, authentication and Postel-owned body
      for every domain.
    - One ignored, read-only live test reads repository facts and labels from
      `faictor/postel-sandbox`. It assumes no issue, pull request, label or
      branch state.
    - The live workflow runs manually or for an explicitly named
      `codex/live-e2e-*` branch, shares one concurrency group across branches
      without cancellation, runs one test thread, and delays calls under a
      machine-global inter-process lock. Ordinary pushes and pull requests do
      not contact the sandbox. Octocrab's rate-limit-aware retry is enabled
      independently.
    - Above that, the verification ladder is the implementation skill's.

4. Build Outputs

    - Crates, and nothing else: no binary, no image, no docs build, no
      release artifact of its own.

5. Runtime

    - Postel runs inside whichever tool links it; nothing here owns a
      process, a server or a schedule.
    - The host store holds only what is live; anything outlasting an
      environment's container is written to a mounted volume, the bucket or
      the platform ([./110-data-access.lex]).
    - Nothing under the record prefixes expires, and no command deletes
      under them ([./contracts/records.lex]).

6. Siblings

    - kent links the contracts crate and drives the pr domain
      ([./architecture.lex]).
    - edward links the repo and jobs domains and supplies their providers.
    - minsky and sam write records through the bucket client.
    - Postel links no sibling tool.
