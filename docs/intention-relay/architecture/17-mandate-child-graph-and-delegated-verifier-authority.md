# Mandate Child Graph and Delegated Verifier Authority

**Approved future design. Not implemented; activation requires an activating specification.**

Owner: architecture 17. Decisions: ADR 0009, ADR 0027, ADR 0053. Research: m4plus_concept.md.

This document owns future Mandate child-graph relations, immutable delegation, direct-parent controls, graph
terminalization, and separately issued delegated verifier authority. It applies only to future `Mandate` and
`VerifierMandate` execution. M3/M4 Sessions, Runs, provider selection, tool-call denial, replay,
recovery, bytes, IDs, UUIDs, cursors, events, and snapshots retain their recorded ordinary semantics, and retained RLM
child/activity material remains research and historical provenance, not future Mandate graph authority.

## Ownership and non-authorities

Architecture 13 owns Mandate lifecycle, reason validity/order, fresh admission, and user-conflict precedence.
Architecture 14 is the historical record of the removed execution-meaning envelope and decoders (ADR 0046); records are
typed serde JSON. Architecture 15 owns the fixed `sub_agent` registry slot, frozen selection, direct tool
admission, and generic tool-loop evidence. Architecture 16 owns durable reevaluation, readiness, candidate selection,
and admission handoff.

This document owns child/verifier payload semantics and their durable relations. It is not a second lifecycle,
scheduler, registry, provider/tool selector, session-fork model, activity/UI system, or general notification system;
parenthood, ancestry, activity, a prompt, Goal, Skill, tool, model, provider, MCP source, bridge/kernel, evidence, or
verdict never grants lifecycle, scheduling, tool, or target-mutation authority.

## Child graph identity and immutable delegation

A `sub_agent` invocation creates a new durable **child Mandate**, not a child run, an ordinary pending turn, a session
branch, process, provider continuation, or retained RLM task. A child has exactly one immutable `ParentMandateId` and one
immutable root-graph identity; its authoritative relation is an append-only direct edge, and graph summaries and
recursive indexes are rebuildable projections, not authority.

```text
MandateChildEdgeV1
  edge_id
  root_mandate_id
  parent_mandate_id
  parent_revision
  creating_run_id
  creating_tool_call_id
  child_mandate_id
  child_initial_revision
  delegation_snapshot_reference

MandateChildDelegationSnapshotV1
  delegation_id
  parent_mandate_id
  parent_revision
  creating_run_id
  child_objective_and_scope
  child_mode
  frozen_context_references
  selected_goal_skill_evidence_references
  required_evidence_contract_references
  continuation_configuration
  capability_selection_rule
  provider_capability_selection
  activity_graph_id
  typed_provenance_references
```

`required_evidence_contract_references` freezes the explicit evidence contracts the child must satisfy;
`provider_capability_selection` and `activity_graph_id` freeze the child's provider-capability selection rule and its
activity-graph identity. They are immutable credential-free references, never live provider material or activity
authority.

The snapshot is an immutable credential-free typed JSON record. It may freeze only explicitly selected child objective,
scope, mode, safe context, Goal, Skill, evidence, continuation, capability, and provenance references; it excludes
credentials, endpoints, SDK values, handles, raw transcript/output, mutable parent state, tool permissions,
provider/MCP/process/kernel/bridge connections, policy/quota/confirmation inheritance, and unfinished external effects.
Every child fresh run resolves its own immutable meaning from its child revision and delegation snapshot; parent
revision, ancestry, configuration, registry, provider, Goal, Skill, activity, UI, or live resources cannot repair
missing child meaning. Typed serde JSON owns the encoding (ADR 0046); this document owns the child-link nested selection
that binds exact edge and snapshot references.

## Atomic child creation and graph integrity

One idempotent transaction commits all or nothing:

- child Mandate identity and initial immutable revision;
- direct edge, root-graph identity, and delegation snapshot;
- graph projection and later activity reference;
- parent `sub_agent` terminal creation result;
- all affected events, snapshots, container journal sequences, and idempotency evidence.

