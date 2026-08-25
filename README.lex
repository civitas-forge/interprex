Postel

    Postel is a Rust library that abstracts development-platform apis into
    high-level domains: code hosting, issue tracking, code review, ci jobs and
    releases.

    Each domain may be served by a different platform, behind one api
    interface — platforms coexist, and a domain can move to another platform
    without its callers changing. The api sits above raw endpoints, in
    operations a caller means: requesting a review, resolving a thread,
    publishing a release asset.

    The goal is easier integration with development platforms: portable
    calling code, platform switches with partial adoption, and callers that
    never grow specific to a single platform.

1. Domains

    code hosting:
        Configuring repositories, branches, access control, and merge
        strategy and rules.
    tracker — issue tracking:
        Tracking issues, bugs and feature requests.
    code review:
        Proposed changes, review requests, review threads and findings, and
        review state.
    jobs — ci:
        Integrating with ci pipelines: triggering jobs and managing job
        status.
    releases:
        Cutting releases and managing their assets, versions and notes.

2. Providers

    Each domain accepts providers — one per platform, all serving the same
    interface — so several platforms combine behind one api. Github is the
    first provider, and a new one slots in per domain without touching
    callers.

3. The Docs

    [./GLOSSARY.lex]:
        The words this repository defines.
    [./docs/architecture.lex]:
        The shape: crates and clients, domains and refusals, deployment,
        boundaries, and the tools that link it.
    [./docs/interface.lex]:
        What a consumer links, calls and tests against.
    [./docs/verify.lex]:
        The checkable assertions: interface, configuration, test tiers, build
        outputs, runtime, siblings.
    [./docs/100-platform.lex]:
        The development platform and its five domains.
    [./docs/110-data-access.lex]:
        The four stores, what each answers, and the method for settling a
        model and its access together.
    [./docs/200-stack.lex]:
        Bought and built: octocrab and the github client, the bucket, and
        provider authentication.
    [./docs/210-crates.lex]:
        The crate layout.
    [./docs/contracts/records.lex]:
        The write discipline every record writer meets.
