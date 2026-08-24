Crate Layout

    How the code is physically structured: one crate per subsystem, and no binary above them — the binary is whichever tool links these crates. Subsystem isolation — what a crate may declare, and cargo enforcing it — is the implementation skill's.

    Each subsystem states its functions in its own api.rs. A crate named for a domain carries that domain's vocabulary, and vocabulary is what a repo boundary exists to keep apart.

1. Subsystems

    The platform is two crates rather than one because a contract is per domain while a provider is per system. The contracts crate holds the five domain traits in the tools' own terms and names no provider; every consumer declares it ([./interface.lex]). One crate per provider holds the client and the five domain modules implementing those traits against its system. The bucket client and the secret-store client are a crate each.

    Beside the subsystems sit the model crate and sys, this repo's own and shared with no other.
