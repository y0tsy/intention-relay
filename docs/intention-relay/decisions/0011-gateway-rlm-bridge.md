# 0011: Gateway/RLM Bridge

## Status

Accepted.

## Decision

Future Gateway/RLM attachment is a typed, daemon-owned ingress to the one Rust-owned capability path. It uses a
negotiated bridge capability, an ephemeral daemon-issued attachment grant, and durable `BridgeOperationId` idempotency
bound to the daemon-assigned `ToolCallId`. It neither creates a second gateway, registry, listener, daemon, authority
plane, nor direct primitive path.

The bridge contract selection is immutable future run meaning. Live grants, channels, daemon epochs, cursors,
kernels, and resources are ephemeral operational evidence and never durable meaning or authority. Equal bridge
operations return durable evidence; changed reuse fails before effect.

## Invariants

- architecture 15 owns descriptor selection, direct admission, tool-loop facts,
`ToolCallId`, and generic effect evidence;
- architecture 18 owns MCP lifecycle, so bridge transport can carry only safe
MCP projections;
- attachment loss does not cancel a run, and recovery never reissues a grant,
reattaches, retries, resumes, redispatches, or repeats old work; and
- replay and lookup are negotiated, read-only, history-before-live, and cause
no external effect.

## Compatibility and non-goals

M3/M4 bytes and ordinary behavior remain unchanged, and M4 tool calls remain denial evidence. Retained RLM
bridge/child/activity semantics remain historical where they conflict with the direct-admission model.
The bridge is a trusted-local product control, not a sandbox or privilege boundary; kernel lifecycle, RLM executor
topology, provider evolution, Skills/Goals, context, session branching, activity/UI, and MCP administration remain
separate.

Owner: architecture 19. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).

Provenance: `m4plus_concept.md`, selected typed daemon host bridge and gateway protocol, bridge supersession,
and retained RLM material.
