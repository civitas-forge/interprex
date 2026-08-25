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
        A proposed change: its facts, reviewed revisions, formal review
        submissions, findings and replies, review requests, check results and
        draft-ready transition.
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

2. Code Review History

    Reading a code review returns one result containing its current change
    range and its formal review submissions. A submission records one reviewer,
    an optional provider app, the exact reviewed head commit, its disposition,
    submission time, summary and zero or more findings. The current code review
    result separately carries its base and head commits. Github does not
    retain the historical base commit on a review submission, so Postel does
    not fill it with the current base and present an invented historical range.

    Submissions are never combined. Two reviewers on one revision are two
    submissions, and one reviewer submitting twice on that revision is also two
    submissions. A submission with no findings remains in the history because
    the reviewer still reviewed that revision. The reviewer set is derived from
    those submissions; a requested reviewer has not reviewed until a submission
    exists. A check result is not a reviewer.

    A finding is the initial inline comment in a review thread. It carries the
    file path, the current line when its anchor still maps, the original line,
    and an open or resolved status. Later comments are ordered replies on that
    finding. A reply by the change author does not make the author a reviewer or
    move the finding to a later submission.

    Rounds are derived rather than stored. All submissions against the same
    head commit share a revision round. A reviewer's submissions, ordered by
    submission time, form that reviewer's rounds. For every round after that
    reviewer's first, Postel derives the new-code range from the prior reviewed
    head to the new reviewed head. A pushed revision therefore starts a new
    revision round when it receives a submission, while a second submission
    against an unchanged head remains in the existing revision round. Commit
    identifiers name range endpoints; they do not assert that one endpoint
    remains an ancestor after a force push.

3. What The Contracts Do Not Cover

    Agent acts — opening issues and Github pull requests, assigning labels,
    merges and branch pushes — cross no contract ([./architecture.lex]).

    A domain holds configuration and work in flight both — a tracker's label
    taxonomy beside its issues, a repository's merge requirements beside its
    branches — and the two change on different cadences. The contracts carry
    both; which caller drives which of the two is no fact of this repository's.
