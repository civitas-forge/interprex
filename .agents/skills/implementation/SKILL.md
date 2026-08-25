---
name: implementation
description: How the tools in this org are built — modeling first, the verification ladder, the config and CLI layers, the composition root, and what earns a separate binary. Load before writing or restructuring code in any of these repos, before adding a crate, and before proposing that something become its own binary.
---

# Implementation

These hold for every tool here. None of them carries domain vocabulary, and
nothing outside the code reads them — what a caller may rely on is the
command-line contract in each repo, not this.

## Model First

Before any subsystem: the data models and the command tree. Every subsystem
crate depends on the model crate and on no other subsystem, so a model that is
wrong is wrong in every subsystem at once and has to be right before any of
them is written.

API signatures and call flows get fleshed out early, even as passthrough, to
prove the data exchanged carries what each side needs before implementations
exist.

## Subsystems In Isolation

Non-negotiable: each subsystem is written, tested and finished in isolation.
One sees only its own domain — the changelog manager only changelog things, the
version bumper only versions, a builder only its block and target.

Subsystems share data models, never code, and never reach into each other.
That is enforced by the crate structure rather than by discipline: a subsystem
crate declares no other subsystem in its `Cargo.toml`, so an attempt to reach
into one fails to compile instead of passing review.

## The Verification Ladder

Each subsystem climbs its own before anything composes it:

1. Pure unit tests.
2. Tests against an injected sys — all shell, disk and path access behind one
   small injectable set.
3. Fixtures, inside the real image.
4. One real consumer, just this command.
5. That command across every consumer that has it.

The first three rungs are cheap and local; the last two are what prove a real
consumer. A subsystem that has not climbed its own ladder is not ready to be
composed, and a composite assembled from unfinished parts fails at the level
where the failure is most expensive to diagnose.

## Configuration

Clapfig is the config layer. Each subsystem declares its own config struct;
composition, layering, strict unknown-key errors naming file, key and line,
error rendering, template generation and json-schema export are clapfig's.

Internal settings load through the same layer as a repo's own declarations, so
a consumer overrides an internal setting exactly where one is exposed and
nowhere else. A key no subsystem exposed is not a passthrough: it fails at
load.

Cross-field invariants live in the post-validate hook, in code. Generated
config files carry a schema directive, and tombi is the editor toolchain those
schemas target. Format-preserving rewrites of a declaration file use
`toml_edit`.
