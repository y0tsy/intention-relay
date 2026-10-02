# 0007: Unified Tool Registry and Direct Mandate Tool Admission

## Status

Accepted.

## Decision

Future Mandate tool work uses one Rust-owned, composition-assembled registry
with fourteen fixed slots, immutable intended owners, and `Reserved` or `Active`
entries. A Mandate run freezes its exact registry, descriptor, hook, and
model-visible tool selection in immutable execution meaning.

Compatible selected active Mandate descriptors are directly admitted after typed
validation and actual readiness checks. Confirmation, risk selectors, corridors,
root-origin rules, quotas, reservations, and secondary tool authority cannot
veto them. `WorkspaceRoot` is a Mandate default relative base and `execute` CWD,
not a containment boundary. Existing ordinary behavior remains unchanged.

## Invariants

- composition alone assembles active descriptors; no second registry or bypass
  path exists;
- `Reserved` slots are not model-visible or executable;
- registry/descriptor changes cannot reinterpret stored selections;
- `ToolEffectProfile` is descriptive, not authority or sandboxing;
- a tool effect starts only after durable `ToolCallStarted` evidence;
- tools never automatically retry or resume after recovery;
- an unknown started effect pauses only its owning Mandate and requires exact
  reconciliation before later fresh work.

## Compatibility and non-goals

This resolves CON-001 and CON-002 for future Mandate execution only. M3/M4
records, ordinary WorkspaceRoot containment, ordinary Plan/Build confirmation,
M4 tool-call denial, and recovery behavior remain unchanged. This decision does
not define child, MCP, verifier, bridge, kernel, scheduler, provider evolution,
SQL, protocol implementation, crates, or runtime activation.

## Primary owner and evidence

[Tool Registry and Direct Mandate Tool Loop](../architecture/15-tool-registry-and-mandate-tool-loop.md)
owns detailed rules. A later implementation requires registry/selection golden
vectors, no-bypass and no-current-state-reconstruction fixtures, direct-admission
and workspace matrices, transaction/effect/recovery fixtures, negotiated replay,
historical M4 preservation, redaction evidence, and full quality-gate acceptance.

## Provenance

`m4plus_concept.md`, unified registry, direct active-descriptor admission,
WorkspaceRoot, and model-tool-loop sections.