Equal creation identity and equal typed content return the original child, edge, snapshot, and result; changed reuse
fails before another child, edge, run, activity record, or external action exists. No provider, tool, process, network,
kernel, MCP, bridge, child runtime, or scheduler effect occurs inside the transaction, and publication occurs only after
commit and a scoped durable reread.

Creation validates authoritative edge ancestry under the same storage linearization as insertion. It rejects self-link,
cycle, second parent, reparent, detach, merge, root conversion, cross-root/project/workspace relation, missing or stale
parent revision, wrong creating run/tool call, incompatible duplicate creation, and malformed identity before durable
mutation. A committed graph is a rooted directed tree: each non-root has one parent, shares one root, and is reachable
from that root.

## Direct-edge controls and messages

Parenthood grants only this closed direct-child control family:

```text
MandateChildControlV1
  GetStatus
  AwaitTerminalSummary
  SendInstruction
  ReplyToClarification
  PauseChild
  StopChild
```

A child may send only `Report` and `ClarificationRequest` to its direct parent, and only the immutable direct edge may
carry these controls/messages. Siblings, indirect ancestors/descendants, roots, unrelated Mandates, adapters, providers,
MCP, bridge/kernel, and models outside the admitted parent loop have no graph-control authority.

Messages are typed, redacted, idempotent, and ordered by one delegation-edge container journal, the container journal of
the container "delegation pair", whose shared `pair_order` positions (`AgentPairOrderDto`) cover both directions. That
journal is local to its pair and is never reused for global event order.
`GetStatus` and `AwaitTerminalSummary` are observation only. Instructions, replies, reports, and clarification requests
are durable evidence, not authority: they create no `RunId`, consume no reason, revise no Mandate, mutate no
already-sent provider request, and directly schedule no work. A later scheduler reread may evaluate any separately valid
reason, and architecture 13 alone may admit a fresh run. A terminal or interrupted recipient rejects delivery: an
undelivered ordinary message to a terminal or interrupted child is rejected with a typed durable delivery outcome, while
the original message and its reason remain in the activity-tree container journal.

Parenthood cannot complete, needs-rework, revise, archive, issue verifier authority, or control a
sibling/root/unrelated Mandate; a user can mutate every child under architecture 13. Parent controls validate the exact
edge, frozen delegated control, the expected Mandate container journal sequence, expected lifecycle, and idempotent
operation identity.

## Terminalization and recovery

A child terminal summary is safe provenance/evidence, not completion or failure of the parent objective. Parent
completion requires durable terminal evidence for every applicable descendant; a parent may initiate pause/stop only
against a direct child, and a daemon may execute a transitive durable **safety cascade** over immutable descendant edges
without granting indirect authority to the parent.

Cascade intent, selected subtree/graph epoch, each descendant transition, and completion are distinct durable idempotent
facts. A creation racing an active terminalization intent either commits first and is included in the selected subtree,
or is rejected after the closure intent commits; completion revalidates the same graph epoch and cannot terminalize from
a stale descendant set.

A partial result belongs to the exact child that started the interrupted effect. It pauses no child and never becomes
parent state: the child's next model step proceeds with the bounded captured output and its notice.

Recovery completes before graph scheduling/readiness. It rebuilds projections only from supported durable edge facts,
retains edges, snapshots, messages, summaries, checkpoints, authority references, and immutable meaning, classifies
unfinished work independently, and never resumes, retries, reattaches, rediscovers, or reruns
child/provider/tool/process/kernel/MCP/bridge/scheduler work. A later admission is fresh and has a new `RunId`.

## Separately issued delegated verifier authority

A verifier is a normal Mandate executing as `VerifierMandate`. It can mutate a target only through a separately
user-issued, revisioned, active, target-scoped authority; neither parenthood, child work, Goal, Skill, activity, prompt,
evidence, verdict, tool, provider, nor current configuration supplies it.

