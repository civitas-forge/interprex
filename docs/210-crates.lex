Crate Layout

    How the code is physically structured: one binary, and one crate per subsystem beneath it. Isolation is enforced by cargo, not discipline: a subsystem crate declares no other subsystem, so an attempt to reach into one fails to compile rather than passing review.

    Subsystems meet each other in common data and nowhere else. Each states its functions in its own api.rs, and each declares the model crate carrying the types they speak, and sys, holding all shell, disk and path access in one small injectable set. Neither is shared with another repo: a crate named for a domain carries that domain's vocabulary, and vocabulary is what a repo boundary exists to keep apart.

1. Subsystems

    Two crates, and they are two rather than one because a contract is per domain while a client is per backend. The contracts crate holds the five domain traits in the tools' own terms and names no backend; every consumer declares it. One crate per backend holds the client and the five domain modules implementing those traits against it.

    A consumer depends on the contracts and receives a backend at its own composition root, so the rules that read a contract are tested against a fake one: no network, no third-party account, and nothing left behind in a real repo.
