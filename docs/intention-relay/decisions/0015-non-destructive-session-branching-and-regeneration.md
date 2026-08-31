# 0015: Non-Destructive Session Branching and Regeneration

## Status

Accepted.

## Decision

Future user-initiated session branching creates an independent ordinary child
Session with immutable conversation lineage and frozen causal context. It never
rewrites source history, rolls back effects, copies active work, or transfers
Mandate, child, verifier, provider, tool, bridge, kernel, or MCP authority.

`Regenerate response` is a user-turn fork followed by separately idempotent
ordinary execution. It does not create a Mandate, trigger reason, or bypass
fresh admission. A credential-free provider-profile override is permitted only
for this regeneration flow and remains a future default proposal.

## Invariants

- only committed-user-turn and completed-assistant-turn boundaries are valid;
- child context is a flattened immutable credential-free snapshot with no live
  ancestor, sibling, or current-state reconstruction;
- roots use deterministic conversation-tree identities, children use immutable
  parent/operation/boundary lineage, and lineage audit is separate from Session
  and Run sequences;
- workspace/external state is explicitly `Unverified`, never rollback evidence;
- ordinary fork policy ceilings never become Mandate admission or child-graph
  quotas; and
- M3/M4 bytes, selections, events, cursors, snapshots, replay, recovery, and
  M4 tool denial retain their recorded meaning.

## Compatibility and non-goals

This decision is documentation-only. It activates no crate, migration, protocol,
UI, schema, provider behavior, or quality-policy change. Mandate association,
activity/UI implementation, workspace clone/rebind, autonomous branching,
destructive retention, and implementation activation remain separate.

## Primary owner and evidence

[Non-destructive session branching and regeneration](../architecture/23-non-destructive-session-branching-and-regeneration.md)
owns detailed behavior. A later implementation requires canonical, transaction,
migration, protocol, recovery, isolation, redaction, cross-platform, and outcome
evidence through the standard quality gate.

## Provenance

`m4plus_concept.md`, selected session-branching and regeneration material,
reconciled against architectures 04 and 13--22.
