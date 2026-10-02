# Programmatic Caller Policy and Admission

## Status and scope

## Traceability

- Normative owner: architecture 27.
- Decision record: [`0022`](../decisions/0022-programmatic-caller-policy-directions.md), amended by [`0048`](../decisions/0048-limits-by-precedent-and-no-content-scanning.md) (corridors, reservations, and calendar limits removed).
- Reconciliation topics: `PCP-001..008`.
- Research provenance: `m4plus_concept.md`.
- Status: documentation-approved; implementation-authorized work requires a later activating specification under [Milestone 5+](11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment).

**Approved future architecture, documentation-only.** This document is the sole
detailed owner for the future programmatic-caller policy and admission model:
root origins and durable provenance, durable policy identity/scope/narrowing,
admission decisions and typed input constraints, exact user confirmation,
policy lifecycle and live tightening, and run-selection compatibility. The
corridor, reservation, and run/calendar-limit clauses were removed by
[ADR 0048](../decisions/0048-limits-by-precedent-and-no-content-scanning.md).
It does not authorize a crate,
implementation, protocol, storage migration, configuration, network connection,
local process, or delivery scope.

It applies to future fresh runs only. M3/M4 bytes, queue tickets, sessions,
runs, events, snapshots, replay, recovery, and `ToolCallRecorded ->
tool_execution_unavailable` retain their recorded ordinary semantics. This
policy is logical product control and audit evidence, not an operating-system
security boundary against code running with the user's ordinary OS authority.
It does not create a durable autonomous actor, a second daemon, a second tool
registry, a remote identity, or an authority that survives an active daemon-held
run.

## Ownership and non-authorities

Architecture 13 owns Mandate lifecycle and fresh admission. Architecture 15
owns the registry, frozen direct-tool selection, tool admission, and the model
tool loop. Architecture 17 owns child creation and verifier authority.
Architecture 18 owns MCP lifecycle. Architecture 19 owns bridge grants and
ingress. Architecture 22 owns provider selection. Architecture 24 owns
activity/UI projections. Architecture 26 owns the continual-harness model,
whose `sub_agent` use is gated by the exact confirmation defined here.

A root origin, provenance record, policy, revision, confirmation,
counter, draft, or snapshot cannot create a `RunId`, Mandate
reason, lifecycle transition, scheduler candidate, tool permission, registry
slot, child edge, verifier authority, MCP capability, bridge grant, kernel
epoch, context projection, branch, or reconciliation result. It is not a second
runtime, registry, scheduler, persistence authority, or sandbox. A local
protocol peer remains an adapter under the ordinary operating-system-user
boundary, not an account or a caller-selected principal.

## Root origin, calling path, and durable provenance

Every programmatic action has one daemon-assigned root origin:

```text
ProgrammaticCallerRootOriginDto
  InteractiveUser { originating_turn_id }
  ContinualHarness { harness_id, rule_revision, trigger_reason_id }
```

`InteractiveUser` is the root of an ordinary user-admitted run and all of its
descendants. `ContinualHarness` is the root of one separately admitted harness
launch and all of its descendants. These values are not account identities,
credentials, operating-system identities, or user-supplied input. No third root
exists in this first scope: a protocol peer, detached Python task, child agent,
MCP service, provider, bridge channel, queued item, replay, and daemon recovery
cannot become an independent root.

The policy distinguishes only the root origin. The daemon nevertheless retains
the exact internal calling path as immutable audit provenance:

```text
ProgrammaticCallerProvenanceDto
  root_origin
  root_session_id
  root_run_id
  current_session_id
  current_run_id
  parent_link_references
  bridge_operation_reference_when_present
  leading_goal_reference_when_present
  tool_call_id
  selected_tool_id
  selected_descriptor_revision
  selected_mcp_method_reference_when_present
  typed_input_identity
  effective_policy_snapshot_reference
  admission_basis_reference
  provenance_record_identity
```

The daemon creates this record before `ToolCallStarted`, together with the
admission outcome it explains. It records no raw input, workspace path, grant,
credential, provider value, Python value, socket, process handle, external
response, or implementation resource. A returned tool result may refer to the
safe provenance record but does not turn it into model context by itself.

