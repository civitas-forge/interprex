Glossary

    These definitions name the concepts in Postel's public interfaces.

    domain:
        One area of a development platform with its own provider interface:
        code hosting, tracker, code review, jobs or releases.
    provider:
        An implementation of one or more domain interfaces for an external
        system.
    adapter:
        A provider implementation that translates between Postel models and an
        external system's transport and data formats.
    client:
        The provider-owned code that authenticates and communicates with its
        external system.
    identity:
        The principal under which a provider authenticates an operation, such
        as a GitHub user or app installation. A review actor and a provider
        identity are different concepts.
    code review:
        One proposed change and its reviews, independent discussions, general
        conversation and outstanding review requests. A GitHub pull request is
        represented as a code review.
    review:
        One provider review record against one head commit. A review may be a
        draft or submitted, may come from the change author, and may contain
        findings.
    review author:
        The platform actor that owns a review record. The application that
        produced the review is a separate fact.
    review relationship:
        What the provider establishes between the review author and change
        author: change author, other or unknown. Unknown does not assert that
        the actors differ.
    draft review:
        A review that has not been submitted. It has no disposition or
        submission time.
    submitted review:
        A review carrying a disposition and submission time.
    finding:
        An inline review thread attached to the review in which it originated.
        Its initial comment, replies, source location, resolution status and
        outdated status remain together.
    independent discussion:
        An inline review thread with no originating review. Later replies do
        not change its origin.
    conversation comment:
        A general comment on the proposed change with no source location.
    review request:
        One currently outstanding request for an actor or team to review a
        change. The observed target and the provider address that can request
        it again are separate facts. It describes current state, not request
        history.
    review application:
        The provider application through which an actor created or submitted a
        review.
        It is attribution, not the actor and not the provider's authentication
        identity.
    refusal:
        A structured provider error returned when required data is missing,
        inconsistent or cannot be represented by a Postel model.
    record path:
        A validated relative object-store path used for exact reads, creates and
        prefix listings.
