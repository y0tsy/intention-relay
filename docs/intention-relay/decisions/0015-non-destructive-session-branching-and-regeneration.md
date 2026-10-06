# 0015: Non-Destructive Session Branching and Regeneration

## Status

Accepted.

## Decision

Future user-initiated session branching creates an independent ordinary child Session with immutable conversation
lineage and frozen causal context. It never rewrites source history, rolls back effects, copies active work, or
transfers provider, tool, bridge, kernel, or MCP authority.

`Regenerate response` is a user-turn fork followed by separately idempotent ordinary execution. A credential-free
provider-profile override is permitted only for this regeneration flow and remains a future default proposal.

## Invariants

- only committed-user-turn and completed-assistant-turn boundaries are valid;
- child context is a flattened immutable credential-free snapshot with no live
ancestor, sibling, or current-state reconstruction;
- roots use deterministic conversation-tree identities, children use immutable
parent/operation/boundary lineage, and lineage audit is separate from Session and Run sequences;
- workspace/external state is explicitly `Unverified`, never rollback evidence;
- M3/M4 bytes, selections, events, cursors, snapshots, replay, recovery, and M4
tool denial retain their recorded meaning.

## Compatibility and non-goals

Activity/UI implementation, workspace clone/rebind, autonomous branching, destructive retention, and implementation
activation remain separate.

Owner: architecture 23. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).

Provenance: `m4plus_concept.md`, selected session-branching and regeneration material, reconciled against architectures
04, 14, 15, and 18--22.
