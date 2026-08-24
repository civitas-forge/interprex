Glossary

    These words mean here what this repo says they mean, and a text using one
    of them in another sense belongs somewhere else.

    domain:
        One area of the development platform under one contract — repo
        (code hosting), tracker (issue tracking), pr (code review), jobs
        (ci) or releases. Each names its own provider.
    contract:
        The trait stating a domain's operations in the tools' own vocabulary,
        naming no provider.
    operation:
        One call a contract states: what is handed in, what comes back, and
        the refusals it can raise.
    refusal:
        The structured error a provider raises for a fact it cannot express,
        naming the provider and the fact, at the call that needs it.
    provider:
        The implementation answering a contract against one system — the
        github provider, the gcs provider. A deployment selects one per
        domain. Never an agent cli.
    client:
        The code inside a provider that reaches its system — one client per
        system, holding its authentication, transport and retry, shared by
        every tool that links it.
    installation:
        The grant under which an app authenticates on the platform, carrying
        its permissions and its tokens.
    token:
        A credential opening one identity's access — an installation's on the
        platform, a configuration-scoped service token on the secret store.
        A platform token is fetched, cached and refreshed in-client and never
        persisted; a service token arrives through the environment.
    store:
        One of the five places an entity lives: the platform, git, the
        bucket, the secret store, the host.
    lookup:
        A read answered by one exact address.
    listing:
        A read of everything under one path prefix.
    scan:
        A question a store answers only by reading and filtering, because it
        cuts across the store's own order.
    computed answer:
        A result computed from records at query time and never written back.
    snapshot:
        A platform read captured as a record, so a later query answers from
        the record rather than from the platform.
    identifier:
        The string naming one entity; identifier and path are one form under
        one parser.
    namespace:
        The path an identifier is — one form under one parser; a prefix of
        it is a query.
    schema version:
        The number in an object's name stating which schema its record
        carries; one number covers a domain.
