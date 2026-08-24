Crate Layout

    How the code is physically structured: one crate per subsystem, and no binary above them — the binary is whichever tool links these crates. Subsystem isolation — what a crate may declare, and cargo enforcing it — is the implementation skill's.

    Each subsystem states its functions in its own api.rs. A crate named for a domain carries that domain's vocabulary, and vocabulary is what a repo boundary exists to keep apart.

1. Subsystems

    The platform is two crates rather than one because a contract is per domain while a provider is per system. The contracts crate holds the five domain traits in the tools' own terms and names no provider; every consumer declares it ([./interface.lex]). One crate per provider holds the client, its typed configuration and project-configuration reader, and the five domain modules implementing those traits against its system. The bucket client is its own crate.

    Beside the subsystems sit the model crate and sys, this repo's own and shared with no other.

2. The Crates

    `postel-model`:
        Provider-neutral identifiers and returned facts. A field added here is
        a promise every provider must honor or explicitly refuse.
    `postel-contracts`:
        The five object-safe asynchronous domain traits, structured provider
        errors and the five independent provider-selection variables. It
        depends on the model and no provider.
    `postel-github`:
        One authenticated Octocrab client per configured identity, the typed
        project configuration reader, and the repo, tracker, pr, jobs and
        releases adapters. Octocrab and Github response shapes cross no public
        interface.
    `postel-bucket`:
        The create-only record client. Google Cloud Storage is its default
        production adapter and an injected object-store implementation serves
        tests.
    `postel-fake`:
        A stateful in-memory implementation of all five domain traits for
        consumer tests. It records domain outcomes rather than internal call
        expectations.
    `postel-sys`:
        The small injectable set for filesystem and clock access used while
        constructing edge adapters.

    Each crate's top-level Rust documentation owns its design and trade-offs.
    This document owns only the physical dependency layout; other module docs
    point here rather than restating that layout.
