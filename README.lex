Postel

    Postel holds the clients for the systems these tools do not own — the
    development platform, the object store, the secret store — and the domain
    contracts that state what is wanted of them in the tools' own terms. Code
    above a contract never learns which system answered.

    Postel is linked crates and nothing else: no binary, no process of its
    own, no command line. It runs inside whichever tool links it.

    One client per backend rather than one per caller: authentication, secret
    transport, retry policy and asset streaming have to behave identically
    everywhere, and three implementations of each would move independently.

    This file is the charter: what postel owns, what it does not, and the words
    it defines. A fact that fits none of the three does not belong in this repo.

1. What It Owns

    The domains:
        Five contracts, each stating its operations in the tools' vocabulary
        and naming no backend — repo, tracker, pr, jobs, releases. A backend
        that cannot express a fact a contract requires refuses loudly; it never
        approximates.
    The clients:
        One per backend, holding app authentication and token refresh, secret
        transport sealed before it leaves the process, streaming upload and
        download for large release assets, and a rate-limit-aware retry policy.
    Identity:
        Not a domain. Each backend authenticates its own way and carries its
        own credentials, so identity belongs to a backend and is never selected
        apart from one.
    The stores:
        What each store answers cheaply, what it answers only by scan, and what
        it does not answer at all — and the method for settling a model and its
        access together, since a model whose questions a store cannot answer is
        not implementable.
    Record layout:
        The paths that address a record in the object store, the naming that
        carries a schema version where a reader can see it without opening the
        object, and the rule that a writer only ever creates.

2. What It Does Not Own

    Each entry states a contract. None of them names who is on the other side,
    because anything honoring the contract serves.

    Which backend a domain names:
        A deployment declares that per domain, in the environment. Postel
        implements every domain against each backend it carries, and a domain
        moving to another system costs one module rather than a rewrite.
    What the domains are for:
        A contract states operations and has no opinion about who calls them or
        why. Ownership of the objects behind a domain divides among callers,
        and that division is theirs to state.
    Acts performed with a backend's own cli:
        Opening issues and pull requests, assigning labels, merging and pushing
        cross no contract here. They are performed directly, under rules that
        live with whoever performs them.
    Git:
        Refs, tags, branches and pushes are git itself, identical under any
        backend, and no domain models them.

3. The Vocabulary It Owns

    These words mean here what this repo says they mean, and a text using one
    of them in another sense belongs somewhere else.

    - domain, contract, operation, refusal
    - backend, client, installation, token
    - store, lookup, listing, scan, computed answer
    - snapshot, identifier, namespace, schema version
    - backend — one system a domain's operations are implemented against.
      Never an agent cli.
