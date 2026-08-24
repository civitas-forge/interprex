Architecture

    The shape of postel: what the crates are, how a domain reaches its
    provider, what a deployment declares, and where postel's ownership ends.

1. The Shape

    Postel is linked crates and nothing else: no binary, no process of its
    own, no command line. It runs inside whichever tool links it.

    A tool calls a domain contract; a provider answers it. Code above
    a contract never learns which system answered.

    One client per system rather than one per caller: authentication,
    secret transport, retry policy and asset streaming have to behave
    identically everywhere, and three implementations of each would move
    independently. What the client holds is [./200-stack.lex]'s; which
    crate holds what is [./210-crates.lex]'s.

2. Domains and Refusals

    The development platform decomposes into five domains — repo, tracker,
    pr, jobs, releases — each stating its operations in the tools'
    vocabulary and naming no provider. Each is defined in
    [./100-platform.lex].

    A fact two domains both name is answered by the domain that owns it,
    never by whichever provider is nearer: check results belong to the pr
    domain, so a caller reads them there and a jobs provider elsewhere
    publishes into the pr domain rather than being asked directly. One
    contract answers whatever runs the jobs.

    A provider that cannot express a fact a contract requires refuses
    loudly; it never approximates. The refusal comes at the call that needs
    the fact, as a structured error naming the provider and the fact.
    Construction probes nothing — no capability, no variable, no network.

3. Deployment

    A consumer constructs each provider at its composition root. Provider
    credentials come from one of the two forms in [./interface.lex]: the
    project configuration or a typed value supplied directly. How the consumer
    obtains a directly supplied value is no fact of postel. Postel opens no
    credential store.

    Github is the default in source. A deployment that selects otherwise
    sets the domain's selection variable, one per domain — declared by the
    contracts crate, since a selection is read before any provider exists
    to declare it. A domain names one provider, and the domains choose
    independently: moving the tracker to another system costs one new
    provider — one per domain and system — and leaves the other four where
    they are, a module rather than a rewrite. Postel implements every
    domain in each provider it carries.

    Authentication belongs to the provider. The github provider holds the
    configured user token and named app credentials. It uses the user identity
    for user operations and the required app installation for app-only
    operations. A missing credential is a structured error naming the identity
    and credential kind at the first operation that needs it. Construction
    reaches no network.

    A selection reaches the providers and nothing else. Agent acts
    cross no contract ([#4]), so moving a domain never reaches them, and
    refs, tags, branches and pushes stay git's under any provider.

4. Boundaries

    Each entry states a contract for whoever is on the other side, and the
    contracts under [./contracts/] name no counterparty, because anything
    honoring a contract serves.

    What the domains are for:
        A contract states operations and has no opinion about who calls
        them or why. Ownership of the objects behind a domain divides among
        callers, and that division is theirs to state.
    Acts performed with the platform's own cli:
        Opening issues and pull requests, assigning labels, merging and
        pushing cross no contract here. Whoever performs them drives the
        platform's own cli, under rules that live with the performer, and no
        client here grows to own them.
    Identity:
        Not a domain. Each provider authenticates its own user and app
        identities and carries their credentials, so identity belongs to a
        provider and is never selected apart from one.
    Git:
        Not a domain. Refs, tags, branches and pushes are git itself,
        identical under any provider, and no domain models them.

5. The Tools That Link It

    kent:
        Links the contracts crate and drives the pr domain; every platform
        read and write in a review passes through it.
    edward:
        Links the repo and jobs domains for derived repo state and the ci
        runtime, and supplies their providers at its composition root.
    minsky, sam:
        Write their records — sessions, events, access — through the bucket
        client, under the write discipline of [./contracts/records.lex];
        one reader queries across all of them.

    Postel links none of them: the dependency points one way.
