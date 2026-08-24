Data Access

    A tool reading this platform keeps no server and no store of its own: the platform is the store ([./100-platform.lex]). It answers only the questions its api exposes, and nothing a caller does adds one, so what can be asked constrains what may be modeled. This doc owns that constraint and the method for working inside it.

1. The Coupling

    Where a tool owns a database, the model settles first and access follows: any field can be indexed, any query added later. Neither holds here. A model whose questions the platform cannot answer is not implementable, and the set of questions it will answer, examined after the fact, is examined once the schema already sits in every repo. Model and access settle together, per domain.

2. The Stores

    Five, and each answers a different set of questions. Every entity names the one it lives on. Two of them are the platform and git; outside those there are exactly three more, and no fourth is added: an object store — a file store, never a service — the secret store, and the host, which holds only what is live.

    Stores:
        | Store | Holds | Answers cheaply | Answers by scan, index, or not at all |
        | github | releases and their assets, issues, pull requests, workflow runs, packages | an exact address; one hop down its hierarchy | anything spanning repos; anything off the hierarchy |
        | git | a repo's declarations | the whole set at a ref | nothing else is asked of it |
        | bucket | what is neither code nor platform state: session records, access events, the build cache | a lookup or a prefix listing by path | anything off the path, which is read and computed ([#5]) |
        | secret | credential configurations, per repo and per tier | the values one configuration holds, opened by that configuration's token | anything across configurations; any read without a token |
        | host | live sessions, workspaces, environments | a lookup by path | anything else |

        :: table align=llll header=1 ::

    Github holds no arbitrary query: no join, no secondary index, no filter on a field its api does not expose. A question off the hierarchy is answered by fetching and filtering locally, by reading records and computing it ([#5]), or not at all — and which of the three is a stated decision, never a discovery at implementation time.

    The bucket is a file store and not a service: nothing but a path reaches it, so the identifier discipline ([#4]) is all the query language it has.

    The host store holds only what is live. Nothing in it is shared, and an environment's container is discarded when it ends, so anything that must outlast it is written to a mounted volume, to the bucket or to the platform.

3. The Method

    Per domain, in order:

    - Enumerate what the tools emit, not what a design would like to have.
    - Keep what someone asks a question of; drop the rest.
    - State each question, and who asks it — dependency resolution, troubleshooting, performance.
    - Map each question onto a store: answered natively, answered by adaptation, or unanswered.
    - Name what is left unanswered, so the gap is a decision rather than a surprise.

    The mapping feeds back: an address that does not resolve in one call is a modeling error, not an access cost to absorb.

4. Identifiers

    A namespace is a path. The identifier and the path are one form under one parser, so a prefix is a query and a listing is a directory read. Which questions a segment order makes cheap and which it turns into scans is stated with the record paths ([./contracts/records.lex]).

    An identifier matched across a boundary travels as one string, since the far side cannot read the pieces separately. An identifier used within one read boundary lives as separate fields, since nothing has to carry it anywhere.

5. Computed Answers

    A question no store answers by address or by listing is answered by reading the records and computing it. What a computed answer may and may not do is the records contract's ([./contracts/records.lex]).
