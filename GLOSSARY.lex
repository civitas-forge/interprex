Glossary

    These definitions name recurring concepts in the development-platform
    interfaces. The record client's vocabulary is defined in
    [./docs/contracts/records.lex].


    provider:
        An implementation of the domain interfaces for one development
        platform, owning authentication, pagination and normalization, such
        as `GithubProvider`. The external service itself is the platform.
    change request:
        A proposed change published for merge as a platform pull or merge
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
        platform resolution status, optional finding resolution and outdated
        status remain together.
    finding resolution:
        The addressing user's recorded `ADDRESSED`, `INVALID` or `WONT_FIX`
        reason, together with that user's severity assessment and the reply
        that supplies the actor, explanation and timestamps. The three reason
        values match GitHub's `PullRequestReviewThreadResolutionReason` enum;
        severity and reply provenance are Interprex fields. This conclusion is
        separate from the platform thread's open or resolved status.
    addressing severity:
        The `critical`, `major`, `minor` or `nit` effect assigned by the user
        resolving a finding. It need not match a severity stated by the
        reviewer and is never inferred from review prose.
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
        Platform data that an Interprex model cannot faithfully represent:
        required facts are missing or inconsistent, or the entity lies
        outside the domain's model. Interprex returns
        `ProviderError::Unrepresentable` instead of approximating.
    invalid input:
        A caller request that contradicts itself, such as an upload whose
        stream does not match its declared length. Interprex returns
        `ProviderError::InvalidInput`; correcting the request, not retrying
        it, resolves the error.
