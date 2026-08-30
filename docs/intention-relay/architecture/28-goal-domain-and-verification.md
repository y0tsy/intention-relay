# Goal Domain and Verification

## Status and scope

## Traceability

- Normative owner: architecture 28.
- Decision record: [`0023`](../decisions/0023-goal-domain-and-verification-directions.md).
- Detail decisions: [`0030`](../decisions/0030-continual-harness-safe-failures-and-selection-record-detail.md) (harness selection-record content), [`0033`](../decisions/0033-accepted-m5plus-execution-directions.md) (post-disconnect work).
- Reconciliation topics: `GOL-004..013, VGT-001..006`.
- Research provenance: [`m4plus_concept2.md`](../m4plus_concept2.md).
- Status: documentation-approved; implementation-authorized work requires a later activating specification under [Milestone 5+](11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment).

**Approved future architecture, documentation-only.** This document is the sole
detailed owner for the future Goal aggregate domain: Goal identity, scope, and
tree; Goal lifecycle, readiness, and user decision; leading-goal run selection;
delegated Verification Mandates; verification gates and evidence; working
memory, roles, and templates; model proposals and user confirmation; the
conversation-compaction working form; and Goal-domain bounds and closed safe
failures. It does not authorize a crate, implementation, storage migration,
public protocol change, configuration schema, or delivery scope.

It applies to future fresh runs only. M3/M4 bytes, queue tickets, sessions,
runs, events, snapshots, replay, recovery, and `ToolCallRecorded ->
tool_execution_unavailable` retain their recorded ordinary semantics. For new
Mandate work, Goals remain acceptance/evidence records, not the
work-authorization plane; leading-goal, policy, confirmation, and bound
semantics are historical-only where they conflict with architectures 13--27.

## Ownership and non-authorities

Architecture 13 owns Mandate lifecycle and fresh admission. Architecture 14
owns execution-envelope framing and canonical records. Architecture 15 owns the
registry and tool loop. Architecture 17 owns child graph and verifier
authority. Architecture 18 owns MCP lifecycle. Architecture 21 owns context
selection and projection (Goal context selection, Skills, memory/compaction
selection). Architecture 24 owns activity/UI projections. Architecture 27 owns
programmatic-caller policy.

This document owns only the Goal aggregate domain and its verification
semantics. A Goal, revision, link, gate, memory record, role, template,
proposal, or summary cannot create a `RunId` (except through the ordinary
admission path), Mandate reason, lifecycle transition, scheduler candidate,
tool permission, registry slot, child edge, verifier authority, MCP capability,
bridge grant, kernel epoch, branch, or reconciliation result. It is not a
second runtime, registry, scheduler, persistence authority, or sandbox.

## Goal identity, scope, and tree

The daemon assigns `GoalId`, `GoalRevisionId`, and `GoalLinkId`; one immutable
scope belongs to each Goal. A project Goal enters a session only through an
explicit durable link, never by implication; a session Goal belongs only to its
one session. A project Goal may own project or session children; a session
child is allowed only when the owner session has the explicit link (created
atomically if absent); a session Goal cannot own a project child; no crossing
of project or `WorkspaceId`.

```text
GoalScopeDto
  Project { project_id }
  Session { project_id, session_id }

GoalDto
  goal_id
  scope
  active_revision
  lifecycle_state
  readiness_state
  user_decision_state

GoalRevisionDto
  goal_id
  revision
  title
  objective
  inherited_rule_references
  local_rule_references
  required_gate_references
  canonical_revision_digest

GoalParentLinkDto
  parent_goal_id
  child_goal_id
  child_revision_at_link
  required = true
  canonical_link_digest

GoalSessionLinkDto
  project_goal_id
  session_id
  effective_from_revision
  canonical_link_digest
```

