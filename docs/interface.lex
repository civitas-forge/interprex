Interface

    What a tool links and calls. Postel is consumed at a composition root,
    never invoked: there is no command line here.

1. Linking

    A consumer declares `postel` for provider-neutral values and interfaces,
    then links the adapter it selects at its own composition root. It constructs
    that provider from exactly one configuration source ([#3]). Consumer rules
    are tested against `postel-test`'s stateful in-memory provider: no network,
    no third-party account, and nothing left behind in a real repository.

    `postel` groups values and interfaces in domain modules. Adapter crates use
    matching domain modules, while named client, configuration and state files
    own shared implementation. The complete crate list is [./210-crates.lex]'s.

2. The Domain Contracts

    Five, each defined in [./100-platform.lex]:

    - code hosting — a repository's existence and configuration
    - tracker — issues and labels
    - code review — a proposed change and its review
    - jobs — the ci runtime
    - releases — releases, assets and notes

    The first complete operation set:
        code hosting:
            Read repository facts and merge settings, apply merge settings,
            list and upsert rulesets, and encrypt then write one repository
            secret.
        tracker:
            Read one issue, list labels and upsert one label.
        code review:
            Read one code review and every review thread with its complete,
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
    typed configuration to `from_config`. The Github adapter exposes
    `from_project` for the file form. The forms do not merge, and both construct
    the same provider.

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

    The bucket client creates, reads and lists records by path, and its public
    interface contains no vendor type; valid paths are defined in
    [./contracts/records.lex].
