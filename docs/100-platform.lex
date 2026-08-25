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
        A proposed change: its facts, reviewed revisions, submitted reviews,
        findings, inline discussions, general conversation, outstanding review
        requests, check results and draft-ready transition.
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

2. Code Review Model

    Reading a code review returns one complete observation of the proposed
    change, its submitted reviews and findings, independent inline discussions,
    general conversation and currently outstanding review requests. The
    provider completely paginates each declared collection and refuses a fact
    it cannot normalize rather than silently deleting it. Platforms need not
    supply a transaction across independently changing collections, so the
    result does not claim that every value was captured at one instant.

    The proposed change carries its current base and head commits. Each
    submitted review records one reviewing platform actor other than the change
    author, an optional provider app, the exact reviewed head commit, its
    disposition, submission time and optional summary. Github does not retain
    the historical base commit for a submitted review, so Postel does not pair
    its reviewed head with the current base and present an invented historical
    range.

    Submitted reviews are never combined. Two actors reviewing one revision are
    two reviews, and one actor submitting twice on that revision is also two
    reviews. A review with no findings remains present. A requested identity has
    not reviewed until a submitted review exists. Actors carry opaque provider
    identities and display logins; an optional app says how the actor submitted
    the review. When the platform no longer returns an actor, Postel preserves a
    distinct unavailable identity rather than combining unrelated deleted
    actors.

    A finding is structurally part of the submitted review that created it. It
    is an inline thread with a stable file or line-range location, an open or
    resolved status, an initial comment and ordered replies. A line location
    retains its diff side and original range, plus the current mapped range when
    Github supplies one. The thread's outdated flag is independent of whether
    that current mapping is present. Replies do not create a new submitted
    review or move the finding to another review.

    An inline thread that did not originate in a submitted review remains as an
    independent discussion. This includes a thread begun by the change author
    and a visible thread from a review that has not been submitted. A reviewer
    replying there does not turn the discussion into a finding. A provider
    response that names a thread but supplies no initial comment is incomplete,
    so the read refuses instead of silently deleting that thread. General
    conversation comments remain separate because they have no source location
    and are not submitted reviews. Non-inline text Github stores on an implicit
    review by the change author is preserved in that conversation; its update
    time is absent because Github supplies only the submission time.

    Outstanding requests name users, bots and teams and say whether Github
    requested them as code owners. A request remains visible with an unavailable
    target when the platform no longer returns that identity. Organization and
    enterprise teams remain distinct. An observed user, bot or organization
    team supplies a request target that a caller can use again; an unavailable
    identity, placeholder or enterprise team does not. Requests describe current
    state rather than request and removal history. A check result is not a
    reviewer.

    Postel does not assign rounds, select a previous review, decide that a
    reviewer is stale, classify finding severity or recommend a next action.
    Those answers depend on caller configuration and policy. A caller that
    selects a prior reviewed head may form a commit range from it to another
    head. The endpoints do not assert ancestry after a force push.

3. What The Contracts Do Not Cover

    Agent acts — opening issues and Github pull requests, assigning labels,
    merges and branch pushes — cross no contract ([./architecture.lex]).

    A domain holds configuration and work in flight both — a tracker's label
    taxonomy beside its issues, a repository's merge requirements beside its
    branches — and the two change on different cadences. The contracts carry
    both; which caller drives which of the two is no fact of this repository's.
