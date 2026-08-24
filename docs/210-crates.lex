Crate Layout

    How the code is physically structured: one crate per subsystem, and no binary above them — the binary is whichever tool links these crates. Subsystem isolation — what a crate may declare, and cargo enforcing it — is the implementation skill's.

    Each subsystem states its functions in its own api.rs. The model crate and sys are this repo's own, shared with no other: a crate named for a domain carries that domain's vocabulary, and vocabulary is what a repo boundary exists to keep apart.

1. Subsystems

    The platform is two crates rather than one because a contract is per domain while a client is per backend. The contracts crate holds the five domain traits in the tools' own terms and names no backend; every consumer declares it. One crate per backend holds the client and the five domain modules implementing those traits against it. The bucket client and the secret-store client are a crate each.

    A consumer depends on the contracts and receives a backend at its own composition root, so the rules that read a contract are tested against a fake one: no network, no third-party account, and nothing left behind in a real repo.
