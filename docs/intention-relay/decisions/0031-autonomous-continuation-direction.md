# ADR 0031: Post-M5 Autonomous Continuation Direction

## Status

Accepted 2026-08-30. This decision records the "Continue autonomously"
direction as an accepted future direction. It does not activate implementation:
the direction is bound to
[Milestone 5+](../architecture/11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment)
and is implemented only through a separately accepted activating specification
at the start of that milestone.

## Decision

The "Continue autonomously" claim from
[`m4plus_concept.md`](../m4plus_concept.md) is adopted as an accepted future
direction and owned by [architecture 13](../architecture/13-mandate-domain-and-durable-lifecycle.md):

- **Continue autonomously** creates or activates a **Build-mode Mandate** by
  default for future Mandate work;
- after a known terminal run disposition, the daemon records its terminal
  evidence and, when continuation remains enabled, returns the Mandate to
  `Active`; a pending coalesced continuation reason then admits a completely
  fresh run;
- there is no hidden retry count, automatic escalation threshold, or
  conversion of a known failure into an unknown effect; and
- Build mode is the default for **Continue autonomously**, while Plan mode
  remains meaningfully distinct (it denies ordinary project `write`/`edit`,
  and plan mutation remains its own typed plan operation).

The direction is additive to, and does not amend, the accepted Build Autopilot
direction of [ADR 0017](../decisions/0017-build-autopilot-and-plan-focus-continuity.md)
and [ADR 0018](../decisions/0018-plan-build-autopilot-activation-scope.md):
those records govern ordinary Plan/Build runs, while this direction governs
future Mandate continuation. Neither mode is a sandbox or a claim to constrain
programs running with the user's ordinary OS authority.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after
a later activating specification, and is bound to Milestone 5+ in the roadmap.

## Rationale

The authoritative package review of 2026-08-30 confirmed the "Continue
autonomously" claim (including the Build-mode Mandate default) is present in
`m4plus_concept.md` but absent from the authoritative documentation: no
architecture, ADR, or reconciliation row adopts it, and it is distinct from the
ordinary Build Autopilot direction of ADR 0017/0018. This decision adopts it as
an accepted future direction so the authoritative documentation no longer leaves
the feature unmapped, while preserving the project rule that no feature is
documented as implemented without code evidence.

## Normative invariants

1. Continue autonomously is Mandate continuation, not old-run resumption: it
   admits only a fresh run with a new `RunId`.
2. A known terminal disposition may return a Mandate to `Active` only after
   required graph terminalization owned by architecture 17 completes.
3. A known non-zero `execute` exit, typed validation failure, provider failure
   with durable terminal evidence, or known MCP result is a known outcome and
   may lead to the next fresh run; the user decides when a known failure means
   pause, stop, completion, revision, or needs-rework, except where an
   explicit delegated verifier has the corresponding operation.
4. Build mode is the default for Continue autonomously; Plan mode remains
   meaningfully distinct and neither mode is a sandbox.

## Failure semantics

- No hidden retry count, automatic escalation threshold, or conversion of a
  known failure into an unknown effect exists.
- Recovery never resumes a run, provider request, tool invocation, bridge
  operation, process, kernel cell, background task, child run, MCP process,
  or external effect; a fresh run may use only durable verified checkpoints
  and selected historical references.

## Compatibility and supersession

This decision supersedes the absence of the direction in architecture 13's
principle-level text. It does not amend ADR 0017/0018 or ordinary Plan/Build
behavior. The closed M4 baseline, M3/M4 bytes, and existing behavior remain
unchanged. Activation remains deferred: no code changes are authorized by this
decision.

## Security and residual risk

The direction remains trusted-local. Continuation admits fresh runs only and
never reattaches or resumes old external work; redaction stays central and
every activating specification must pass the fake-secret regression suite.

## Affected documents

- [`architecture/11-implementation-roadmap.md`](../architecture/11-implementation-roadmap.md)
- [`architecture/13-mandate-domain-and-durable-lifecycle.md`](../architecture/13-mandate-domain-and-durable-lifecycle.md)
- [`reconciliation/README.md`](../reconciliation/README.md)
- [`reconciliation/source-of-truth-matrix.md`](../reconciliation/source-of-truth-matrix.md)
- [`reconciliation/contradiction-register.md`](../reconciliation/contradiction-register.md)
- [`reconciliation/concept-supersession-index.md`](../reconciliation/concept-supersession-index.md)
- [`reconciliation/deferred-excluded-register.md`](../reconciliation/deferred-excluded-register.md)
- [`reconciliation/evidence-register.md`](../reconciliation/evidence-register.md)
- [`reconciliation/compatibility-register.md`](../reconciliation/compatibility-register.md)
- [`decisions/README.md`](README.md)

## Required evidence

No implementation evidence is claimed. The activating specification must declare
exact crates, DTO/wire/storage versions, feature profiles, coverage tiers,
fixtures, and outcome evidence, and pass `make quick`, `make verify`, and
Linux/Windows CI before acceptance.

## Non-goals

This decision does not implement the direction; it does not change M3/M4
behavior; it does not renumber M5--M9; it does not activate a crate, schema,
migration, protocol, or feature. It does not amend ADR 0017/0018 or ordinary
Plan/Build behavior. Automatic continuation after client disconnection,
autonomous harness goal mode, and production activation remain outside this
decision.
