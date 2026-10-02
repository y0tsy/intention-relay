# 0018: Plan/Build Autopilot Activation Scope

## Status

Accepted as the activation-scope companion to [ADR 0017](0017-build-autopilot-and-plan-focus-continuity.md).

## Decision

The accepted implementation slice is additive:

1. Plan semantics become planning focus with available, audited `execute`.
2. Ordinary Plan project `write`/`edit` denial and plan-artifact ownership are
preserved.
3. One Build Autopilot policy admits the configured active capabilities with
no per-action confirmation.
4. Durable plan approval starts a fresh Build run in the same Session by
default and keeps the conversational context.
5. The optional new-Session handoff uses only a frozen safe snapshot and an
independent fresh run.
6. M3/M4 records and the one-active-run, no-resume, unknown-effect,
commit-before-effect, redaction, and DTO-only boundaries are preserved.

This activation adds no sandboxing, rollback, implicit authority, automatic retry of started effects, provider routing,
or hidden permission configuration, and it deletes or shortens no existing architecture, roadmap, reconciliation,
closure, or research document.

Owner: architecture 07. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).
