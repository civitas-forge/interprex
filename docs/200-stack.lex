Implementation Stack

    What is bought and what is built for the clients here, and the boundary between them. Every touch goes through sys, so a caller and its tests stay oblivious to argv, escaping and output formats.

1. The Github Client

    Octocrab is bought; the client over it is built here, one per system ([./architecture.lex]). Callers call the domain contracts ([./100-platform.lex]) and none of them calls octocrab directly. Which crate holds it is [./210-crates.lex]'s.

    The wrapper holds what every binary needs and what has to behave
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

    Octocrab types what it types: checks, releases and assets, workflow dispatch and runs, labels, secrets transport, app auth. Where it does not type, the wrapper reaches its raw REST escape hatch — repo settings, rulesets, branch protection — and its graphql method, the only route to review threads and their resolution, the draft-ready flip, and reviewer requests by login. gh is a developer convenience, never a runtime dependency.

    Copilot reviews are requested by bot login through the request mutation — the one path that also re-requests after a push. The copilot auto-review ruleset rule stays off in derived repo state: platform-side automation would land reviews outside the round count.

2. The Bucket Client

    Google Cloud Storage is the default provider. Callers hold paths and records ([./110-data-access.lex], [./contracts/records.lex]); no vendor type crosses the client.