```mermaid
flowchart TD
  U[User] --> A[Issued authority]
  A --> V[Verifier Mandate]
  V --> B[Frozen audit baseline]
  B --> E[Evidence and verdict]
  E --> M[Atomic target mutation]
```

```text
VerifierAuthorityV1
  authority_id
  authority_revision
  verifier_mandate_id
  immutable_target_set_reference
  allowed_operations
  audit_contract_reference
  issuance_expiry_revocation_consumption

VerifierAuditBaselineV1
  authority_reference
  verifier_mandate_revision
  target_mandate_id
  target_revision_and_sequence
  target_lifecycle
  frozen_goal_gate_evidence_references
  optional_partial_effect_reference
  audit_contract_reference
```

Authority revisions, target sets, audit baselines, evidence, verdicts, and mutations are immutable. A target set is
explicit and never expands through parent/child, ancestry, descendants, siblings, Goals, sessions, branches, activity,
or shared evidence; a verifier cannot target itself. Its children may gather evidence but
cannot inherit, relay, consume, amplify, or exercise target-mutation authority.

Typed serde JSON owns the `VerifierMandate` record shape (ADR 0046); this document owns verifier nested selection field
semantics. A verifier payload with missing, corrupt, stale, or unsupported mandatory selection cannot downgrade to
Mandate or Ordinary execution.

### Stale baseline tuple

A verifier baseline is fresh only when all required values match the committed state: exact target identity, target
revision, Mandate container journal sequence, lifecycle, authority revision, audit contract, graph epoch where
applicable, and operation idempotency identity. Any mismatch is a typed pre-mutation stale failure; the system never
best-effort merges, retargets, substitutes current state, or retries with changed meaning.

## Audit, mutations, and conflicts

A verdict is durable evidence only: it neither schedules work nor mutates a target. Before dependent verifier work or
mutation, validate exact verifier identity/revision, authority revision/lifecycle, target membership, allowed operation,
audit contract, frozen target baseline, and operation-specific lifecycle prerequisites; missing, revoked, expired,
consumed, corrupt, mismatched, stale, or unsupported authority/baseline fails closed before mutation, and no path
substitutes current authority, target revision, Goal, configuration, registry, ancestry, readiness, evidence store, or
UI state.

Primary delegated operations are `MarkNeedsRework`, `MarkComplete`, `Stop`, and `ReviseFull`; `Pause` and `Resume` are
not implicit verifier powers.

-  `MarkNeedsRework` requires explicit authority and qualifying fail evidence. It creates neither a trigger nor a
resumed run.
-  `MarkComplete` requires explicit authority, unconditional pass evidence, and required graph terminalization closure.
- `Stop` never asserts completion.
-  `ReviseFull` requires its own authority and creates only an immutable future revision. It never rewrites an admitted
run or historical evidence.

One target-mutation transaction validates the authority, baseline, evidence, verdict, target revision, container journal
sequence, lifecycle, graph closure where required, and idempotency identity. It commits all or nothing: applied/rejected
result, target projection/events/snapshots when changed, authority consumption where selected, audit linkage, the target
container journal sequence, safe activity/notification reference, and idempotency evidence.

User lifecycle, revision, revocation, and authority-revision mutations win optimistic conflicts. A
losing parent, daemon, or verifier action performs a scoped reread and cannot merge, retarget, select another operation,
or retry with changed meaning.

## Verifier interruption and protocol boundary

If verifier external work is interrupted, the verifier execution yields a bounded `Partial` result with its notice,
never mutates the target, and pauses nothing; the partial result is not a qualifying verdict and never becomes target
state. Recovery preserves all authority/audit/mutation history but never replays verifier evidence work or
reapplies a committed mutation.