The tree is a DAG; every child is an obligatory component, and a parent is not
technically ready until every obligatory child reaches a terminal user-decision
state. Self-link, cycle, duplicate direct child, cross-project link, or
out-of-link session child fails before partial record. A revision is
credential-free, typed, bounded, immutable, and canonicalized; edits create new
revisions and never rewrite admitted-run meaning. A child may add a stricter
rule, remove an optional presentation item, or narrow a limit, but cannot widen
a tool subset, class, quota, scope, required gate, or hard policy; the
effective result is recorded in the child revision.

## Goal lifecycle, readiness, and user decision

```text
GoalLifecycleStateDto
  Active
  NeedsRework
  Paused
  Stopped
  Archived

GoalReadinessStateDto
  NotReady
  Ready { verified_evidence_set }

GoalUserDecisionStateDto
  Unaccepted
  Accepted
  AcceptedWithException { exception_evidence_set }
```

`Ready` means the exact effective revision plus every obligatory child plus
every required gate has selected successful evidence; it is not a model
statement and not synonymous with acceptance. An exception names only a known
failed, unavailable, expired, or externally ambiguous **gate** and its safe
evidence; it never creates success or `Ready`. `AcceptedWithException` is
terminal for the parent's component calculation but carries exception evidence
upward; a parent can be accepted only with an exception set that explicitly
includes each inherited child exception; an exception cannot omit, cancel, or
bypass an active obligatory child.

Required-gate failure moves the Goal to `NeedsRework` (retaining the gate
result and prior evidence); a successful gate is valid only until a new
revision of the Goal, gate, template, or obligatory child invalidates it.
`PauseGoal` and `StopGoal` prevent new ordinary and verification runs, cascade
through the non-terminal subtree, cancel active runs via
`Running -> Cancelling -> Cancelled`, and leave a started external effect
`ExternalEffectUnknown` when unprovable. Pause is reversible via explicit
resume; stop is terminal. Archiving is explicit and reversible only for a
terminal idle Goal; it retains all history as readable and restore changes
presentation only, never launching a run.

## Leading-goal run selection and durable transactions

A run is ordinary (no leading Goal) or goal-directed (exactly one leading
Goal); never multiple coequal Goals after admission. A verification run has
that one Goal with kind `VerificationOnly`; it may collect gate evidence but
cannot alter Goal, memory, Skill, role, template, or connection state.

```text
RunExecutionMeaningDto
  schema_version = run-execution-meaning-v4
  resolved_provider_selection
  model_capability_set
  context_projection_selection
  tool_execution_selection
  reasoning_history_manifest_reference
  terminal_provenance_references
  harness_selection = Disabled | ContinualHarnessSelectionV1 { ... }
  goal_selection = Disabled | GoalRunSelectionV1 { ... }
  mcp_selection = Disabled | McpMethodCatalogSelectionV1 { ... }
  programmatic_caller_policy_selection =
    Disabled | ProgrammaticCallerPolicySelectionV1 { ... }
  agent_activity_selection = AgentActivitySelectionV1 { ... }
```

