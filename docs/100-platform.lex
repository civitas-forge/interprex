The Development Platform

    A development platform is the hosted system carrying repos and their configuration, issues, pull requests, ci jobs and releases. The tools that read it run no server of their own. It holds the data, and a tool stores its own concepts as that platform's objects wherever one fits, rather than keeping a second copy under its own names — a pull request is a pull request, an epic is an issue.

1. Domains

    The platform is not one contract. It decomposes into five domains, each with its own contract and its own backend selection, so domains can live on different systems — the tracker in one, jobs in another, prs in a third.

    repo:
        A repo's existence and configuration: settings, merge requirements, required checks, secrets.
    tracker:
        Issues and the label taxonomy.
    pr:
        The pull request: its facts, reviews, threads, review requests, check results, the draft-ready flip.
    jobs:
        The ci runtime: the generated thin callers, dispatch, runs, runners, caches, and a run's own transport containers — each a named archive of any number of files, carrying its own expiry, holding whatever one job hands the next.
    releases:
        Draft and live releases, their assets and notes.

    Identity is not a domain. Each backend authenticates its own way and carries its own credentials — on github, app installation, permissions and tokens — so identity belongs to a backend and is never selected apart from one. A domain that moves takes its new backend's identity with it.

    A backend carries as many identities as a deployment configures ([#3]), and in the pr domain a write names the identity it is performed under: the caller chooses which identity each write uses, and the backend authenticates it.

    Git is not a domain either. Refs, tags, branches and pushes are git itself, identical under any backend.

    The contract speaks the model's terms — jobs, not workflows; tracker, not an issues api. A backend that cannot express a required fact refuses loudly; it never approximates. The refusal comes at the call that needs the fact, as a structured error naming the backend and the fact; construction probes no capabilities.

2. What The Contracts Do Not Cover

    Some acts cross no contract: opening issues and pull requests, assigning labels, merges, branch pushes. Whoever performs them drives the backend's own cli, and no client here grows to own them.

    A domain holds configuration and work in flight both — a tracker's label taxonomy beside its issues, a repo's merge requirements beside its branches — and the two change on different cadences. The contracts carry both; which caller drives which surface is no fact of this repo's.

3. Backend Selection

    A domain names one backend, and the domains choose independently ([#1]): moving the tracker to another system costs one new backend module — one per domain and backend ([./210-crates.lex]) — and leaves the other four where they are.

    A fact two domains both name is answered by the domain that owns it, never by whichever backend is nearer. Check results belong to the pr domain ([#1]), so a caller reads them there and a jobs backend elsewhere publishes into the pr domain rather than being asked directly. One contract answers whatever runs the jobs.

    Everything a deployment declares to these crates arrives as environment variables. A module names the variables it needs, the way the secret-store client names the one carrying its own token. The operator — whoever runs a tool on a host — sets them; in ci a caller passes them from the repo's secrets; a missing variable is a structured error naming it, at the call that first needs it — construction probes nothing, the same rule every refusal follows ([#1]). Nothing is discovered: no crate here reads a configuration file.

    Github is the default in source. A deployment that selects otherwise sets the domain's selection variable, one per domain — declared by the domain's contract crate, since a selection is read before any backend exists to declare it.

    Per-backend identity — the app, the installation, the token names — is the variables the backend module declares. They carry names and ids only, never a value; values live in the secret store ([./110-data-access.lex]).

    A selection reaches the backend modules and nothing else. Agent acts cross no contract — they drive the backend's own cli ([#2]) — so moving a domain never reaches them, and refs, tags, branches and pushes stay git's under any backend ([#1]).
