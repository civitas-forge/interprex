The Development Platform

    A development platform is the hosted system carrying repositories and their
    configuration, issues, code reviews, ci jobs and releases. The tools that
    read it run no server of their own. It holds the data, and a tool stores
    its own concepts as that platform's objects wherever one fits, rather than
    keeping a second copy under its own names — on Github, a code review is a
    pull request and an epic is an issue.

1. Domains

    The platform is not one contract. It decomposes into five domains, each
    with its own contract and its own provider selection
    ([./architecture.lex]), so domains can live on different systems — the
    tracker in one, jobs in another, code reviews in a third.

    code hosting:
        A repository's existence and configuration: settings, merge
        requirements, required checks and secrets.
    tracker:
        Issues and the label taxonomy.
    code review:
        A proposed change: its facts, reviews, threads, review requests, check
        results and draft-ready transition.
    jobs:
        The ci runtime: the generated thin callers, dispatch, runs, runners, caches, and a run's own transport containers — each a named archive of any number of files, carrying its own expiry, holding whatever one job hands the next.
    releases:
        Draft and live releases, their assets and notes.

    On github, a provider authenticates two kinds of identity
    ([./architecture.lex]): a user and an app installation. The user token
    authorizes ordinary platform operations. An app's credentials authorize
    the client to obtain an installation token for operations available only
    to that app. A provider carries one user identity and as many named app
    identities as its configuration declares. An operation that requires a
    particular app names that identity; the provider authenticates it. A
    domain that moves takes its new provider's identities with it.

    The contract speaks the model's terms — jobs, not workflows; tracker, not
    an issues api; code review, not pull request. A provider that cannot
    express a required fact refuses ([./architecture.lex]).

2. What The Contracts Do Not Cover

    Agent acts — opening issues and Github pull requests, assigning labels,
    merges and branch pushes — cross no contract ([./architecture.lex]).

    A domain holds configuration and work in flight both — a tracker's label
    taxonomy beside its issues, a repository's merge requirements beside its
    branches — and the two change on different cadences. The contracts carry
    both; which caller drives which of the two is no fact of this repository's.
