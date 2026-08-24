---
name: register
description: The writing register for this repo's design docs. Load before writing or editing any .lex doc here — the rules, the deletion test, and before/after examples of the slop this workspace bans.
---

# The Register

Docs here state the settled model and nothing else. A doc is right when
every sentence could have been written before the session that produced
it, and will still be true three years out.

## Rules

- Present tense, the model as it is. Never how it got here, what it
  replaced, or what it might become.
- No provenance: no dates, no "as of/currently/previously", no session
  or verification narration, no cross-reference chains. Session
  evidence — surveys, inventories, spike results — dies with the
  session; only its consequences land, stated as model facts.
- One fact, one home. Another doc may point, never restate.
  Overturning a decision means deleting its statement at its one home,
  not amending it.
- Two files are indexes, not homes, and are exempt from the rule
  above: `docs/verify.lex` (one-line checkable assertions — commands,
  variables, tiers, outputs, runtime needs — with no rationale) and
  `GLOSSARY.lex` (term definitions). A line there is a claim to check
  against the owning doc, never a second statement of it.
- The deletion test, per line: would removing this cause a future
  reader or agent to err? If not, cut it.
- Budget: a doc drifting past ~200 lines is compressing badly or
  hoarding facts that belong elsewhere.
- No history in the repo. What was tried, what it replaced and when are
  git log's and a pull request's; a doc carries none of it, and no file
  is set aside to hold it.

## Stance

A doc states the model from inside it. It never describes the model from
outside.

That is the general form of the banned-word list. Every entry in
`.AGENTIC_MUMBLE_JUMBLE` is a word available only from outside — naming a
relationship in the architecture instead of naming what the thing does.
The list catches the words that have already cost a commit; the rule
catches the ones nobody has coined yet.

Applied, one instance each:

| Outside | Inside |
|---|---|
| the seam between the two tools | one hands the other a repo, a ref and a writability |
| the store's query surface | what a query can ask of the store |
| the gate on publish | publish refuses unless the store holds every planned artifact |
| a round mints another round | a round opens another round |
| the darwin leg | the darwin produce runs |
| it rides the same path | it is requested the way every reviewer is |
| the withheld arm | the run with the content withheld |

The test for any sentence: does it describe the thing, or describe the
design of the thing? A metaphor is what you reach for when you are
commenting on a design rather than stating it — so the metaphor is the
symptom and the view from above is what produced it.

## The README

README.lex is the one product-facing doc and inverts the stance: it
describes the tool from outside, to a reader who does not yet hold the
model. It opens by naming the category and function in plain words — "a
Rust library that abstracts development-platform apis into domains" —
before any word this repo defines. Everyday vocabulary first, project
vocabulary second; goals stated as reader benefits; structure shown as
concrete lists, with canonical names bound to their plain labels where
the reader first meets them.

What a README never carries: definition by relation ("linked crates
living inside whichever tool links it" says nothing to a newcomer that
"a Rust library" does not say better), chains of design verdicts
readable only from inside the finished model, or implementation
mechanics — deployment, module layout — at intro altitude. The
banned-word list and the deletion test still apply.

## Before / after

Bad:  2026-07-30: verified across all 22 repos on origin/main that the
      manifest fan-out is removed.
Good: A dep's payload never drives further fetches.

Bad:  We decided to kill lanes because agents kept reintroducing them.
Good: There is no lane. Tasks and triggers are declared; CI structure
      is derived from them.

Bad:  Currently the provider takes just one command (this may change as
      we learn more).
Good: A tool reaches its provider through exactly one command.

Bad:  The new naming grammar (replacing the old rust-triple scheme)
      is name-version-platform-arch.
Good: Asset names derive from the grammar: name, version, platform,
      arch.

## Mechanics

Lex format: load the lex-primer skill. Lint with `lexd check`. The
pre-commit hook prints word deltas and slop tells — warnings to act
on, never blocks.
