# ADR 0027: Post-M5 Child, Kernel, Bridge, and MCP Detail Directions

## Status

Accepted 2026-08-30. This decision records the child-agent (RLM), kernel,
bridge, and MCP detail layers as accepted future directions. It does not
activate implementation: the layers are bound to
[Milestone 5+](../architecture/11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment)
and are implemented only through a separately accepted activating specification
at the start of that milestone.

## Decision

The following detail from
[`m4plus_concept.md`](../m4plus_concept.md) is adopted and owned by the
respective authoritative packages:

- **Architecture 17 (child/verifier)**: `ParentSubAgentCommandDto`,
  `SubAgentHandleDto`, `RlmChildMessageOperation`, `MandateChildMessageDto`,
  `MandateChildTerminalSummaryDto`, `RlmParentLinkDto` identity, message queue
  limits (16 messages / 512 KiB per direction, 1 slot / 64 KiB clarification
  reserve), tree bounds (16/3/0/2/64/16/360 minutes),
  `SubAgentClassDto` Light/Medium/Heavy (64/256/1,024), delegation snapshot
  bounds (512 KiB each, 4 MiB per tree), the 60-minute clarification sublimit,
  child kernel seeding, the 17 closed `sub_agent_*`/
  `model_stream_progress_timeout` safe failures, the child-creation and
  delegation fields `child_provider_capability_selection`,
  `child_activity_graph_id`, and `required_evidence_contract_references`, and
  the child-graph prose clauses (terminal or interrupted recipient rejects
  delivery; composition-root-only activation of `sub_agent`; handle content
  exclusion; a message committed before a child's terminal decision is included
  only in its next fresh model request while one to a terminal child is
  rejected; a stale handle cannot revive a terminal child and a child cannot
  autonomously request another parent authority context; the durable admission
  transaction validates the parent authority context, applicable policy,
  selected descriptor, selected class, tree counters, and delegation snapshot;
  no class bypasses the one-daemon authority, WorkspaceRoot, Plan/Build mode,
  hooks, confirmation, redaction, or a stricter current admission decision and
  no class falls back to a current default when unavailable; a child never
  outlives its parent; after restart neither a reply nor a follow-up can resume
  the interrupted child and the selected automatic continuation is permitted
  only while the same daemon process still owns the same active child run and
  the matching reply arrives before its deadline; the first scope requires no
  token ceiling from providers that cannot report usage);
- **Architecture 20 (kernel)**: the `KernelExecutionRequestDto` family,
  60-minute idle / 16 live kernels / 10-minute cell bounds,
  `kernel-state-snapshot-v1`, `KernelOutputChunkDto` closed kinds, the 6
  closed `kernel_*` safe failures, the explicit negatives that
  `StopRunCommandDto` remains the only first-scope run cancellation command and
  that the daemon does not wait for the cell to acknowledge an interrupt, idle
  disposal discarding the in-memory namespace after recording only the safe
  checkpoint metadata that already exists without canceling or altering an
  unrelated durable run, the terminal result never being reconstructed from a
  formatted footer, kernel diagnostics containing only safe status, bounded
  sizes, failure codes, and correlation references, and background in-memory
  results being captured only by a later successful foreground cell;
- **Architecture 19 (bridge)**: `BridgeRunGrantDto`,
  `BridgeAttachmentResponseDto`, `BridgeInvocationCommandDto`,
  `BridgeInvocationAcceptedDto`, 16 unfinished operations, the 1-MiB frame /
  64-frame / 10-second / 512-KiB / 4-MiB / 256-fact-512-KiB bounds, the 6
  closed `bridge_*`/`daemon_tool_gateway_required` safe failures, the
  explicit negative that the first bridge contract adds no per-`ToolCallId`
  cancellation command, and the slow-peer non-delay property (the bounded
  64-frame/10-second path never delays durable execution or healthy
  subscribers);
- **Architecture 18 (MCP)**: the bounded `McpMethodDto` gateway, connection
  scope and local-stdio process lifecycle, the 6 closed `mcp_*` safe failures,
  and the supersession of the concept2 `MandateMcpCapabilitySourceDto`/
  `DiscoveryDto`/`CapabilityRevisionDto` names by the authoritative
  `MandateMcpCapabilitySourceV1`/`MandateMcpDiscoveryV1`/
  `MandateMcpCapabilityRevisionV1` records.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after
