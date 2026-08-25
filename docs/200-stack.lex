Implementation Stack

    What is bought and what is built for the clients here, and the boundary
    between them. Github network requests use Octocrab, project configuration
    reads use Tokio filesystem access, and record operations use `ObjectStore`.
    Github domain operations return Postel values and structured errors rather
    than Octocrab types. Bucket record operations use `RecordPath`, `Bytes` and
    `BucketError`; injected construction accepts an `ObjectStore`.

1. The Github Client

    Octocrab is bought; `postel-github` builds the adapter over it
    ([./architecture.lex]). Callers use the provider-neutral interfaces
    ([./100-platform.lex]) and do not call Octocrab directly. `client.rs` owns
    authenticated client construction, `config.rs` owns typed configuration and
    pure TOML parsing, and the five domain modules own Github operations. The
    complete file layout is [./210-crates.lex]'s.

    The Github client contains what every consumer needs and what must behave
    identically in every consumer. User authentication: `GH_TOKEN` is used
    directly. App authentication: installation tokens are fetched, cached and
    refreshed in-client from the named app's credentials. Repository-secret
    transport: values seal client-side (crypto_box sealed box) before the put.
    Asset transport: large release assets upload through a dedicated streaming
    call, and downloads stream through the client. Replayable requests use
    octocrab's rate-limit-aware retry policy, enabled because its default is
    not. A streamed upload is one-shot and is not retried after bytes may have
    been sent; retrying it requires a fresh caller-owned stream. And the graphql
    documents are hand-written rather than generated — the operation count is
    small and the schema is enormous.

    Octocrab supplies typed operations for checks, releases and assets,
    workflow dispatch and runs, labels, secret transport and app
    authentication. For repository settings, rulesets and branch protection, the
    Github domain modules call Octocrab's raw REST methods. They use its GraphQL
    method for review threads and resolution, marking a draft ready, and
    reviewer requests by login. `gh` is a developer convenience, never a runtime
    dependency.

    Copilot reviews are requested by bot login through the request mutation — the one path that also re-requests after a push. The copilot auto-review ruleset rule stays off in derived repository state: platform-side automation would land reviews outside the round count.

    The reviewer-request mutation is verified without a sandbox write. The live suite is read-only ([./verify.lex]), so the loopback transport test asserts the exact `requestReviewsByLogin` document and the `[bot]`-suffix partition into `botLogins`; that document is the same one GitHub's own gh CLI sends for Copilot reviewers, which stands in for a live round trip.

2. The Bucket Client

    Google Cloud Storage is the default provider. Callers hold paths and records ([./110-data-access.lex], [./contracts/records.lex]); no vendor type crosses the client.
