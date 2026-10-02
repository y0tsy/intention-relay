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

The closed attempt-evidence family (`ExternalAttemptPhaseDto`,
`ExternalAttemptEvidenceDto`) is future detail owned by
[architecture 13](../architecture/13-mandate-domain-and-durable-lifecycle.md).
It is shared by `execute`, kernel/bridge, MCP discovery and invocation,
provider-adjacent external work, and child work. Only daemon-owned execution and
recovery logic classifies an attempt: before start, a result is a known
pre-effect outcome, including `InterruptedBeforeStart`; after start without
durable terminal proof, loss, cancellation, or restart records
`ExternalEffectUnknown`. A known nonzero exit, typed provider/MCP failure,
schema mismatch, or validated terminal result remains known. Unknown evidence
atomically prevents the next model step, automatic retry or continuation,
rediscovery, reattachment, and old-work resume; for Mandate work it atomically
moves `Working` to `PausedAwaitingDecision`. Recovery writes missing terminal
outcomes and the run transition to `Interrupted` atomically and never opens
another model step, repeats a tool, or reconstructs a remote continuation.

## Invariants

- external work is never inside a durable transition transaction;
- durable mutation is atomic and publication follows commit plus scoped reread;
- recovery never repeats old external work;
- child/verifier uncertainty cannot fabricate uncertainty for a parent or
  target.

## Compatibility and non-goals

This does not alter M4 interruption behavior or define verifier authority, MCP
invocation, tool loop, or retry policy; package-specific attempt records remain
deferred.

Owner: architecture 13. Evidence: activating specification per
[architecture 12](../architecture/12-quality-gates-and-makefile.md).

Provenance: `m4plus_concept.md`, external-attempt taxonomy and recovery
sections.