a later activating specification, and is bound to Milestone 5+ in the roadmap.
The retained RLM session-scoped identity and fixed limits remain historical
provenance where they conflict with Mandate child-graph semantics.

## Rationale

The authoritative package review of 2026-08-30 confirmed these detail layers are
present in `m4plus_concept.md` but absent from the authoritative packages,
which cover the semantics at principle level. This decision adopts the detail so
the authoritative documentation fully covers the features, while preserving the
project rule that no feature is documented as implemented without code evidence.

## Normative invariants

1. `sub_agent` creates a durable child Mandate; the child model adds no ToolId,
   registry entry, or independent authority.
2. Message queues are bounded and redacted; equal replay returns the stored
   message; changed reuse fails before publication.
3. Tree bounds and classes are RLM-tree policy and never become Mandate
   admission quotas or child-graph limits.
4. A clarification request has a 60-minute deadline, a sublimit of the
   360-minute child lifetime; a late reply fails closed.
5. One live kernel epoch belongs to exactly one admitted `RunId`; retained
   session-scoped idle/concurrency limits are historical provenance, while the
   first-scope 60-minute/16/10-minute limits are future policy.
6. The bridge introduces no second start marker, result stream, or sequence; a
   grant is non-secret transport evidence.
7. The MCP bounded gateway uses user-approved `McpMethodDto` records only; a
   local stdio process is run-owned, lazy, and never reattached.

## Failure semantics

- Limit failures are known typed pre-effect rejections; no content is truncated
  or partly committed.
- A started unproven effect is `ExternalEffectUnknown` and never retried,
  reattached, or rerun.
- An unnegotiated client fails closed; historical M4 runs retain byte-identical
  replay and denial.

## Compatibility and supersession

This decision supersedes the absence of the detail in the principle-level text
of architectures 17, 20, 19, and 18. The closed M4 baseline, M3/M4 bytes, and
existing behavior remain unchanged. Activation remains deferred: no code changes
are authorized by this decision.

## Security and residual risk

The layers remain trusted-local. Messages, summaries, checkpoints, grants, and
failures are bounded and credential-free; redaction stays central and every
activating specification must pass the fake-secret regression suite.

## Affected documents

- [`architecture/11-implementation-roadmap.md`](../architecture/11-implementation-roadmap.md)
- [`architecture/17-mandate-child-graph-and-delegated-verifier-authority.md`](../architecture/17-mandate-child-graph-and-delegated-verifier-authority.md)
- [`architecture/18-mandate-mcp-capability-lifecycle.md`](../architecture/18-mandate-mcp-capability-lifecycle.md)
- [`architecture/19-mandate-gateway-rlm-bridge.md`](../architecture/19-mandate-gateway-rlm-bridge.md)
- [`architecture/20-ipython-kernel-lifecycle.md`](../architecture/20-ipython-kernel-lifecycle.md)
- [`reconciliation/README.md`](../reconciliation/README.md)
- [`reconciliation/source-of-truth-matrix.md`](../reconciliation/source-of-truth-matrix.md)
- [`reconciliation/contradiction-register.md`](../reconciliation/contradiction-register.md)
- [`reconciliation/concept-supersession-index.md`](../reconciliation/concept-supersession-index.md)
- [`reconciliation/deferred-excluded-register.md`](../reconciliation/deferred-excluded-register.md)
- [`reconciliation/evidence-register.md`](../reconciliation/evidence-register.md)
- [`reconciliation/compatibility-register.md`](../reconciliation/compatibility-register.md)
- [`decisions/README.md`](README.md)

## Required evidence

No implementation evidence is claimed. The activating specification must declare
exact crates, DTO/wire/storage versions, feature profiles, coverage tiers,
fixtures, and outcome evidence, and pass `make quick`, `make verify`, and
Linux/Windows CI before acceptance.

## Non-goals

This decision does not implement the layers; it does not change M3/M4 behavior;
it does not renumber M5--M9; it does not activate a crate, schema, migration,
protocol, or feature. A sub-agent executor/recursion topology, process
supervision, RLM/IPython executor topology, direct MCP administration, and
production activation remain outside this decision.
