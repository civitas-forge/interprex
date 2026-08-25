Crate Layout

    The workspace contains four library crates and no binary. A consumer links
    the libraries it needs and owns its process, command line and composition
    root. Cargo dependencies keep provider-neutral types separate from vendor
    code and keep the record client independent from development-platform code.

    Source files name their responsibility. The crates do not use a generic
    `api.rs`; public items are reexported from each crate's `lib.rs`.

1. Dependency Layout

    `postel` defines the provider-neutral values and interfaces. Both
    `postel-github` and `postel-test` depend on it, while neither adapter depends
    on the other. `postel-bucket` does not depend on any of the other three
    crates. A consumer may therefore use the platform interfaces, the Github
    adapter, the in-memory test provider and the record client independently.

    The domain modules in `postel` determine which facts and operations belong
    together. Development-platform adapter modules implement those interfaces
    without exporting Octocrab or vendor response types.

2. The Crates

    `postel`:
        Provider-neutral models, structured errors, provider selection and five
        object-safe asynchronous interfaces. Its source is organized as
        `error`, `provider`, `repository`, `issues`, `pull_requests`, `jobs` and
        `releases`. The crate root reexports the public types and full interface
        names for convenient imports. Compatibility aliases retain the former
        short trait names.
    `postel-github`:
        The Github adapter. `client.rs` constructs and stores Octocrab clients,
        reports provider errors and reads `.postel.toml` with Tokio filesystem
        access. `config.rs` owns typed credentials and pure TOML parsing. The
        `repository`, `issues`, `pull_requests`, `jobs` and `releases` modules
        implement the corresponding `postel` interfaces. Octocrab and Github
        response types are private to this crate.
    `postel-test`:
        A stateful in-memory provider for consumer tests. `state.rs` stores
        observable domain state and seed data. Separate `repository`, `issues`,
        `pull_requests`, `jobs` and `releases` modules implement the same
        interfaces as the Github adapter. Tests can execute consumer rules
        without a network or third-party account.
    `postel-bucket`:
        An independent create-only record client. Its cohesive public
        implementation lives in `lib.rs`. Google Cloud Storage is the default
        production object store, and construction accepts any `ObjectStore`
        implementation. Record operations use `RecordPath`, `Bytes` and
        `BucketError`; no Google Cloud Storage type appears in them.

3. Ownership by File

    Provider-neutral data and interfaces are grouped by domain rather than by
    whether an item was formerly called a model or a contract. Each `postel`
    domain module owns both its values and its asynchronous provider trait.
    `error.rs` owns model and provider errors; `provider.rs` owns provider
    selection constants and values.

    In `postel-github`, `config.rs` parses configuration without filesystem or
    network access. `client.rs` performs project-file reads and constructs
    authenticated clients. Each domain module owns its endpoint choices,
    response normalization and implementation of the corresponding `postel`
    interface.

    In `postel-test`, `state.rs` owns shared in-memory storage and public seed or
    observation methods. Each domain module owns only that domain's interface
    implementation. In `postel-bucket`, the record path, errors, client and
    Google Cloud Storage constructor form one small interface and remain in
    `lib.rs`.
