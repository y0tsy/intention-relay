# 0002: External Attempt Evidence and Unknown-Effect Reconciliation

## Status

Accepted.

## Decision

Every future external attempt is classified as `AdmittedBeforeStart`, `Started`,
`KnownTerminal`, or `UnknownTerminal`. Known validation failures, known provider
failures, and known non-zero process exits remain known outcomes. An unknown
terminal effect exists only after started work when its terminal effect cannot
be durably proven.

An unknown effect pauses the owning Mandate in `PausedAwaitingDecision`. It
blocks automatic continuation, retry, reattachment, rediscovery, and the next
model step. Only the user or explicitly scoped future verifier authority may
reconcile the exact uncertainty to a later fresh Active path or Stopped state.

## Invariants

- external work is never inside a durable transition transaction;
- durable mutation is atomic and publication follows commit plus scoped reread;
- recovery never repeats old external work;
- child/verifier uncertainty cannot fabricate uncertainty for a parent or target.

## Compatibility and non-goals

This does not alter M4 interruption behavior or define package-specific attempt
records, verifier authority, MCP invocation, tool loop, or retry policy.

## Evidence

Future work requires crash/cancel phase fixtures, atomic pause evidence,
no-repeat recovery tests, exact-reference reconciliation tests, and fake-secret
redaction coverage.

## Provenance

`m4plus_concept2.md`, external-attempt taxonomy and recovery sections.
