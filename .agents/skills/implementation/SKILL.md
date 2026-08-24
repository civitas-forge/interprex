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

## The CLI Layer

Standout is the CLI framework, and the split is strict: lib crates and a
binary crate, and nothing crosses it.

The lib crates carry all logic as a rust api — rust parameters in, rust data
returned, no CLI vocabulary, fully testable without a terminal. They never
depend on clap, standout, view models, templates, styles, or app
construction.

The binary is a declarative standout app and nothing else: clap derive
annotations wire the command tree; handlers are thin adapters that translate
parsed arguments into library calls and library results into serializable
view models; rendering goes through template files and css themes, per output
mode — structured output for agents, rendered templates for humans. A handler
never prints, never emits ansi, never holds logic: it returns the view model
and the framework does the rest.

The split is also the sequence. The lib side is built first and completely —
the types, the options, the operations, and their tests — with no output and
no CLI anywhere, so all of it is unit testable and tested before a command
exists. Only when the library is done does the cli work start: the wiring,
the templates and styles, and the integration tests standout's own tooling
carries.

Tests follow the split: library behavior through its own api first; handlers
as typed calls; TestHarness for the in-process argv-to-output pipeline; a
spawned process only for seams the harness cannot model.

The canonical shape is the standout repo's todo-example — todo-core the
CLI-free library, tdoo the binary-only app. The binary's commands honor the
command-line contract stated in the repo's own docs/contracts/cli.lex.

## The Composition Root

Each binary crate carries a `cli` module — one file per command group — and it
is the one place that names every subsystem. It parses arguments into model
types and dispatches. No logic lives there.

Config loading is ownership-driven: each subsystem declares the config struct
for the sections it owns, and the binary's settings module composes them.

## What Earns A Separate Binary

One test, and it is not size: a subsystem earns a binary when its contract
serves a **different consumer at a different cadence**.

Both halves have to hold. A subsystem serving the same consumers under one
version stays a crate however large it grows, because splitting it buys a
version to coordinate and nothing else. A small thing whose consumer is
something else entirely, changing on its own schedule, has earned one — that is
what a separate binary buys: a fix ships without touching the other, and lands
for callers with nothing restarted.

The case that tests the rule and does not break it: a query engine that runs at
query time only, is linked into nothing on a regular path, and opens a
connection when a question is asked. It is one more command group, not another
binary.

The argument is reusable, which is why it is written down — someone will reach
for it, and reaching for it should mean applying this test rather than
re-deriving one.
