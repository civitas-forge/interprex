Writing Records

    The object store holds what is neither code nor platform state. Several
    writers put records into it and one reader queries across all of them, so
    the discipline below is what every writer meets. What a given record holds
    is the writer's; how it is written, named and reached is here.

    The store is a file store and not a service. Nothing but a path reaches it,
    and the choice of path is therefore a modeling decision with named losers.

1. Writers Only Create

    Nothing is ever added to an existing object. Every record enters as its own
    object and no writer reopens one.

    A network store has no cheap append and no locking worth relying on, and a
    store that goes back to edit what it has written becomes a concurrency
    problem whatever it runs on. Because nothing is modified, a writer needs to
    know nothing about what is already there: two writers never collide, and
    none of them lists the store first.

2. One Object, One Record

    One object holds one record of one kind, and its name is that record's key.

    The exception is a record kind that one process emits many of. Those go in
    one object per process, and what keeps the names apart is a fact the
    process already holds — the instant it started. No writer reopens what
    another wrote.

3. Paths

    A path serves direct access; a query serves analysis. Only questions along
    the path's own order are answerable by listing, and every question cutting
    across it is a scan — so segment order chooses which one question is cheap,
    and the rest are stated as scans rather than discovered as slow.

    Records shard by date, so a listing is bounded and a window is a prefix.
    Below the date, a writer groups by whatever makes its one worthwhile
    listing cheap.

    Nothing in a path is a second declaration. Every segment is composed from
    the record, and a path is never parsed back into one. A writer knows every
    segment before it writes, because whatever prepared its work fixed them.

4. The Schema Version

    It goes in the object's name, because the name is the only part of an
    object a query reads without opening it. Carried inside the record it would
    be invisible until parsed, so a reader on a new schema would open every old
    object to learn it should skip it. Carried as a path segment it would
    re-shard on a change that is about neither dates nor grouping.

    One number covers a domain, whose records move together, and a query names
    the versions it reads.

5. Queries Read

    Everything computed from the records — a metric, an aggregate, a rate — is
    recomputed on the next query and never written back. Nothing has to be
    invalidated, and no stored answer can disagree with the records behind it.

    A fact that appears only in a computed result and in no record is a fact
    nothing captured.

6. Retention

    Nothing expires. No lifecycle rule runs over these prefixes and no command
    deletes under them.

    A deleted record does not narrow an answer, it changes one: a measurement
    over a window whose records expired reports a difference the expiry
    produced. What grows is scan cost rather than storage, and scan cost is
    answerable from the same queries.
