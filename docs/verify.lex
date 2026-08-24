Verify

    Every line is a checkable assertion; the reasoning lives in the doc the
    line points at. An agent verifying postel walks this file top to bottom,
    checks each line against the code, and reports the lines that do not
    hold.

1. Interface

    - The repo builds as crates only; no target produces a binary
      ([./210-crates.lex]).
    - The contracts crate declares the five domain traits — repo, tracker,
      pr, jobs, releases — and depends on no provider ([./architecture.lex]).
    - Each provider crate implements all five domains against its one
      system ([./210-crates.lex]).
    - The bucket client and the secret-store client are one crate each.
    - Every subsystem crate states its functions in api.rs.
    - No crate shells out to gh ([./200-stack.lex]).

2. Environment

    - No crate reads a configuration file; every deployment fact arrives as
      an environment variable ([./architecture.lex]).
    - A missing variable raises a structured error naming it, at the first
      call that needs it — never at construction.
    - One selection variable per domain, declared by the contracts crate;
      unset means github.
      :: tbd :: The five selection variables' names.
    - Per-provider identity variables carry names and ids only, never a
      value.
      :: tbd :: The github provider's identity variable names.
    - The secret-store client names the one variable carrying its own
      token.
      :: tbd :: That variable's name.

3. Test Tiers

    - Rules that read a contract run against a fake provider: no network, no
      third-party account, nothing left behind in a real repo
      ([./interface.lex]).
    - Above that, the verification ladder is the implementation skill's;
      postel adds no tier of its own.

4. Build Outputs

    - Crates, and nothing else: no binary, no image, no docs build, no
      release artifact of its own.

5. Runtime

    - Postel runs inside whichever tool links it; nothing here owns a
      process, a server or a schedule.
    - The host store holds only what is live; anything outlasting an
      environment's container is written to a mounted volume, the bucket or
      the platform ([./110-data-access.lex]).
    - Nothing under the record prefixes expires, and no command deletes
      under them ([./contracts/records.lex]).

6. Siblings

    - kent links the contracts crate and drives the pr domain
      ([./architecture.lex]).
    - edward links the repo and jobs domains and the secret-store client.
    - minsky and sam write records through the bucket client.
    - Postel links no sibling tool.
