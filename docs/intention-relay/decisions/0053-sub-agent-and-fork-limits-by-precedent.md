# ADR 0053: Sub-agent and fork limits by precedent

## Status

Accepted 2026-10-05. It removes every numeric cap from the future child, bridge, activity, and fork surfaces and from
the future kernel and model-progress designs: class step budgets, graph depth/child/descendant counts, child lifetime
and clarification deadlines, kernel idle/live/cell durations, bridge slow-peer numbers, and the model progress
deadline. No numeric cap survives on those surfaces; the implemented liveness safeguards stay as code constants in
their owning documents. It keeps the failure-code removals and the tool-output and tool-group boundary decisions. It
authorizes no new numeric bound, queue, reservation, refusal outcome, or error code.

## Scope and supersession

In scope: the child-graph message queue, clarification reserve, delegation-snapshot, child-result, and
child-concurrency bounds; the child class step budgets; the graph depth, direct-child, and descendant counts; the child
lifetime and clarification deadlines; the bridge unfinished-operation bound and slow-peer numbers; the activity message
bounds, reference-count bound, and research-value ledger; the fork tree depth, descendant-count, boundary-rate, and
base-snapshot bounds; the kernel idle, live-count, and foreground-cell bounds; the model progress deadline; the closed
failure members that guarded those bounds; and the tool-loop output-limit outcome and 16-call tool-group maximum.

Out of scope: pagination and representation bounds, the implemented transport, storage, tool-window, process,
provider-attempt, and host-streaming safeguards, and every behavior of the ordinary runtime.

| Record | Affected clause | Amended to |
| --- | --- | --- |
| [ADR 0009](0009-mandate-child-graph-and-delegated-verifier-authority.md) | The invariant list, which recorded no child-graph structure | The list opens with a rooted direct-edge tree with label-only classes and no numeric depth, child-count, descendant, lifetime, or clarification cap |
| [ADR 0026](0026-session-branching-detail-directions.md) | The decision's fixed-limits bullet, the closed-failure bullet, the rationale's "limits" wording, normative invariant 5, and the "limit failures" failure-semantics clause | The tree keeps only the 64-summary page and the 128-scalar title bounds, the closed failure list drops its four removed members, and lineage is ordinary-session structure that never constrains Mandate admission |
| [ADR 0027](0027-child-kernel-bridge-mcp-detail-directions.md) | The architecture-17 clause listing "the message queue, tree, class, delegation, and clarification limits" with its 17 closed safe failures, the architecture-19 clause listing "16 unfinished operations, the 1-MiB frame / 64-frame / 10-second / 512-KiB / 4-MiB / 256-fact-512-KiB bounds" with its 6 closed safe failures, and the architecture-20 clause listing the 60-minute idle / 16 live kernel / 10-minute cell bounds | The child structure is a shape contract with label-only classes and no numeric bounds, the bridge keeps only the transport, storage, and host-streaming safeguards, the kernel policy states no fixed durations, and the failure lists drop every cap member |
| [ADR 0029](0029-activity-and-notification-detail-directions.md) | The decision's fixed-activity-bounds bullet, the closed-failure bullet, the closing "numeric values are first-scope limits ... intrinsic/capacity bounds" clause, and the "limit failure" failure-semantics clause | No numeric activity bound is activated; delivery stays ordered by the container journal and `pair_order`, the closed failure list drops the queue, tree-count, and size members, and a future activity bound returns only with a recorded precedent |
| [ADR 0048](0048-limits-by-precedent-and-no-content-scanning.md) | The keep-list row for tool read and output windows | The row records the explicit cut: a window that ends content reports the cut through the result's truncation flag and marker and never refuses the result or presents it as complete |

The named clause of each record is amended in place; every other clause of those records stays as written.

## Decision

### No surviving numeric cap

No numeric cap on work, time, or graph size survives on the child, bridge, activity, or fork surfaces or in the kernel
and model-progress designs. Every remaining value is removed because none is implemented, so no value is restated as a
bound; the classes remain only as labels for typed child profiles, the graph keeps its rooted direct-edge shape, and the
lifetime, clarification, and progress mechanisms keep their shape without fixed durations. A new bound returns only
with a recorded precedent under [ADR 0048](0048-limits-by-precedent-and-no-content-scanning.md).

The child graph keeps the delegation protocol: `Instruction`, `Report`, `ClarificationRequest`, and `ClarificationReply`
ordered by the monotonically increasing `pair_order` of the delegation-pair container journal.

### Removed values and their failure members

The following values have no code (the child, bridge, activity, fork, kernel, and progress designs are approved future
design) and no recorded precedent. They are deleted, not renumbered, relocated, or replaced by a warning, soft cap,
counter, or periodic audit:

