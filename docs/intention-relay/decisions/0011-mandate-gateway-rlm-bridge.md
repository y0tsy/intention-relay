# 0011: Mandate Gateway/RLM Bridge

## Status

Accepted.

## Decision

Future Gateway/RLM attachment is a typed, daemon-owned ingress to the one
Rust-owned capability path. It uses a negotiated bridge capability, an ephemeral
daemon-issued attachment grant, and durable `BridgeOperationId` idempotency
bound to the daemon-assigned `ToolCallId`. It neither creates a second gateway,
registry, listener, daemon, authority plane, nor direct primitive path.

The bridge contract selection is immutable future Mandate meaning. Live grants,
channels, daemon epochs, cursors, kernels, and resources are ephemeral
operational evidence and never durable meaning or authority. Equal bridge
operations return durable evidence; changed reuse fails before effect.

## Invariants

- architecture 15 owns descriptor selection, direct admission, tool-loop facts,
  `ToolCallId`, and generic effect evidence;
- architecture 13 owns Mandate uncertainty and reconciliation;
- architecture 17 owns child Mandate creation and verifier authority, so
  `sub_agent` bridge ingress cannot revive retained RLM child identities;
- architecture 18 owns MCP lifecycle, so bridge transport can carry only safe
  MCP projections;
- attachment loss does not cancel a run, and recovery never reissues a grant,
  reattaches, retries, resumes, redispatches, or repeats old work; and
- replay and lookup are negotiated, read-only, history-before-live, and cause
  no external effect.

## Compatibility and non-goals

M3/M4 bytes and ordinary behavior remain unchanged. M4 tool calls remain denial
evidence. Retained RLM bridge/child/activity semantics remain historical where
they conflict with the Mandate graph and direct-admission model. The bridge is a
trusted-local product control, not a sandbox or privilege boundary.

This decision does not activate a crate, Python dependency, protocol, listener,
kernel, storage schema, migration, runtime, quality policy, or production work.
Kernel lifecycle, RLM executor topology, provider evolution, Skills/Goals,
context, session branching, activity/UI, and MCP administration remain separate.

## Primary owner and evidence

[Mandate Gateway/RLM Bridge](../architecture/19-mandate-gateway-rlm-bridge.md)
owns detailed behavior. A later implementation requires canonical, authority,
idempotency, fault/recovery, protocol, compatibility, redaction, cross-platform,
and outcome evidence through the standard quality gate.

## Provenance

`m4plus_concept.md`, selected typed daemon host bridge and gateway protocol,
Mandate bridge supersession, and retained RLM material.
