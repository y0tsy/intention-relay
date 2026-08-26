# ADR 0018: Plan/Build Autopilot Activation Scope

## Status

Accepted as the activation-scope companion to ADR 0017. It preserves the
existing milestone documents and records the exact documentation transition
without silently activating production code.

## Decision

The accepted implementation slice is additive:

1. Update Plan semantics to planning focus with available, audited `execute`.
2. Preserve ordinary Plan project `write`/`edit` denial and plan-artifact
   ownership.
3. Add one Build Autopilot policy with no per-action confirmation for configured
   capabilities.
4. Add durable plan approval that starts a fresh Build run in the same Session
   by default and keeps the conversational context.
5. Add optional new-Session handoff only through a frozen safe snapshot and
   independent fresh run.
6. Preserve M3/M4 records, one-active-run, no-resume, unknown-effect,
   commit-before-effect, redaction and DTO-only boundaries.

## Non-goals

This activation does not add sandboxing, rollback, implicit authority,
automatic retry of started effects, new provider routing, or hidden permission
configuration. It does not delete or shorten existing architecture, roadmap,
reconciliation, closure, or research documents.

## Required implementation evidence

The owning activation must add contracts, storage/runtime integration,
Plan/Build outcome tests, same-session/new-Run tests, full-context handoff tests,
redaction and recovery tests, update machine-readable crate/test/coverage policy,
and pass `make quick`, `make verify`, architecture/doc checks and Linux/Windows
CI. Post-M4 documentation remains non-executable until that implementation
specification is separately accepted by the project workflow.