`WorkspaceRoot` is the addressing anchor for relative paths, the child-process
CWD, and the default glob/grep scope; it is not a security boundary, and no
lexical symlink parser or containment check exists
([ADR 0047](../decisions/0047-workspace-root-addressing-anchor.md)). The former
`WorkspacePathOutsideObserved` audit record and its `LexicallyOutsideRoot` /
`ResolvedLinkOutsideRoot` observation kinds were removed with that machinery;
absolute paths and `..` are used as supplied. This makes no claim about
filesystem activity hidden inside `execute`, IPython, or child processes.

Every gateway request remains bound to an active daemon-held run. A live
`BridgeRunGrantDto` is necessary transport evidence for a bridge request but is
not a policy selection, authorization, or durable fact. Its expiry, channel
detachment, or daemon exit cannot transfer a pending call into another run. A
future request without the active context fails before a primitive, provider,
kernel, process, network call, or child admission.

## Durable policy identity, scope, and narrowing

Programmatic policy is a first-class durable record, separate from a goal,
memory card, skill, role, gate template, harness rule, or MCP connection. The
daemon assigns `ProgrammaticCallerPolicyId` and immutable revisions:

```text
ProgrammaticCallerPolicyDto
  policy_id
  scope
  lifecycle_state
  active_revision
  policy_record_identity

ProgrammaticCallerPolicyRevisionDto
  policy_id
  revision
  root_origin_rules
  admission_rules
  inherited_policy_references
  revision_identity

ProgrammaticCallerPolicyScopeDto
  Project { project_id }
  Goal { project_id, goal_id }
  Session { project_id, policy_owner_session_id }
```

A project policy applies to every current and future session in its project,
including ordinary, child, branch, and harness service sessions. This is
intentionally different from a project goal, which remains applicable only
through its selected explicit goal-to-session link. A goal policy is applicable
only when that goal is the leading goal or an ancestor in its frozen effective
goal chain. A session policy applies to its owner session and to a fork only
through the explicit immutable inherited-policy reference recorded by that fork.
It never crosses a project or `WorkspaceId`.

At run admission the daemon resolves applicable project, selected-goal-chain,
and session policies into one immutable
`EffectiveProgrammaticCallerPolicySnapshotDto`. It records each policy ID and
revision, scope provenance, root-origin constraint, ordered narrowing result,
and a typed snapshot identity. It holds
no full rule text, raw input, credential, process resource, current counter
value, live policy state, or confirmation. The snapshot is immutable historical
execution meaning; it is not reconstructed from a current policy projection.

Every applicable rule constrains a call by the intersection of both of these
independent selectors:

1. the selected tool's declared `ToolEffectProfileDto` flags; and
2. the exact `ToolId`, plus an exact selected MCP connection/method revision
   where the tool is `mcp`.

An effect selector alone cannot admit a different tool, and an exact tool or
MCP method selector cannot escape a stricter applicable effect selector. The
most restrictive decision, smallest bound, and narrowest input constraint win.
A child goal, session policy, role, child-agent selection, or harness class may
only add a restriction. It cannot add a tool, remove an effect condition,
broaden a method, increase a class, extend a scope, enlarge a limit, or weaken
a confirmation requirement. A nominally stronger child class is valid only if
the resulting effective policy still narrows the parent selection.

A session fork stores immutable references to the source session policies and
their `ProgrammaticCallerPolicyId` values rather than copies. A branch can add
only a new policy that narrows the inherited intersection. Thus a fork cannot
obtain a fresh allowance by copying, changing a title, or creating a new
session. Its materialized historical context remains immutable even if a later
live policy makes a future action unavailable.

## Decisions, typed input constraints, and confirmation

The closed first policy decision is:

```text
ProgrammaticAdmissionRuleDto
  Prohibited
  DirectLocalRead
  ExactConfirmationRequired
  BoundedConfirmationRequired
```

The `BoundedConfirmationRequired` variant remains part of the closed decision
vocabulary; the bounded corridor that once carried it was removed by
[ADR 0048](../decisions/0048-limits-by-precedent-and-no-content-scanning.md), so
each admission it names is an exact confirmation bound to one call.

