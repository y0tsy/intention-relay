# 0007: Unified Tool Registry

## Status

Accepted.

## Decision

Future tool work uses one Rust-owned, composition-assembled registry with fourteen fixed slots, immutable intended
owners, and `Reserved` or `Active` entries.

`WorkspaceRoot` is a default relative base and `execute` CWD, not a containment boundary. Existing ordinary behavior
remains unchanged.

## Invariants

- composition alone assembles active descriptors; no second registry or bypass
path exists;
- `Reserved` slots are not model-visible or executable;
- registry/descriptor changes cannot reinterpret stored selections;
- `ToolEffectProfile` is descriptive, not authority or sandboxing;
- a tool effect starts only after durable `ToolCallStarted` evidence;
- tools never automatically retry or resume after recovery;
- a started effect without terminal proof commits a bounded partial result.

## Compatibility and non-goals

M3/M4 records, ordinary WorkspaceRoot containment, ordinary Plan/Build confirmation, M4 tool-call denial, and recovery
behavior remain unchanged. This decision does not define MCP, bridge, kernel, provider evolution, SQL, protocol
implementation, crates, or runtime activation.

Owner: architecture 15. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).

Provenance: `m4plus_concept.md`, unified registry, WorkspaceRoot, and model-tool-loop sections.
