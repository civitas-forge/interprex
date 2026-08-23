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

    Git is not a domain either. Refs, tags, branches and pushes are git itself, identical under any backend.

    The contract speaks the model's terms — jobs, not workflows; tracker, not an issues api. A backend that cannot express a required fact refuses loudly; it never approximates.

2. Ownership

    Each row of the map below has exactly one owner, and the rows divide by when the thing changes rather than by which domain holds it. Domains do not divide cleanly: a tracker's label taxonomy is configuration and its issues are work in flight, and a pull request's facts are one caller's while its creation is the act of whoever opened it.

    Agent acts belong to no binary:
        Opening issues and prs, assigning labels, coordinator merges, branch pushes. Agents perform them with the backend's own cli, under the handbook's rules, and no tool grows to own them.

    The map:
        | What                                          | Domain   | Owner  |
        | :-------------------------------------------- | :------- | :----- |
        | settings, visibility, default branch          | repo     | edward |
        | merge requirements, required checks           | repo     | edward |
        | secrets                                       | repo     | edward |
        | label taxonomy                                | tracker  | edward |
        | issues, label assignment                      | tracker  | agents |
        | pr facts, draft-ready flip                    | pr       | kent   |
        | review requests, reviews, threads, resolution | pr       | kent   |
        | reviewer outcome check runs                   | pr       | kent   |
        | pr creation, workstream merges                | pr       | agents |
        | thin callers, dispatch, runs, runners, caches | jobs     | edward |
        | draft and live releases, assets, receipts     | releases | sam    |
        | installation, permissions                     | identity | edward |
        | tokens at review time                         | identity | kent   |
    :: table align=lll header=1 ::

3. Backend Selection

    A domain names one backend, and the domains choose independently ([#1]): moving the tracker to another system costs one new backend module — one per domain and backend ([./210-crates.lex]) — and leaves the other four where they are.

    A fact two domains both name is answered by the domain that owns it, never by whichever backend is nearer. Check results belong to the pr domain ([#1]), so the pr domain's owner reads them there ([#2]) and a jobs backend elsewhere publishes into the pr domain rather than being asked directly. The pr domain's owner reads one contract whatever runs the jobs.

    Each tool declares the backends for the domains it owns in its own config directory, since no tool reads another's.

    :: tbd ::
        Which file carries the declaration. Both homes exist — the repo-committed per-tool directories, and each tool's own operator-side configuration file — and no documented key in either names a backend.

    Github is the default. A backend that cannot express a fact its domain's contract requires refuses rather than approximating ([#1]).

    A selection reaches the backend modules and nothing else. Agent acts cross no contract — they drive the backend's own cli under the handbook's rules ([#2]) — so moving a domain never reaches them, and refs, tags, branches and pushes stay git's under any backend ([#1]).
