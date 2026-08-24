Interface

    What a tool links and calls. Postel is consumed at a composition root,
    never invoked: there is no command line here.

1. Linking

    A consumer declares the contracts crate and receives a provider at its
    own composition root. It constructs that provider from exactly one
    configuration source ([#3]). The rules that read a contract are tested
    against a fake one: no network, no third-party account, and nothing left
    behind in a real repo.

    Each subsystem states its functions in its own api.rs; the crate list
    is [./210-crates.lex]'s.

2. The Domain Contracts

    Five, each defined in [./100-platform.lex]:

    - repo — a repo's existence and configuration
    - tracker — issues and labels
    - pr — the pull request and its review
    - jobs — the ci runtime
    - releases — releases, assets and notes

    The first complete operation set:
        repo:
            Read repository facts and merge settings, apply merge settings,
            list and upsert rulesets, and encrypt then write one repository
            secret.
        tracker:
            Read one issue, list labels and upsert one label.
        pr:
            Read one pull request and every review thread with its complete,
            provider-ordered comment sequence; resolve a thread, request
            reviewers, mark a draft ready and publish an app-owned check
            outcome.
        jobs:
            Dispatch a workflow with inputs, read one run and cancel one run.
        releases:
            Read a release by tag, create a release, stream one asset with its
            exact byte length and open a download stream.

    This list owns the consumer-visible operation set. Endpoint selection,
    response normalization and authentication choices belong to the provider's
    Rust module documentation and are not repeated here.

3. Provider Configuration

    A provider accepts its configuration in either of two forms. For the file
    form, the consumer supplies the project root and the provider reads
    `.postel.toml` there. For the direct form, the consumer supplies the same
    typed configuration through the provider's public functions in `api.rs`.
    The forms do not merge, and both construct the same provider.

    Github user authentication in `.postel.toml`:
        [provider.github]
        GH_TOKEN = "token"
    :: toml ::

    `GH_TOKEN` authenticates the one configured user identity. Named app
    identities are separate:
        [provider.github.apps.automation]
        APP_ID = 123
        INSTALLATION_ID = 456
        PRIVATE_KEY = "key"
    :: toml ::

    The direct form carries `GH_TOKEN` and a map of named app credentials with
    the same three app fields. A provider may omit credentials it does not use;
    the first operation requiring an omitted user or app credential returns a
    structured error. A missing, unreadable or malformed `.postel.toml` returns
    a structured configuration error when the file form is read.

    Credential values never appear in debug output or errors and postel never
    persists them. Authentication behavior belongs to the provider
    ([./architecture.lex]).

4. The Store Client

    The bucket client moves records at paths, and no vendor type crosses
    it; what a path is allowed to be is [./contracts/records.lex]'s.