Future child/verifier projections use typed JSON-RPC 2.0 methods with authoritative snapshot/replay or typed
resync/error and never deliver partial data (ADR 0045). Replay is read-only: it cannot resend messages, start children,
repeat a cascade, consume authority, collect evidence, or reapply a target mutation. Exact method shapes, pages,
retention, SQL, migrations, and UI remain deferred.

## Delegated child-agent detail (`sub_agent`)

The following first-scope detail applies to future `sub_agent` child work under the Mandate child graph.

**Identity and commands.** `sub_agent` is the canonical child-agent identifier in the required fourteen-slot registry;
one child-agent boundary exists (no alias, second `ToolId`, private Python function, or direct primitive path). The
child-agent owner activates `sub_agent` only through the composition root. The direct effect profile flags are
`child_agent_start` and `child_agent_control` (descriptive only). Each admitted child is a daemon-owned independent
`SessionId` with its own `RunId`, pending turns, immutable run selection, model steps, bridge grants, durable facts,
and at most one active run. The daemon assigns `SubAgentId`, child session identity, and child run identity only after
successful admission; `ToolCallId` remains the canonical identity of the parent `sub_agent` call, and a child never
reuses the parent's `RunId`, `ModelStepId`, grant, or operation identity. Every new root run receives one
daemon-assigned
`AgentActivityTreeId`; every admitted child retains it with its direct-parent link (distinct from `ConversationTreeId`
fork lineage). `RlmParentLinkDto` is a durable immutable runtime relationship containing parent `SessionId`, `RunId`,
`TurnId`, `ModelStepId`, `ToolCallId`, plus child `SubAgentId`, `SessionId`, and initial `RunId`.

The direct parent closed command family is:

```text
ParentSubAgentCommandDto
  Create
  GetStatus
  AwaitResult
  Cancel
  EnqueueFollowUp

SubAgentHandleDto
  sub_agent_id
  child_session_id
  child_run_id
  selected_class
  status
```

Every command is a normal typed tool invocation with its own `ToolCallId`, admission evidence, terminal result, durable
commit, and post-reread publication. `Create` returns `SubAgentHandleDto` immediately after atomic durable admission and
never waits for child completion. The child's narrower daemon-internal RLM operation is:

```text
RlmChildMessageOperation
  Report
  ClarificationRequest
```

bound to the child's immutable `RlmParentLinkDto`, current `ModelStepId`, and daemon-assigned `RlmMessageId`; it is not
a `ToolId`/`ToolCallId`/registry invocation/bridge operation/MCP command/independent authority. `GetStatus` returns a
bounded safe direct-child status plus a bounded descendant summary. `AwaitResult` returns one child terminal outcome,
safe conclusion, and immutable provenance reference; a pending clarification returns a distinct `ClarificationPending {
clarification_request_id, deadline }` that ends only that operation, not the child run. `Cancel` cascades only through
that child's descendant subtree. `EnqueueFollowUp` (direct parent of an active child only) creates an `Instruction` or
`ClarificationReply { clarification_request_id, content }`.

**Messages and summaries.** DTOs:

```text
MandateChildMessageDto
  message_id
  graph_id
  parent_link_reference
  pair_order
  direction = ParentToChild | ChildToParent
  kind = Instruction | Report | ClarificationRequest | ClarificationReply
  sender_run_reference
  recipient_mandate_reference
  recipient_run_reference_when_live
  safe_text
  typed_references
  delivery_state

MandateChildTerminalSummaryDto
  child_mandate_id
  child_revision
  terminal_run_reference
  terminal_kind
  disposition
  verified_checkpoint_reference_when_present
  partial_effect_reference_when_present
  evidence_references
  safe_conclusion
```

