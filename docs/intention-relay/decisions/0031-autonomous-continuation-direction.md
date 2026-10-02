# 0031: Post-M5 Autonomous Continuation Direction

## Status

Accepted 2026-08-30. Not implemented; activation requires an activating specification.

## Decision

The "Continue autonomously" claim from `m4plus_concept.md` is adopted as an accepted future direction owned by
architecture 13:

- **Continue autonomously** creates or activates a **Build-mode Mandate** by
default for future Mandate work;
- after a known terminal run disposition, the daemon records its terminal
evidence and, when continuation remains enabled, returns the Mandate to `Active`; a pending coalesced continuation
reason then admits a completely fresh run;
- there is no hidden retry count, automatic escalation threshold, or
conversion of a known failure into an unknown effect; and
- Build mode is the default for **Continue autonomously**, while Plan mode
remains meaningfully distinct (it denies ordinary project `write`/`edit`, and plan mutation remains its own typed plan
operation).

The direction is additive to, and does not amend, the accepted Build Autopilot direction of [ADR
0017](0017-build-autopilot-and-plan-focus-continuity.md) and [ADR 0018](0018-plan-build-autopilot-activation-scope.md):
those records govern ordinary Plan/Build runs, while this direction governs future Mandate continuation. Neither mode is
a sandbox or a claim to constrain programs running with the user's ordinary OS authority.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after its own activating specification, and
remains bound to Milestone 5+.

## Normative invariants

1. Continue autonomously is Mandate continuation, not old-run resumption: it
admits only a fresh run with a new `RunId`.
2. A known terminal disposition may return a Mandate to `Active` only after
the required graph terminalization owned by architecture 17 completes.
3. A known non-zero `execute` exit, typed validation failure, provider failure
with durable terminal evidence, or known MCP result is a known outcome and may lead to the next fresh run; the user
decides when a known failure means pause, stop, completion, revision, or needs-rework, except where an explicit
delegated verifier has the corresponding operation.
4. Build mode is the default for Continue autonomously; Plan mode remains
meaningfully distinct and neither mode is a sandbox.

## Failure semantics

- No hidden retry count, automatic escalation threshold, or conversion of a
known failure into an unknown effect exists.
- Recovery never resumes a run, provider request, tool invocation, bridge
operation, process, kernel cell, background task, child run, MCP process, or external effect; a fresh run may use only
durable verified checkpoints and selected historical references.

## Rationale

The claim (including the Build-mode Mandate default) was present in `m4plus_concept.md` but absent from the
authoritative documentation, and it is distinct from the ordinary Build Autopilot direction of ADR 0017/0018. Adopting
it records the direction without documenting any feature as implemented.

## Compatibility and non-goals

This decision supersedes the absence of the direction in architecture 13's principle-level text. It does not amend ADR
0017/0018 or ordinary Plan/Build behavior. The closed M4 baseline, M3/M4 bytes, and existing behavior remain unchanged,
and no code changes are authorized by this decision.

Automatic continuation after client disconnection, autonomous harness goal mode, and production activation remain
outside this decision. M5-M9 are not renumbered.

Owner: architecture 13. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).