| Removed value | Where it was recorded | Why it is removed |
| --- | --- | --- |
| Per-direction message count and size (16 undelivered messages, 512 KiB content), the 1-slot/64-KiB clarification reserve, and the 15-slot/448-KiB ordinary allowance | architectures 17 and 24, ADRs 0027 and 0029 | No code; no recorded precedent; a bureaucratic message ceiling over an already-ordered, already-redacted pair journal |
| Delegation snapshot 512 KiB and 4 MiB per root tree | architecture 17, ADR 0027 | No code; no recorded precedent; rejects valid typed content (ADR 0048 removes speculative shape caps) |
| Child terminal result 512 KiB | architecture 17, ADR 0027 | No code; no recorded precedent; a refusal on valid terminal content |
| Concurrent non-terminal children in one tree (16) | architecture 17, ADR 0027 | No code; no recorded precedent; turns capacity into admission policy |
| Unfinished bridge operations per attached peer (16) | architecture 19, ADR 0027 | No code; no recorded precedent; a bridge counter over connection framing and architecture-15 admission |
| Activity research values (1,024 messages, 4 MiB aggregate, 4,096 journal records, 64 KiB record, 256/512-KiB page, 16 references) | architecture 24, ADR 0029 | Never activated; no recorded precedent; the ledger itself classified them as research values |
| Fork tree depth 4,096, descendants 16,384, source-boundary rate 16 per rolling hour, and base snapshot 1 MiB | architecture 23, ADR 0026 | No code; no recorded precedent; the rolling hour is a calendar/period quota of the kind ADR 0048 removes by name, and the snapshot cap rejects valid typed content |
| Class step budgets (`Light` 64, `Medium` 256, `Heavy` 1,024 model steps) | architecture 17, ADR 0027 | No code; no recorded precedent; the classes remain only as labels for typed child profiles |
| Graph depth 2, at most 16 direct children per node, and at most 64 descendants | architecture 17, ADRs 0009 and 0027 | No code; no recorded precedent; the graph keeps its rooted direct-edge shape without counts |
| 360-minute child lifetime and 60-minute clarification deadline | architecture 17, ADRs 0009 and 0027, architecture 24 | No code; no recorded precedent; the lifetime and clarification mechanisms stay with no fixed duration selected |
| Kernel idle retention (60 minutes), at most 16 live kernels, and the ten-minute foreground cell | architecture 20, ADR 0027 | No kernel code; no recorded precedent; the kernel policy keeps idle disposal, finite capacity, and no indefinite wait without fixed bounds |
| 64-frame/10-second bridge slow-peer path | architecture 19, ADR 0027 | No bridge code; the numbers are the live host's subscriber queue and write deadline and stay in [architecture 03](../architecture/03-daemon-transport-and-adapters.md) as implementation constants |
| 60-second model progress deadline (`model_stream_progress_timeout_v1`) | architecture 15, architecture 17 | No code; no progress deadline is implemented; the live enforced bound is the configured provider attempt timeout, and the policy keeps only the content-progress shape |

The matching failure members are deleted with their values:

```text
sub_agent_concurrency_limit_exceeded
sub_agent_delegation_too_large
sub_agent_depth_limit_exceeded
sub_agent_direct_child_limit_exceeded
sub_agent_follow_up_queue_full
sub_agent_result_too_large
sub_agent_tree_descendant_limit_exceeded
bridge_concurrency_limit_exceeded
agent_message_queue_full
agent_message_tree_limit_exceeded
agent_message_too_large
agent_activity_snapshot_too_large
fork_tree_depth_limit
fork_tree_descendant_limit
fork_boundary_rate_limit
fork_snapshot_too_large
```

`sub_agent_lifetime_exceeded`, `sub_agent_clarification_timeout`, and `model_stream_progress_timeout` stay: the
lifetime, clarification, and progress mechanisms remain, only their fixed durations are removed.

The implemented liveness safeguards are unchanged and stay as code constants in their owning documents, not as
product ceilings: transport message size, connect and synchronous IO timeouts, the stale-socket probe, storage read
bounds, tool read and output windows, the durable content bound, process timeout and drain windows, provider attempt
timeout and retries, and the host subscriber queue and write deadline ([ADR
0048](0048-limits-by-precedent-and-no-content-scanning.md) keep-list and
[`production-ceiling-removal.md`](../production-ceiling-removal.md)). Pagination and representation bounds stay as
each owning document records them: the fork tree page, the session title, and the history and notification pages.

### Tool output and tool-group boundaries

`tool_output_limit_exceeded` and `OutputLimitExceeded` are dead in code ([ADR
0025](0025-base-tool-contracts-and-tool-loop-bounds.md), [ADR
0048](0048-limits-by-precedent-and-no-content-scanning.md)). The tool output boundary is explicit truncation instead of
refusal: a tool renders its output within its own per-tool output window and reports a cut through the result's
truncation flag and explicit marker, and that marked result commits as the call's ordinary result. The window stays a
liveness safeguard in the ADR 0048 keep-list; no output-refusal outcome exists.

The same alignment removes the dead 16-call tool-group maximum and its `provider_tool_group_invalid` outcome from
[architecture 15](../architecture/15-tool-registry-and-mandate-tool-loop.md), which [ADR
0048](0048-limits-by-precedent-and-no-content-scanning.md) already superseded: a completed tool-calling step still owns
one non-empty ordered group of unique `ToolCallId` values, group validity is shape-only, and no numeric call or
group-size bound exists.

## Rationale

