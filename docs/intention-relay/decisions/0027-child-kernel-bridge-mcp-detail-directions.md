# 0027: Post-M5 Kernel, Bridge, and MCP Detail Directions

## Status

Accepted 2026-08-30. Not implemented; activation requires an activating specification.

Amended 2026-10-05 by [ADR 0053](0053-sub-agent-and-fork-limits-by-precedent.md): every numeric bridge and kernel bound
is removed — kernel idle/live/cell durations and the bridge slow-peer numbers; the qualitative kernel policy and the
implemented transport, storage, and host-streaming safeguards stay.

## Decision

The kernel, bridge, and MCP detail from `m4plus_concept.md` is adopted and owned by the respective authoritative
packages:

- **Architecture 20 (kernel)**: the `KernelExecutionRequestDto` family, the
idle-disposal, kernel-capacity, and foreground-cell execution policy without fixed durations,
`kernel-state-snapshot-v1`, `KernelOutputChunkDto` closed
kinds, the 6 closed `kernel_*` safe failures, and the explicit negatives that `run.interrupt`
(`InterruptRunCommandDto`) remains the only first-scope run interruption command and that the daemon does not wait for a
cell to acknowledge an interrupt;
- **Architecture 19 (bridge)**: `BridgeRunGrantDto`,
`BridgeAttachmentResponseDto`, `BridgeInvocationCommandDto`, `BridgeInvocationAcceptedDto`, the 1-MiB frame /
512-KiB / 256-fact-512-KiB liveness bounds and the host's bounded slow-peer path, the 5 closed
`bridge_*`/`daemon_tool_gateway_required` safe failures, the explicit negative that the first bridge contract adds no
per-`ToolCallId` cancellation command, and the slow-peer non-delay property of that path;
- **Architecture 18 (MCP)**: the bounded `McpMethodDto` gateway, connection
scope and local-stdio process lifecycle, the 6 closed `mcp_*` safe failures, and the supersession of the concept2
`McpCapabilitySourceDto`/`McpDiscoveryDto`/`McpCapabilityRevisionDto` names by the authoritative
`McpCapabilitySourceV1`/`McpDiscoveryV1`/`McpCapabilityRevisionV1` records.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after its own activating specification, and
remains bound to Milestone 5+.

## Normative invariants

5. One live kernel epoch belongs to exactly one admitted `RunId`; retained
session-scoped idle/concurrency limits are historical provenance, and the first-scope kernel policy states no fixed
idle, count, or cell duration.
6. The bridge introduces no second start marker, result stream, or sequence; a
grant is non-secret transport evidence.
7. The MCP bounded gateway uses user-approved `McpMethodDto` records only; a
local stdio process is run-owned, lazy, and never reattached.

## Failure semantics

- Rejections are known typed pre-effect outcomes; no content is
truncated or partly committed.
- A started unproven effect yields a bounded `Partial` result with its notice and is never retried, reattached, or
rerun; the next model step proceeds.
- An unnegotiated client fails closed; historical M4 runs retain
byte-identical replay and denial.

## Rationale

These detail layers were present in `m4plus_concept.md` but absent from the authoritative packages, which covered the
semantics at principle level. Adopting the detail makes the authoritative documentation cover the features without
documenting any part of them as implemented.

## Compatibility and non-goals

This decision supersedes the absence of the detail in the principle-level text of architectures 20, 19, and 18. The
closed M4 baseline, M3/M4 bytes, and existing behavior remain unchanged, and no code changes are authorized by this
decision.

A sub-agent executor or recursion topology, process supervision, RLM/IPython executor topology, direct MCP
administration, and production activation remain outside this decision. M5-M9 are not renumbered.

Owner: architectures 18-20. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).