Each delegation pair owns one durable container journal across both directions: a dense `pair_order` sequence
(`AgentPairOrderDto`) that is that pair's only gap-detection token; a duplicate or skipped `pair_order` rejects before
publication. Messages are redacted typed records (identity, revision/cursor, safe visibility, provenance references):
equal replay returns the stored message, and changed reuse fails before publication. A terminal child run records one
redacted summary; the parent receives the summary reference once in the next eligible model exchange, never a raw
transcript or output. Summaries aggregate state and provenance; they never make child success complete a parent, child
failure fail a parent, or child evidence satisfy a parent acceptance contract. Usage aggregation dedupes original
`RunId` values. The first scope does not require a token ceiling
from providers that cannot report usage: when a provider reports no usage component, the tree aggregates only the
components it reports, and no synthetic ceiling, price, or inferred cost is introduced. A child
`Paused`/`NeedsRework`/`Stopped` does not implicitly change the parent; a child partial result pauses no parent or
child work: the child's next model step proceeds with the bounded captured output and its notice, and the interruption
is recorded as an urgent graph safety observation.

**Message delivery.** A message is never merged, overwritten, or silently dropped, and there is no per-direction queue
count, size cap, or clarification reservation. A message committed before a child's terminal decision is included only
in its next fresh model request, and one to a terminal child is rejected. The parent cannot use a stale handle to revive
a terminal child, and the child cannot autonomously request another parent authority context. The handle contains no
credential, path, grant, kernel value, transcript, implementation resource, or raw child result.

**Tree bounds and classes.** Code-owned child-graph structure per one root user request (root run at depth zero):

| Bound | Selected value |
| --- | --- |
| Direct children of one node | 16 |
| Maximum child depth | 2 |
| Total descendants in one RLM tree | 64 |
| Full lifetime of one child | 360 minutes from durable admission |

The unconstrained per-node product across the two child depths would be `16 x 16 = 256` slots, and the 64-descendant
cap binds first. A direct child beyond either structure bound is not queued; it receives a known pre-effect terminal
result. Child lifetime
includes delay before work begins and never pauses for tools, confirmation,
`ask_user`, kernel work, or descendants. The durable admission transaction validates the parent authority context,
applicable policy, selected descriptor, selected class, tree counters, and delegation snapshot before it assigns child
identities; it atomically records the child session and run, `RlmParentLinkDto`, immutable delegation snapshot, class
resolution, idempotent operation binding, and audit evidence, or records none of them. `SubAgentClassDto` is closed as
`Light`, `Medium`, `Heavy` with fixed maximum model-step counts of 64, 256, and 1,024 respectively; each class is a step
budget only and resolves with a complete typed profile (one permitted provider profile, lifetime up to 360 minutes,
permitted registered tool subset, max depth up to 2, kernel rules). A nominally stronger class is valid only when every
effective tool/input constraint/scope/lifetime limit remains narrowed. No class bypasses the one-daemon authority,
WorkspaceRoot, Plan/Build mode, hooks, confirmation, redaction, or a stricter current admission decision. The daemon
resolves and persists one immutable child selection and never accepts a raw model name, endpoint, or credential, nor
falls back to a current default when a class is unavailable. Every child receives one immutable
`SubAgentDelegationSnapshotDto` (task, bounded safe textual projection, typed provenance references, parent provenance,
selected effective programmatic-caller-policy snapshot reference, selected `AgentActivitySelectionV1` reference;
excludes raw provider items, reasoning text, tool output, Python objects, live state, grants, credentials, paths, and
implementation resources).

**Child kernel state.** A child session may lazily create its own IPython kernel under the selected session-kernel
lifecycle and the shared limit of sixteen live kernels; it never shares a process or live namespace with the parent. At
first child-kernel creation, the daemon may form an independent full copy of the latest verified parent
`kernel-state-snapshot-v1` (supported serializable values only; excludes grants, tasks, handles, provider resources,
credentials; no reverse sync). No parent checkpoint failure, failed transfer, or unavailability blocks child admission
(the child starts empty with a safe transfer/restoration status), and the daemon never reruns a parent cell to produce a
child copy.

