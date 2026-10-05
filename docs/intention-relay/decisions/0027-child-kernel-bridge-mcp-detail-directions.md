# 0027: Post-M5 Child, Kernel, Bridge, and MCP Detail Directions

## Status

Accepted 2026-08-30. Not implemented; activation requires an activating specification.

## Decision

The child-agent (RLM), kernel, bridge, and MCP detail from `m4plus_concept.md` is adopted and owned by the respective
authoritative packages:

- **Architecture 17 (child/verifier)**: the child commands and messages
(`ParentSubAgentCommandDto`, `SubAgentHandleDto`, `RlmChildMessageOperation`, `MandateChildMessageDto`,
`MandateChildTerminalSummaryDto`, `RlmParentLinkDto`), the message queue, tree, class, delegation, and clarification
limits, child kernel seeding, the 17 closed `sub_agent_*`/`model_stream_progress_timeout` safe failures, and the
child-creation and delegation fields `child_provider_capability_selection`, `child_activity_graph_id`, and
`required_evidence_contract_references`;
- **Architecture 20 (kernel)**: the `KernelExecutionRequestDto` family, the
60-minute idle / 16 live kernel / 10-minute cell bounds, `kernel-state-snapshot-v1`, `KernelOutputChunkDto` closed
kinds, the 6 closed `kernel_*` safe failures, and the explicit negatives that `StopRunCommandDto` remains the only
first-scope run cancellation command and that the daemon does not wait for a cell to acknowledge an interrupt;
- **Architecture 19 (bridge)**: `BridgeRunGrantDto`,
`BridgeAttachmentResponseDto`, `BridgeInvocationCommandDto`, `BridgeInvocationAcceptedDto`, 16 unfinished operations,
the 1-MiB frame / 64-frame / 10-second / 512-KiB / 4-MiB / 256-fact-512-KiB bounds, the 6 closed
`bridge_*`/`daemon_tool_gateway_required` safe failures, the explicit negative that the first bridge contract adds no
per-`ToolCallId` cancellation command, and the slow-peer non-delay property of the bounded 64-frame/10-second path;
- **Architecture 18 (MCP)**: the bounded `McpMethodDto` gateway, connection
scope and local-stdio process lifecycle, the 6 closed `mcp_*` safe failures, and the supersession of the concept2
`MandateMcpCapabilitySourceDto`/ `DiscoveryDto`/`CapabilityRevisionDto` names by the authoritative
`MandateMcpCapabilitySourceV1`/`MandateMcpDiscoveryV1`/ `MandateMcpCapabilityRevisionV1` records.

The child-graph prose clauses (terminal or interrupted recipient rejects delivery, composition-root-only activation of
`sub_agent`, handle content exclusion, message delivery ordering to a terminal child, stale-handle and restart behavior,
the durable admission transaction, class narrowing and no-fallback, child lifetime, and the no-token-ceiling rule) are
owned by architecture 17.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after its own activating specification, and
remains bound to Milestone 5+. The retained RLM session-scoped identity and fixed limits remain historical provenance
where they conflict with Mandate child-graph semantics.

## Normative invariants

1. `sub_agent` creates a durable child Mandate; the child model adds no
`ToolId`, registry entry, or independent authority.
2. Message queues are bounded and redacted; equal replay returns the stored
message; changed reuse fails before publication.
3. Tree bounds and classes are RLM-tree policy and never become Mandate
admission quotas or child-graph limits.
4. A clarification request has a 60-minute deadline, a sublimit of the
360-minute child lifetime; a late reply fails closed.
5. One live kernel epoch belongs to exactly one admitted `RunId`; retained
session-scoped idle/concurrency limits are historical provenance, while the first-scope 60-minute/16/10-minute limits
are future policy.
6. The bridge introduces no second start marker, result stream, or sequence; a
grant is non-secret transport evidence.
7. The MCP bounded gateway uses user-approved `McpMethodDto` records only; a
local stdio process is run-owned, lazy, and never reattached.

## Failure semantics

- Limit failures are known typed pre-effect rejections; no content is
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

This decision supersedes the absence of the detail in the principle-level text of architectures 17, 20, 19, and 18. The
closed M4 baseline, M3/M4 bytes, and existing behavior remain unchanged, and no code changes are authorized by this
decision.

A sub-agent executor or recursion topology, process supervision, RLM/IPython executor topology, direct MCP
administration, and production activation remain outside this decision. M5-M9 are not renumbered.

Owner: architectures 17-20. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).