`DirectLocalRead` is valid only for `InteractiveUser` and only for `read`,
`glob`, `grep`, `expand`, or `retrieve`. It permits local reading or disclosure
of already retained content, subject to the frozen registry, descriptor,
workspace-relative resolution from `WorkspaceRoot`, mode, hooks, and every
stricter policy. It does not state that a file is safe,
current, or free of sensitive content. A direct policy never admits `fetch_url`,
`write`, `edit`, `execute`, `plan_submit`, `sub_agent`, `mcp`, user interaction,
a network call, a process, or a state mutation.

When no durable policy is applicable, an `InteractiveUser` root receives the
same narrow direct-local-read baseline. It applies only to the root run itself,
not automatically to a Python facade, child agent, or any descendant. A
`ContinualHarness` root has no such baseline: its selected read-and-delegate
tool subset and separately selected policy must admit each action. Every other
call requires an exact confirmation.

The baseline itself is a code-owned `InteractiveLocalReadBaselineV1` selection.
Its former numeric bounds (256 root-run actions and 16 concurrent actions) and
its calendar counter were speculative and were removed by
[ADR 0048](../decisions/0048-limits-by-precedent-and-no-content-scanning.md). It
is frozen into the current typed run selection like every other selected rule. A
durable policy may only narrow this baseline. Thus the absence of a stored policy
does not produce a wider admission path.

An admission rule may select only descriptor-declared closed typed input
constraint families. A `DescriptorInputConstraintSelectionDto` names its
family, revision, and typed values; the descriptor is solely responsible for
validation. It cannot contain a raw command, code fragment, unvalidated path,
free-form URL, raw JSON, map, provider object, arbitrary header, dynamic MCP
method name, pattern language, or newly discovered schema.

`execute` therefore never receives `DirectLocalRead` in this first scope.
`ShellCommandTextDto` deliberately permits
pipelines, redirects, and compound commands, so WorkspaceRoot CWD does not make
its input a closed constraint. It remains available only through an
exact user confirmation that displays the exact typed command and still applies
WorkspaceRoot CWD, mode, hook,
output, cancellation, and no-resume rules. A future typed command-template
direction would require a separately selected contract.

`fetch_url` never receives direct admission; it is admitted only through an
exact confirmation whose typed input declares a compatible constraint family and
whose redirects remain within the selected descriptor-fixed restrictions. `mcp`
never receives direct admission: its exact confirmation must name one selected
connection, method, schema, gateway revision, and supported typed input
constraint family.
An MCP service cannot ask the user, create a confirmation, create a goal,
connection, run, child, message, or authority context.

An exact confirmation is one durable user decision bound to exactly one
`ToolCallId`, root tree, `ToolId`, descriptor revision, MCP-method reference
when present, typed input identity, and the applicable policy-snapshot identity.
It cannot be replayed for another call, input, descendant
tree, revision, or later run.

The former bounded confirmation was a user-approved
`ProgrammaticAuthorizationCorridorDto` shared by one root tree. It was removed
from the direction by
[ADR 0048](../decisions/0048-limits-by-precedent-and-no-content-scanning.md)
together with its selectors, shared counts, expiry record, and digest: every
exact confirmation binds one `ToolCallId`, and a repeated call requires its own
confirmation.

`ask_user` remains a normal long-running registered tool rather than a policy
outcome. Only an `InteractiveUser` root may start it. When a descendant needs
an exact confirmation, the daemon records the descendant's safe
provenance but creates the typed question only for the root run. The child,
Python facade, role, harness, and MCP service never directly start `ask_user`.
A harness cannot await user interaction: its `sub_agent` use must be admitted by
an exact confirmation that already exists for the call, or it fails before
child admission.

## Policy lifecycle, live tightening, and drafts

The durable policy lifecycle is closed:

```text
ProgrammaticCallerPolicyLifecycleStateDto
  Active
  Suspended
  Revoked
  Archived
```

An ordinary user edit creates a new immutable active revision only for future
admission. It does not reinterpret an admitted run's frozen effective-policy
snapshot or confirmation. A current live state may
nevertheless impose a stricter decision:

- `SuspendPolicy` blocks every not-yet-started matching call, cancels pending
  confirmations, and permits no new admission.
  It does not claim to roll back or silently stop an action that already reached
  `ToolCallStarted`.
