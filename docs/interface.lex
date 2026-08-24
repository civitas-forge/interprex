Interface

    What a tool links and calls. Postel is consumed at a composition root,
    never invoked: there is no command line here.

1. Linking

    A consumer declares the contracts crate and receives a provider at its
    own composition root. The rules that read a contract are tested against
    a fake one: no network, no third-party account, and nothing left behind
    in a real repo.

    Each subsystem states its functions in its own api.rs; the crate list
    is [./210-crates.lex]'s.

2. The Domain Contracts

    Five, each defined in [./100-platform.lex]:

    - repo — a repo's existence and configuration
    - tracker — issues and labels
    - pr — the pull request and its review
    - jobs — the ci runtime
    - releases — releases, assets and notes

3. The Store Clients

    The bucket client moves records at paths, and no vendor type crosses
    it; what a path is allowed to be is [./contracts/records.lex]'s.

    The secret-store client opens one credential configuration with that
    configuration's token; what a configuration holds is
    [./110-data-access.lex]'s.
