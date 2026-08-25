Glossary

    These definitions name recurring concepts in the development-platform
    interfaces. The record client's vocabulary is defined in
    [./docs/contracts/records.lex].


    change request:
        A proposed change published for merge as a provider pull or merge
        request. Its source is typically a git branch; the request outlives
        that branch. `ChangeRequest` is one complete observation of a change
        request, open or closed, and its code-review data.
    code review:
        The domain that acts on change requests so merging can be approved.
        It covers reviews, findings, standalone threads, unanchored
        comments, review requests and check results.
    review:
        One provider review record acting on a change request: its author,
        reviewing application when known, reviewed head commit, summary and
        findings. Its state distinguishes a draft from a submitted review.
    finding:
        An inline review thread attached to the review in which it
        originated. Its initial comment, replies, source location,
        resolution status and outdated status remain together.
    standalone thread:
        An inline review thread with no originating review. Later replies do
        not change its origin.
    unanchored comment:
        A comment on the change request with no source location.
    review request:
        One currently outstanding request for an actor or team to review a
        change request. The observed target and the provider address that
        can request it again are separate facts. It describes current state,
        not request history.
    reviewing application:
        The provider application through which an actor created or submitted
        a review (`via_app`). It is attribution, not the actor and not the
        authentication identity.
    authentication identity:
        The principal under which a provider authenticates an operation,
        such as a GitHub user or a named app installation. It is never who a
        review is attributed to.
    unrepresentable data:
        Provider data that an Interprex model cannot faithfully represent:
        required facts are missing or inconsistent, or the entity lies
        outside the domain's model. Interprex returns
        `ProviderError::Unrepresentable` instead of approximating.