- `ResumePolicy` may make a suspended policy active again only through an
  explicit user operation on its then-current active revision. It does not
  revive a cancelled run or an expired confirmation.
- `RevokePolicy` atomically creates a later immutable revision whose admission
  is disabled, enters `Revoked`, marks every active root tree that selected the
  policy for cancellation, and denies new admissions. Each affected tree
  follows the existing `Running -> Cancelling -> Cancelled` path. A started
  effect whose final result is not provable remains `ExternalEffectUnknown`.
- A later user decision may create a **new** active revision of the same policy
  identity, retaining its counter history. Revocation is never undone by
  reactivating an old revision, by restoring an archive, or by replay.
- Only a revoked policy with no active dependent tree may be archived. Archive
  is reversible presentation retention only; it never grants authority or
  removes readable revisions, counters, confirmations, provenance, or audit.
  Physical deletion and counter erasure are excluded.

`AutomationPaused` for a continual harness remains distinct from a suspended
policy. The former coalesces automatic triggers while retaining explicit user
launch as defined by the harness model. The latter is a stricter live denial:
it blocks both automatic and explicit admissions that depend on that policy.

The model may prepare one inactive `ProgrammaticCallerPolicyDraftDto` for any
applicable project, goal, or session scope. The draft always displays its
proposed scope, root-origin applicability, selected rules, evidence references,
base revisions, safe rationale, and a typed record identity. It may arise after
the selected goal milestones or a policy denial. An equal later proposal
coalesces evidence into that one pending draft. The draft is not a policy, card,
target-snapshot input, confirmation,
tool selection, or authority. The daemon records it before the root-only user
question; the user may accept, edit and accept, or reject it. Acceptance checks
the exact base state and creates an immutable policy or policy revision.
Rejection changes no policy. No harness rule, policy, or policy revision exists
because a model merely proposed it.

## Admission transaction and recovery

No run or calendar limit, counter, or reservation remains in this direction: the
former per-run limits (256 actions and 16 concurrent actions), the calendar
action limit and its `Day`/`Week`/`Month` period kind, and the atomic
policy-counter reservation transaction were speculative and were removed by
[ADR 0048](../decisions/0048-limits-by-precedent-and-no-content-scanning.md). A
future numeric bound returns only with its recorded precedent naming the failure
mode it prevents, its unit, and its behavior at the bound.

Before `ToolCallStarted`, the daemon atomically verifies every applicable
effective policy, all exact selectors and input constraints, the confirmation,
and the live policy state. It then writes the policy admission evidence
and the tool admission outcome in one transaction, or writes none of them. No
provider, tool, shell, process, filesystem action with an external effect,
network call, kernel operation, MCP process, or child admission occurs in that
transaction.

An equal repeated operation reads the accepted idempotent binding and is not a
second admission. A denial, invalid input, expiration, cancellation, or known
failure before `ToolCallStarted` leaves the run unchanged with the
terminal known pre-effect outcome. `ToolCallStarted` fixes the admitted
outcome, even when later cancellation,
loss, or ambiguity yields `ExternalEffectUnknown`. Recovery records
`InterruptedBeforeStart`, or
records `ExternalEffectUnknown` for a started ambiguous action and never
retries it. It never re-runs the action to recreate an admission.

No numeric contract limit remains in the policy scope: the former counts of
policies, policy references, rules, and typed constraints, the confirmation and
corridor counts, and the fixed evidence, snapshot, and pending-draft sizes were
speculative and are removed by the same record.

Content is never truncated or
partly committed to fit a bound. An unavailable or incompatible
policy, snapshot, or confirmation blocks only the dependent action
before an external effect. It never falls back to the current policy, a current
default, a live ancestor, or a fresh authorization.

## Run-selection compatibility and closed safe failures

The selected immutable policy selection is a typed serde JSON record; the
former `run-execution-meaning-v4` carrier, which extended the former v3 field
table with the selection at tag 11, was removed with the canonical codec by
[ADR 0046](../decisions/0046-typed-serde-json-contracts.md):

