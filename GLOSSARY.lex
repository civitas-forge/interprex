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
        request and its code-review data. Its state is open, closed without
        merging, or merged with the time the platform recorded.
    change request head:
        The branch a change request proposes for merge together with the
        repository holding that branch, which is the repository the change
        request targets or a fork of it. `ChangeRequestHead` reads the branch
        from the one ref spelling the code-review interface accepts,
        `refs/heads/<branch>`. It is both how a caller holding only its
        checkout addresses the open change requests that propose it and what
        an observed change request reports, so the two agree.
    mergeability:
        Whether the platform found a conflict between a change request's source
        and its target: mergeable, conflicted, or unknown while the platform
        has not finished computing the merge. It reports that conflict
        computation alone. A mergeable change request can still be one the
        platform refuses to merge over required checks, approvals or branch
        rules.
    code review:
        The domain that acts on change requests so merging can be approved.
        It covers mergeability, reviews, findings, standalone threads,
        unanchored comments, review requests, the checks on a commit and
        published check results.
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
        change request. The observed target, the provider address that can
        request it again and the time the platform recorded the request are
        separate facts. It describes current state, not request history: the
        time belongs to the request still standing, and it is absent when the
        provider matches the request to no retained request event, as it never
        can for a target the provider cannot identify.
    check:
        One recorded verification of a commit, with its name, the commit, its
        status, the application that published it, its published summary and
        its location. A check that has not finished is pending and has no
        conclusion; a completed check carries its conclusion and the time it
        finished. A read returns the current run of each check name, so a rerun
        replaces the run it repeated. On GitHub these are check runs; GitHub's
        separate legacy commit statuses are not read.
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
