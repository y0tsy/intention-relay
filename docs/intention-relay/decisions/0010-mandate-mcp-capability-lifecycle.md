# 0010: Mandate MCP Capability Lifecycle

## Status

Accepted.

## Decision

Future Mandate MCP work uses the fixed `mcp` ToolId and one daemon-owned Rust capability path to acquire, discover, normalize, freeze, and invoke run-local external capabilities. Discovery begins only through a typed Mandate source proposal and does not require a retained user-created connection or complete method catalog.

Successful discovery creates immutable capability revisions and an ordered accumulated run-local selection. It never creates another ToolId, registry, plugin, authority, lifecycle owner, or scheduler input. Every invocation binds an exact capability revision and typed input before external work. Endpoint and credential material remain private and never enter durable/public meaning.

## Invariants

- one fixed `mcp` ToolId and composition-only capability path exist;
- discovery and invocation have distinct idempotency identities and external attempts;
- every model step and invocation binds exact immutable selection/capability meaning;
- current server, schema, endpoint, credential, registry, configuration, or ancestry cannot repair stored meaning;
- source/server/catalog/result data grants no lifecycle, registry, scheduler, child, verifier, or user authority;
- started ambiguous work pauses only its owning Mandate, and recovery never retries, resumes, reattaches, rediscovers, or repeats it;
- local stdio resources are run-owned, disposed at terminalization, and never shared or reattached; and
- M3/M4 and retained bounded-MCP/RLM history remains unchanged and receives no synthetic MCP state.

## Compatibility and non-goals

Dynamic run-local capability acquisition supersedes retained fixed-catalog/no-discovery/policy-gated MCP research only for future Mandate execution. It does not activate direct MCP administration, a listener, inbound attachment, plugins, SDKs, network/process behavior, schema, migration, crate, protocol implementation, quality policy, or runtime code.

## Primary owner and evidence

[Mandate MCP Capability Lifecycle](../architecture/18-mandate-mcp-capability-lifecycle.md) owns detailed behavior. A later implementation requires canonical/schema/idempotency/fault-injection/recovery/redaction/protocol/compatibility/outcome evidence through the standard quality gate.

## Provenance

`m4plus_concept.md`, Mandate MCP capability selection and dynamic acquisition sections, plus retained bounded MCP gateway material.