```text
ProgrammaticCallerPolicySelectionV1
  root_origin
  effective_policy_snapshot_reference
  policy_selection_identity
  inherited_scope_provenance
```

The selection is `Disabled` only for historical M4 records. Every new
ordinary, goal-directed, verification, child, and harness run carries this
selection, including a selection that contains only the narrow interactive
direct-local-read baseline.

`GoalRunSelectionV1`, `SubAgentDelegationSnapshotDto`,
`GoalDelegationSnapshotV1`, `ContinualHarnessSelectionV1`, and
`McpMethodCatalogSelectionV1` retain only safe typed references to it and to any
later admission evidence. No historical M4 snapshot, event,
`RunId`, replay, or `tool_execution_unavailable` result is rewritten or given a
synthetic policy record.

Live suspension, revocation, registry availability, and daemon
readiness remain outside the immutable selection. They may impose a
stricter present-time denial, but never rewrite historical semantics, reroute a
call, substitute a current policy snapshot, or resume external work.

At a minimum, the policy adds these closed safe failures through `ErrorDto`:

```text
programmatic_policy_snapshot_too_large
programmatic_policy_snapshot_unavailable
programmatic_policy_revision_conflict
programmatic_policy_inheritance_widening_forbidden
programmatic_policy_origin_invalid
programmatic_policy_not_applicable
programmatic_policy_suspended
programmatic_policy_revoked
programmatic_policy_confirmation_required
programmatic_policy_confirmation_expired
programmatic_policy_root_only_interaction
programmatic_policy_input_constraint_mismatch
programmatic_policy_harness_delegation_forbidden
programmatic_policy_draft_conflict
programmatic_policy_draft_too_large
```

They disclose no policy body, raw input, path, grant, credential, Python value,
provider resource, process topology, external response, counter history, or
implementation detail. Every listed failure is known before an external effect,
except that a later cancellation or recovery preserves the independently
selected `ExternalEffectUnknown` evidence for work that had already started.

## Compatibility and historical preservation

- M3/M4 bytes, queue tickets, sessions, runs, events, snapshots, replay, and
  recovery remain authoritative and unchanged; no historical record gains a
  synthetic policy state.
- The `Disabled` policy selection applies only to historical M4 records; it is
  never rewritten.
- For new Mandate work, retained RLM run-rooted activity identity, root-origin,
  direct-pair queue, and fixed observation limits are historical-only where they
  conflict; the Mandate child-work graph and its immutable links own activity
  identity across fresh runs.
- All directions affect fresh runs only, activated under Milestone 5+.

## Dependencies and non-goals

This document depends on architectures 13, 15, 17, 18, 19, 22, 24, and 26 plus
decisions 0001, 0004, 0007, 0009, 0010, 0011, 0014, 0021, and 0022. It does not
define a durable autonomous actor, a second daemon, a second tool registry, a
remote identity, an OS security boundary, a typed command-template direction, a
new policy decoder, or production activation.

A later activating specification must declare exact crates, dependencies, test
targets, coverage declarations under [ADR 0049](../decisions/0049-base-coverage-threshold.md), feature profiles, storage/wire schema, retention, and
bounds, then pass `make quick`, `make docs-check`, `make architecture`,
`make verify`, and Linux/Windows CI. Required evidence includes:

- root-origin and provenance fixtures with no third root and no raw-input
  leakage;
- policy scope/narrowing fixtures: intersection, most-restrictive-wins,
  child-narrowing-only, fork shared-policy-reference, and
  inheritance-widening rejection;
- decision fixtures: `DirectLocalRead` baseline, exact confirmation
  single-binding, `execute`/`fetch_url`/`mcp` exact-confirmation constraints,
  and `ask_user` root-only;
- lifecycle fixtures: suspend/resume/revoke/archive, live-tightening, draft
  coalescing, and no-reactivation-after-revoke;
- admission fixtures: atomic pre-start transaction, idempotent equal-replay,
  and `InterruptedBeforeStart`/`ExternalEffectUnknown` recovery;
- run-selection compatibility fixtures: typed selection contract, `Disabled`
  for historical M4, and no-current-state reconstruction;
- closed safe-failure and fake-secret regression across logs, errors,
  snapshots, events, and adapter DTOs.