**Clarification and progress.** A child may enter `AwaitingClarification` only after its durable `ClarificationRequest`
reaches the direct parent. The request has a fixed 60-minute deadline from durable acceptance, a sublimit of, and never
pausing or extending, the child's 360-minute full lifetime. The direct parent may accept exactly one matching
`ClarificationReply` before that deadline; the daemon then records delivery and creates the next fresh model step of
that same active child run with the reply in the separate RLM message exchange. It does not create a new child, a new
`RunId`, a new authority, or a new external action. A reply after the deadline, parent terminalization, cancellation,
policy-driven cascade, or another reply fails closed. A child never outlives its parent: parent terminalization is not
complete until the child subtree has durable terminal outcomes. On clarification deadline expiry, the child records the
known terminal `sub_agent_clarification_timeout` outcome and begins no further model step. Cancellation, parent
terminalization, child lifetime expiry, policy revocation, and daemon restart atomically invalidate every pending
clarification, and a late reply is rejected. After restart neither a reply nor a follow-up can resume the interrupted
child; the selected automatic continuation is permitted only while the same daemon process still owns the same active
child run and the matching reply arrives before its deadline. On child lifetime expiry, progress-timeout failure, child
cancellation, kernel failure, executor loss, or daemon restart, no provider request, tool, process, kernel, bridge
operation, or external action is resumed, reattached, retried, or rerun; an unfinished child run becomes `Interrupted`
on daemon recovery, and a later retry creates a newly admitted child consuming new tree capacity. `AwaitResult` returns
a closed terminal state, a safe conclusion, and an immutable typed terminal-child-result reference.
`model_stream_progress_timeout_v1` applies to every child step under
[architecture 15](15-tool-registry-and-mandate-tool-loop.md#model-progress-deadline).

**Closed safe failures.** At minimum the child model adds these `ErrorDto` codes:

```text
sub_agent_depth_limit_exceeded
sub_agent_direct_child_limit_exceeded
sub_agent_tree_descendant_limit_exceeded
sub_agent_lifetime_exceeded
sub_agent_class_unavailable
sub_agent_delegation_unavailable
sub_agent_not_active
sub_agent_message_operation_conflict
sub_agent_message_direction_forbidden
sub_agent_clarification_not_pending
sub_agent_clarification_reply_conflict
sub_agent_clarification_timeout
model_stream_progress_timeout
```

They disclose no credential, path, delegation content, Python value, grant, provider resource, process topology, or raw
transcript. The structure above is RLM-tree policy and never becomes a Mandate admission quota or a scheduler gate.

## Compatibility, dependencies, and non-goals

This document depends on architectures 13-16 and decisions 0001, 0004, 0006, 0007, 0008, and 0052. Architecture 18 owns
MCP capability lifecycle; MCP evidence is non-authorizing and an interrupted MCP call's partial result stays local to
its owning Mandate. This document defines no sub-agent executor, worker or recursion topology, product concurrency,
queue, size, or rate quotas, RLM/IPython, MCP capability lifecycle semantics, Skills/Goals/context semantics, provider
evolution, session forks, general activity/notifications/UI, schema, migrations, crates, Cargo, Makefile/CI, or
production implementation.

M3/M4 and retained RLM records receive no synthetic Mandate child edge, delegation snapshot, activity, verifier
authority, target set, audit, verdict, mutation, reconciliation, or execution-kind state; historical M4 tool calls
remain denial evidence, pending turns never become child Mandates or Mandate reasons, and current ancestry or
historical RLM identity cannot reconstruct future child/verifier meaning. Architectures 19-24 own bridge projections,
kernel seeding, Goal/Skill/context selection, provider selections, session forks, and activity projections; none may
create an edge, widen parenthood, relay verifier authority, turn bridge-held evidence into target-mutation authority,
inherit a grant, or widen authority. Architecture 20 permits only separately selected verified checkpoint copies for
child kernel seeding: no live kernel, namespace, task, grant, or authority is inherited, and kernel evidence cannot
widen verifier authority.

## Required evidence before implementation

Evidence: activating specification per [architecture 12](12-quality-gates-and-makefile.md).