A launch admitted by the selected continual-harness model carries a separately
versioned `ContinualHarnessSelectionV1` nested record (owned by
[architecture 26](26-continual-harness.md#selection-record-and-closed-safe-failures))
containing the harness identity, active rule revision, durable trigger reason,
class resolution, dossier digest, checkpoint reference, time-zone application,
and immutable bounds. Historical M4 and other non-harness runs do not acquire a
synthetic harness record.

`GoalRunSelectionV1` contains the leading `GoalId`, exact revision,
scope/session-link provenance, ordered parent revision chain, effective
obligatory-component references, selected gate/template revisions and valid
evidence references, selected memory/Skill/role cards and revisions,
already-revealed full-record references, the selected effective
programmatic-caller policy snapshot reference, an `AgentActivitySelectionV1`
reference, run kind, canonical target-snapshot digest, and immutable bounds. It
holds no full memory/Skill/role/summary, credential, provider value, current
machine state, raw transcript, grant, process resource, or handle. Factory
Skills use the exact `SkillSelectionV1` under the frozen snapshot.

The admission transaction validates the Goal and session link, the complete
target snapshot, all references and bounds, provider selection, registry
revision, and applicable policy, then atomically writes the run, selection,
audit evidence, projections, and snapshots, or none. No external action occurs
inside that transaction. An active run retains its frozen selection; edits
create newer revisions for future admission only. Unknown, corrupt,
unavailable, incompatible, or over-limit selected records block dependent
operations before external work; queue promotion, replay, retry, child
admission, fork, and recovery never substitute current state.
`McpMethodCatalogSelectionV1` is `Disabled` when `mcp` is absent from the
frozen model-tool selection; each `mcp` call records the exact one method
reference and typed input digest before external action.

## Delegated Verification Mandates

A Verification Mandate is an ordinary Mandate whose purpose is audit, with its
own revisioned prompt/objective, tools, provider, runs, activity identity,
checkpoints, child work, evidence, and recovery. Its prompt is independent and
never a security boundary; it has no inherited authority.

```text
VerificationMandateAuthorityDto
  authority_id
  verifier_mandate_id
  authority_revision
  target_set_reference
  allowed_operations
  audit_contract_reference
  canonical_authority_digest

VerificationTargetSetDto
  target_set_id
  immutable_targets
  canonical_target_set_digest

VerificationTargetDto
  target_mandate_id
  frozen_target_revision
  baseline_lifecycle_state
  frozen_goal_references
  frozen_gate_and_evidence_contract_references
  baseline_unknown_effect_reference_when_present

VerificationAuditContractDto
  contract_id
  revision
  required_evidence_kinds
  completion_standard
  reconciliation_standard_when_allowed
  canonical_contract_digest

VerificationAuditEvidenceDto
  evidence_id
  authority_reference
  target_reference
  frozen_target_revision
  frozen_goal_references
  frozen_gate_and_evidence_contract_references
  evidence_kind
  retained_content_reference
  canonical_evidence_digest

VerificationTargetOperationDto
  MarkCompleted
  MarkNeedsRework
  Pause
  Resume
  Stop
  ReviseFull
  ResolveUnknownEffect

VerificationTargetMutationDto
  mutation_id
  authority_reference
  audit_contract_reference
  target_reference
  operation
  audit_evidence_references
  expected_target_revision
  idempotency_key
  canonical_mutation_digest

VerificationAuditVerdictDto
  Pass
  Fail
  Inconclusive
  TargetRevisionStale
  TargetUnavailable
  VerifierUnavailable
  VerifierExternalEffectUnknown
```

Authority is usable only while its revision is active, unrevoked, unexpired,
and unconsumed where required, and is owned by the named verifier Mandate; it
cannot target itself. The target set is immutable and explicitly enumerated,
never expands, and child work cannot inherit, relay, or amplify mutation
authority. `ReviseFull` may create a complete new target revision but never
rewrites history or alters a live old run. User reconciliation uses the same
durable record family without verifier authority and may yield only `Active` or
`Stopped`. A baseline change, missing baseline, or incompatibility before a
verdict or mutation is `TargetRevisionStale` and fails closed; user mutations
win via ordinary optimistic concurrency; a verdict is durable evidence, never
an implicit trigger. A mutation commits atomically or not at all; a duplicate
equal mutation returns the saved result; changed key reuse fails.
`ResolveUnknownEffect` is allowed only when the authority explicitly includes
it, the target is `PausedAwaitingDecision`, and the contract/evidence proves
the reconciliation standard on an unchanged baseline; the sole outcomes are
`Resume` (to `Active`) or `Stop`. The verifier's own `ExternalEffectUnknown`
pauses only the verifier at `PausedAwaitingDecision` and never mutates the
target. Activity records: `VerificationAuthorityIssued`,
`VerificationAuditStarted`, `VerificationAuditVerdictRecorded`,
`VerificationTargetMutationApplied`, `VerificationTargetMutationRejected`,
`VerificationUnknownEffectReconciled`. Recovery never resumes verifier/target
work, repeats tools, or reapplies mutations; historical M4 and v1--v4 records
never acquire synthetic verifier authority.

## Verification gates and evidence

```text
VerificationGateDto
  ReferenceGate { evidence_contract_revision, accepted_reference_kinds }
  ExecutableGate { template_id, template_revision }
```

A `ReferenceGate` validates an exact durable reference (terminal child result,
accepted user declaration, or terminal registered-tool result). An
`ExecutableGate` names a user-created typed template at one exact revision; no
raw shell text, arbitrary path/URL/header map, executable code, opaque JSON, or
model-supplied provider resource. A template uses a registered capability plus
one closed typed input family; the owner validates effect profile, mode,
required confirmation, and selected class before execution. Gate templates have
`Project`/`Goal`/`Session` scope, immutable revisions, safe cards, explicit
user create/edit/archive/restore, and typed provenance; a model may only prepare
a template proposal under the proposal rules. An executable gate runs in the
terminal verification phase of an ordinary leading-goal run or a separately
user-admitted `VerificationOnly` run, using the same typed evidence family with
distinct provenance. Failure, timeout, cancellation, output bound, unknown
effect, missing reference, stale revision, or unavailable template are known
typed outcomes; no hidden retry, no next-phase readiness claim; retries occur
only before the first irreversible durable fact and never repeat a started
external action.

## Working memory, roles, and templates

```text
MemoryKindDto
  Fact
  Decision
  Preference
  PastFailure
```

Records are first-class typed durable records with scopes `Project`/`Goal`/
`Session`; session scope stays in its session; project/Goal records reach a
session only via the selected Goal/session link and frozen target snapshot.
Every record has a daemon-assigned identity, owner scope, immutable revisions,
canonical digest, safe card, lifecycle state, source/provenance, bounded typed
references, and explicit archive/restore and replacement/rollback relation. The
daemon never decides two texts conflict; a newer record replaces an older one
only via an explicit typed replacement link (identity and revision); without it
both cards are visible; rollback creates a new immutable revision linked to an
earlier record and never rewrites a historical selection. Every applicable
active card (project, selected Goal chain, current session) enters the target
snapshot; a card includes only kind, title, scope, bounded safe purpose, exact
revision, digest, and typed retained-content reference. Full content is revealed
only by explicit `retrieve` against that reference (no new `ToolId`, no body in
the snapshot, no replacement of a historical reference); an unavailable or
incompatible full record blocks only the dependent disclosure/model step.

A Skill is a bounded textual procedure with typed references to memory, roles,
gate templates, and registered capabilities; no binary, executable, install,
plug-in, dynamic registration, arbitrary MCP schema, or hidden external action.
A Role is a named reusable `sub_agent` template (task, permitted class, tool
subset, contextual limits, typed references); a concrete use may only narrow
task, class, context/result limits, and tool subset; it cannot add a tool,
increase a class, widen scope, bypass a gate, change a provider, or weaken
policy. Skill and role cards are discovered first; full records require
explicit disclosure.

## Model proposals and user confirmation

After one of these durable Goal milestones the model may prepare at most one
coalesced proposal of a particular owner scope and record kind: technical
readiness; user acceptance, acceptance with exception, or stop; required-gate
failure; or a terminal outcome of an obligatory child. `RefinementDraftDto`
contains a daemon-assigned identity, selected source run/Goal and milestone,
exact base revisions, bounded typed edit set, evidence references, safe
rationale, and canonical digest. It is not a current record, does not appear in
cards or model context, cannot be used by a child/Skill/role/gate/harness/MCP
method, and grants no execution authority. A later equal proposal adds evidence
to the one pending draft rather than producing an unbounded queue. The daemon
records the proposal durably before asking the user; via the normal `ask_user`
path the user may accept, edit-and-accept, or reject; the draft remains pending
until an explicit decision. Acceptance validates the exact base revision and
creates a new immutable record revision; a stale base is a typed conflict;
rejection changes no active record. Programmatic-caller policy uses the same
user-confirmed flow for its separate inactive policy drafts under
[architecture 27](27-programmatic-caller-policy-and-admission.md).

## Conversation compaction and progressive context disclosure

Compaction is a versioned model-context projection, not a replacement for the
source transcript, model facts, tool facts, reasoning, kernel checkpoint,
harness checkpoint, child dossier, or Goal evidence. `ConversationSummaryDto`
covers one continuous completed durable-history range and contains schema/
canonicalization version, start/end references, a previous-summary reference
when present, bounded safe content, digest, and provenance; original facts
remain readable. The working form is one cumulative current summary plus the
later uncompacted suffix. A new revision made inside an already active run uses
the previous selected summary plus the next bounded completed source range and
becomes part of the next model step in the same run; it executes no registered
tool, creates no service run, and cannot run before a user-admitted run, after
terminalization, or after restart. A user or model may request it in an active
run; if the selected model-context bound would be exceeded, the daemon requires
it in that same run rather than silently creating another. Correcting a summary
creates a separately immutable later revision with exact source and predecessor
references; the earlier summary remains historical evidence. Fork creation
stores only exact compatible selected summary references in the frozen
projection and never reads a current ancestor or imports a future summary.
Headroom, VFR, and `retrieve` retain their own semantics and never turn source
content into summary text implicitly.

## Branches, children, cancellation, and recovery

A new session fork receives a frozen copy of applicable project-Goal links,
session memory/Skill/role/template cards, exact revisions, and the selected
current summary reference in an independent fork snapshot. It does not receive
a new session Goal, active Goal run, grant, kernel namespace, current ancestor
record, local MCP process, external connection resource, or future source
change; the user explicitly creates any new session Goal in the branch.
Programmatic-caller policy uses a separate immutable session-policy
inheritance record: a fork refers to the same source-session policies and the
same durable counters rather than copying a fresh allowance; a branch may add
only a new policy that narrows the inherited effective policy.

An admitted child agent receives `GoalDelegationSnapshotV1` inside the existing
bounded `SubAgentDelegationSnapshotDto`: parent task, leading-Goal identity and
revision, effective required constraints, applicable cards, selected role when
any, selected effective programmatic-caller-policy snapshot reference, selected
`AgentActivitySelectionV1` reference, and only the required safe references; not
a full parent transcript or live target context. Later parent/Goal/memory/
Skill/role/policy/activity edits do not change the child snapshot, though live
suspension or revocation can impose a stricter present-time denial. A child
uses independently assigned session/run/class/tool/provider selections and
cancellation/no-resume rules. On cancellation, failure, or daemon restart, no
provider request, gate, tool, MCP request, process, kernel action, child, or
external effect is retried, reattached, resumed, or rerun; unfinished work
becomes `Interrupted` with already selected known pre-effect or unknown-effect
evidence. Goals, records, summaries, proposals, and readable history survive
recovery; a later user attempt obtains a new run identity and target snapshot.

## Bounds and closed safe failures

The first scope uses these code-owned limits:

| Subject | Selected limit |
| --- | ---: |
| Goals in one project | 256 |
| Goals in one session | 64 |
| Goal-tree depth | 16 |
| Direct children of one goal | 32 |
| Session links of one project goal | 64 |
| Gates of one goal | 32 |
| Active applicable memory cards in one run | 128 |
| Selected skills and roles in one run | 32 |
| Full memory, skill, role, template, summary, draft, or safe gate result | 512 KiB |
| Canonical goal target snapshot | 1 MiB |
| Goal context, revealed records, and summaries in one run | 4 MiB |
| Pending proposal per owner scope and record kind | 1, with evidence coalescing |

No content is truncated or partly committed to satisfy a bound. The Goal-domain
closed safe failures through `ErrorDto` are:

```text
goal_limit_exceeded
goal_tree_depth_limit_exceeded
goal_child_limit_exceeded
goal_session_link_limit_exceeded
goal_cycle_detected
goal_not_active
goal_revision_conflict
goal_snapshot_too_large
goal_snapshot_unavailable
goal_gate_limit_exceeded
goal_gate_unavailable
goal_gate_failed
goal_not_ready
goal_acceptance_exception_invalid
goal_archive_not_terminal
memory_entry_limit_exceeded
memory_entry_too_large
memory_reference_unavailable
memory_replacement_conflict
skill_entry_too_large
skill_reference_unavailable
delegation_role_invalid
delegation_role_widening_forbidden
compaction_summary_too_large
compaction_summary_unavailable
compaction_history_unavailable
refinement_draft_conflict
refinement_draft_too_large
```

They disclose no credential, path, raw external response, private process
resource, grant, full memory/Skill/role/template content, model proposal text,
provider resource, or implementation detail. The package continues to exclude
autonomous continuation, work after client disconnection,
attachments/images/binary/rich-MIME input, dynamic extensions and
installation, dynamic tool registration, physical deletion, and administration
of long-lived workers, leases, attach/detach, force-kill, or supervisor
recovery. Work/continuation/requeue after client disconnection is an accepted
post-M5 future direction under
[ADR 0033](../decisions/0033-accepted-m5plus-execution-directions.md), to be
executed in Milestone 5+ as an explicit durable contract that never silently
resumes old external work; it is not activated here.

## Compatibility and historical preservation

- M3/M4 queue tickets, sessions, runs, events, snapshots, replay, and recovery
  remain authoritative and unchanged; no historical record gains synthetic
  Goal, gate, memory, proposal, or compaction state.
- For new Mandate work, Goals are acceptance/evidence records, not the
  work-authorization plane; leading-goal, policy, confirmation, and bound
  semantics are historical-only where they conflict with architectures 13--27.
- All directions affect fresh runs only, activated under Milestone 5+.

## Dependencies and non-goals

This document depends on architectures 13, 14, 15, 17, 18, 21, 24, and 27 plus
decisions 0001, 0003, 0006, 0007, 0009, 0010, 0013, 0022, and 0023. It does not
define actual Goal persistence, search/index/vector retrieval, prompt assembly,
SQL/wire tags, migrations, retention/deletion/encryption, source-page sizes,
resource values, provider evolution, session branching, activity/UI, physical
Plan artifacts, direct MCP administration, Python/Jupyter process behavior,
Cargo, Makefile/CI, or production activation.

A later activating specification must declare exact crates, dependencies, test
targets, coverage tiers, feature profiles, storage/wire schema, retention, and
bounds, then pass `make quick`, `make docs-check`, `make architecture`,
`make verify`, and Linux/Windows CI. Required evidence includes:

- Goal identity/scope/tree, obligatory children, DAG integrity, and
  no-cross-project fixtures;
- lifecycle/readiness/user-decision state machines, exception evidence, and
  pause/stop/archive fixtures;
- leading-goal run-selection admission fault injection and
  no-current-state reconstruction fixtures;
- delegated Verification Mandate authority, target-set, stale-baseline,
  operation matrix, and reconciliation fixtures;
- reference/executable gate fixtures with no hidden retry and no raw
  shell/URL/JSON;
- memory/role/template card, disclosure, replacement, and rollback fixtures;
- proposal coalescing, accept/edit/reject, and stale-base conflict fixtures;
- compaction working-form, correction, fork-reference, and bound fixtures;
- bound/closed-safe-failure and fake-secret regression across logs, errors,
  snapshots, events, and adapter DTOs.