- The removed values were documentation-only product counters over surfaces with no code. Keeping them would ship
  queues, reservations, rates, budgets, and size refusals that no failure history justifies and no implementation owes.
- No numeric cap remains: the delegation model is a shape (a rooted direct-edge tree with label-only classes), and its
  lifetime, clarification, and progress mechanisms are contracts whose durations a later activating specification
  selects, not numbers this documentation fixes for them.
- A bound that no code enforces and no observed failure motivates is deleted rather than justified; the implemented
  liveness safeguards stay because they answer concrete transport, storage, process, and provider failures.
- The tool boundary already truncates with an explicit marker in code; the documentation now matches that behavior
  instead of asserting a refusal outcome no code produces.

## Invariants

1. No numeric cap exists on the child, bridge, activity, or fork surfaces or in the kernel and model-progress designs;
   a new bound returns only with a precedent recorded in an ADR or architecture section.
2. The child graph is shape only (rooted direct edges, label-only classes), and the lifetime, clarification, and
   progress mechanisms fix no durations; no kept value becomes a Mandate admission quota, a scheduler gate, or a bridge
   counter.
3. Removed values and their failure members have no producer, consumer, alias, or compatibility path.
4. The delegation-pair container journal is the only message order; messages are never merged, overwritten, or silently
   dropped, and no queue count, size, or clarification reservation exists.
5. A tool that reaches its output window reports the cut explicitly; no output-refusal outcome exists.
6. Pagination and representation bounds and the implemented liveness safeguards are unchanged.
7. No new numeric value, error code, DTO family, storage mechanism, or wire version is introduced.

## Compatibility

This is a documentation-only change. The child, bridge, activity, fork, kernel, and progress designs remain approved
future design and are not implemented; removing their numeric caps changes no code and authorizes no implementation.
Recorded M3/M4 bytes, meanings, replay, and recovery are untouched. No protocol, DTO, configuration, or storage schema
version changes.

## Security and failure behavior

- Removed caps protected nothing structural: messages stay typed, redacted, ordered by the delegation-pair journal, and
  rejected only for direction, operation identity, order, or a terminal recipient.
- A tool output window still bounds one read or rendered result so a large file or match set cannot force unbounded
  allocation; the cut is explicit and never presented as complete.
- No implemented safeguard is loosened: the transport, storage, tool-window, process, provider-attempt, and
  host-streaming constants keep their code values and reasons.
- No runtime content scanning is introduced ([ADR 0048](0048-limits-by-precedent-and-no-content-scanning.md)).

## Non-goals

No new numeric bound, queue, reservation, counter, rate window, or refusal outcome; no change to the implemented
safeguards' constants or to the label-only class set; no implementation of the child, bridge, activity, or fork
surfaces; no change to M3/M4 recorded behavior; no migration, version bump, or compatibility layer; no edit to closed
milestone records.

## Affected documents

[Decisions index](README.md) lists this record. [ADR
0009](0009-mandate-child-graph-and-delegated-verifier-authority.md), [ADR
0026](0026-session-branching-detail-directions.md), [ADR
0027](0027-child-kernel-bridge-mcp-detail-directions.md), [ADR
0029](0029-activity-and-notification-detail-directions.md), and [ADR
0048](0048-limits-by-precedent-and-no-content-scanning.md) carry the amended clauses. [Architecture
03](../architecture/03-daemon-transport-and-adapters.md) owns the implemented host subscriber queue and write deadline
constants; [architecture 15](../architecture/15-tool-registry-and-mandate-tool-loop.md) owns the tool output boundary,
the tool-group shape contract, and the model progress policy whose duration a later activating specification selects;
[architecture 17](../architecture/17-mandate-child-graph-and-delegated-verifier-authority.md) owns the child graph
shape, the label-only classes, and the closed failure members; [architecture
19](../architecture/19-mandate-gateway-rlm-bridge.md) owns the bridge bounds, which keep only the transport, storage,
and host-streaming safeguards; [architecture 20](../architecture/20-ipython-kernel-lifecycle.md) owns the kernel policy
without fixed durations; [architecture 23](../architecture/23-non-destructive-session-branching-and-regeneration.md)
owns fork lineage; [architecture 24](../architecture/24-activity-ui-and-adapters.md) owns activity messaging; and
[`production-ceiling-removal.md`](../production-ceiling-removal.md) mirrors the policy and the explicit truncation
marker. Secondary cleanup: [architecture
20](../architecture/20-ipython-kernel-lifecycle.md) also drops the stale 4-MiB group reference that ADR 0048 already
superseded.

## Evidence

The change is accepted only together with: a documentation search receipt showing the removed numbers have no
remaining hit in `docs/intention-relay/` outside the removal statements recorded here, the implemented safeguards in
their owning documents, and the provider-reasoning and Goal-domain `4 MiB` bounds outside this record's scope; no
numeric cap remains in the child, bridge, activity, fork, kernel, or model-progress clauses; and valid links and Mermaid
with the documentation and architecture checks passing. Gates: `make quick`, `make verify`, `docs-check`, `make
architecture`, Linux/Windows CI.
