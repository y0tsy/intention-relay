# Programmatic Caller Policy and Admission

## Status and scope

## Traceability

- Normative owner: architecture 27.
- Decision record: [`0022`](../decisions/0022-programmatic-caller-policy-directions.md).
- Reconciliation topics: `PCP-001..008`.
- Research provenance: [`m4plus_concept2.md`](../m4plus_concept2.md).
- Status: documentation-approved; implementation-authorized work requires a later activating specification under [Milestone 5+](11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment).

**Approved future architecture, documentation-only.** This document is the sole
detailed owner for the future programmatic-caller policy and admission model:
root origins and durable provenance, durable policy identity/scope/narrowing,
admission decisions and typed input constraints, confirmation and bounded
corridors, policy lifecycle and live tightening, run and calendar limits with
reservations, and run-selection compatibility. It does not authorize a crate,
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
whose `sub_agent` use is gated by the corridor defined here.

A root origin, provenance record, policy, revision, corridor, confirmation,
counter, reservation, draft, or snapshot cannot create a `RunId`, Mandate
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
  typed_input_digest
  effective_policy_snapshot_reference
  admission_basis_reference
  limit_reservation_references
  canonical_provenance_digest
```

The daemon creates this record before `ToolCallStarted`, together with the
admission outcome it explains. It records no raw input, workspace path, grant,
credential, provider value, Python value, socket, process handle, external
response, or implementation resource. A returned tool result may refer to the
safe provenance record but does not turn it into model context by itself.

For a tool whose typed input contains one or more local paths, the daemon may
also create a best-effort audit-only `WorkspacePathOutsideObserved` record. It
contains only the `ToolCallId`, `ToolId`, selected descriptor revision, a closed
observation kind, and the count of explicit path values observed outside the
session's default root. It contains no source, normalized, absolute, canonical,
or symlink-target path; no `WorkspaceRoot`; no CWD, command, content, path
digest, operating-system detail, or file error. The record is safe audit data,
never model context, a public tool-result field, a notification, a confirmation,
or a sanction. Failure to persist this optional record never blocks, changes,
retries, or rolls back the tool call.

```text
WorkspacePathObservationKindDto
  LexicallyOutsideRoot
  ResolvedLinkOutsideRoot
```

The workspace owner first resolves a relative path from `WorkspaceRoot` or uses
an absolute path as supplied. It records `LexicallyOutsideRoot` when the
lexically resolved path is outside the root. When a symlink or an existing
parent can be resolved, it additionally records `ResolvedLinkOutsideRoot` if the
resolved target is outside. Missing, inaccessible, changing, or unresolvable
links produce no guessed observation and never deny access. This observation
covers explicit path-bearing DTOs only; it makes no claim about filesystem
activity hidden inside `execute`, IPython, or child processes.

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
  calendar_period_kind
  lifecycle_state
  active_revision
  canonical_policy_digest

ProgrammaticCallerPolicyRevisionDto
  policy_id
  revision
  root_origin_rules
  admission_rules
  per_run_limits
  calendar_limit
  inherited_policy_references
  canonical_revision_digest

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
per-run limits, calendar-counter identities, and a canonical digest. It holds
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
their `ProgrammaticCallerPolicyId` values rather than copies. The source and all
such forks consume the same calendar counters. A branch can add only a new
policy that narrows the inherited intersection. Thus a fork cannot obtain a
fresh calendar allowance by copying, changing a title, or creating a new
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

`DirectLocalRead` is valid only for `InteractiveUser` and only for `read`,
`glob`, `grep`, `expand`, or `retrieve`. It permits local reading or disclosure
of already retained content, subject to the frozen registry, descriptor,
WorkspaceRoot default resolution and observation where applicable, mode, hooks,
per-run limit, and every stricter policy. It does not state that a file is safe,
current, or free of sensitive content. A direct policy never admits `fetch_url`,
`write`, `edit`, `execute`, `plan_submit`, `sub_agent`, `mcp`, user interaction,
a network call, a process, or a state mutation.

When no durable policy is applicable, an `InteractiveUser` root receives the
same narrow direct-local-read baseline. It applies only to the root run itself,
not automatically to a Python facade, child agent, or any descendant. A
`ContinualHarness` root has no such baseline: its selected read-and-delegate
tool subset and separately selected policy must admit each action. All other
calls require an exact confirmation or a bounded confirmation corridor.

The baseline itself is a code-owned `InteractiveLocalReadBaselineV1` selection
with a maximum of 256 root-run actions and 16 concurrent actions. It has no
calendar counter, cannot create a corridor, and is frozen into the v3 policy
selection like every other selected rule. A durable policy may only narrow this
baseline. Thus the absence of a stored policy does not produce an unbounded or
unaccounted admission path.

An admission rule may select only descriptor-declared closed typed input
constraint families. A `DescriptorInputConstraintSelectionDto` names its
family, revision, and typed values; the descriptor is solely responsible for
validation. It cannot contain a raw command, code fragment, unvalidated path,
free-form URL, raw JSON, map, provider object, arbitrary header, dynamic MCP
method name, pattern language, or newly discovered schema. A descriptor that
cannot express the needed constraint cannot participate in a bounded corridor
for that call.

`execute` therefore never receives `DirectLocalRead` or a bounded-corridor
admission in this first scope. `ShellCommandTextDto` deliberately permits
pipelines, redirects, and compound commands, so WorkspaceRoot CWD does not make
its input a closed corridor constraint. It remains available only through an
exact user confirmation that displays the exact typed command and still applies
WorkspaceRoot CWD, explicit-path observation where applicable, mode, hook,
output, cancellation, and no-resume rules. A future typed command-template
direction would require a separately selected contract.

`fetch_url` never receives direct admission. It may use a bounded corridor only
when its descriptor declares a compatible typed constraint family and each
redirect remains within the selected descriptor-fixed restrictions. `mcp` never
receives direct admission: its corridor must name one selected connection,
method, schema, gateway revision, and supported typed input constraint family.
An MCP service cannot ask the user, create a confirmation, create a goal,
connection, run, child, message, or authority context.

An exact confirmation is one durable user decision bound to exactly one
`ToolCallId`, root tree, `ToolId`, descriptor revision, MCP-method reference
when present, typed input digest, applicable policy-snapshot digest, and all
selected limits. It cannot be replayed for another call, input, descendant
tree, revision, or later run.

A bounded confirmation is a user-approved
`ProgrammaticAuthorizationCorridorDto`:

```text
ProgrammaticAuthorizationCorridorDto
  root_session_id
  root_run_id
  root_origin
  effective_policy_snapshot_reference
  required_effect_selectors
  exact_tool_or_mcp_method_selectors
  descriptor_input_constraint_selections
  maximum_action_count
  maximum_concurrent_actions
  expires_at_run_terminal
  confirmation_reference
  canonical_corridor_digest
```

The corridor belongs to one active root tree and reaches every descendant in
that tree. A child may use only the same or a narrower exact selector and typed
constraint, and every action consumes the one shared remaining count and
concurrency allocation. No child can copy, widen, detach, retain, or apply a
corridor to a sibling root, a fork-created future root, another harness launch,
or a later run. A corridor expires on root terminalization, cancellation,
interruption, policy suspension or revocation, daemon restart, or its own
bounded terminal evidence. It is not a lasting policy revision.

`ask_user` remains a normal long-running registered tool rather than a policy
outcome. Only an `InteractiveUser` root may start it. When a descendant needs
an exact or bounded confirmation, the daemon records the descendant's safe
provenance but creates the typed question only for the root run. The child,
Python facade, role, harness, and MCP service never directly start `ask_user`.
A harness cannot await user interaction: its `sub_agent` use must fit the
user-approved corridor already selected by the harness rule, or it fails before
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
snapshot, corridor, confirmation, or audit evidence. A current live state may
nevertheless impose a stricter decision:

- `SuspendPolicy` blocks every not-yet-started matching call, cancels pending
  confirmations, releases their reservations, and permits no new reservation.
  It does not claim to roll back or silently stop an action that already reached
  `ToolCallStarted`.
- `ResumePolicy` may make a suspended policy active again only through an
  explicit user operation on its then-current active revision. It does not
  revive a cancelled run, expired confirmation, released reservation, or old
  corridor.
- `RevokePolicy` atomically creates a later immutable revision whose admission
  is disabled, enters `Revoked`, marks every active root tree that selected the
  policy for cancellation, and denies new reservations. Each affected tree
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
base revisions, safe rationale, and canonical digest. It may arise after the
selected goal milestones, a policy denial, or exhaustion of a policy limit.
An equal later proposal coalesces evidence into that one pending draft. The
draft is not a policy, card, target-snapshot input, corridor, confirmation,
tool selection, or authority. The daemon records it before the root-only user
question; the user may accept, edit and accept, or reject it. Acceptance checks
the exact base state and creates an immutable policy or policy revision.
Rejection changes no policy. No harness rule, policy, or policy revision exists
because a model merely proposed it.

## Run and calendar limits, reservations, and recovery

Every selected policy supplies both a per-run action limit and a per-run
concurrency limit. A user may select values no greater than 256 actions and 16
concurrent actions for one run; an effective intersection takes the smallest
non-zero bound. A policy also owns one calendar action limit from 1 through
4,096. The counter belongs to `ProgrammaticCallerPolicyId`, never to one
revision, scope projection, session, branch, child, root run, or corridor.

At initial policy creation, the user selects exactly one immutable calendar
period kind:

```text
ProgrammaticCalendarPeriodKindDto
  Day
  Week
  Month
```

A revision may narrow the numeric calendar limit but cannot change the period
kind. A different period requires a different policy identity. The current
calendar window retains the project time zone with which that window was first
opened. The next window reads the then-current project time zone. This does not
recalculate a current window after a project time-zone edit, while preserving
the project-calendar rule that nonexistent local times move forward to the
nearest valid local time and repeated local times occur once.

Before `ToolCallStarted`, the daemon atomically verifies every applicable
effective policy, all exact selectors and input constraints, run counters,
calendar counters, confirmation or corridor, and live policy state. It then
creates all required policy-counter reservations, the policy admission evidence,
and the tool admission outcome in one transaction, or writes none of them. No
provider, tool, shell, process, filesystem action with an external effect,
network call, kernel operation, MCP process, or child admission occurs in that
transaction.

An equal repeated operation reads the accepted idempotent binding and does not
reserve a second unit. A denial, invalid input, expiration, cancellation, or
known failure before `ToolCallStarted` releases each reservation atomically
with the terminal known pre-effect outcome. On `ToolCallStarted`, reservations
become permanent calendar and run consumption, even when later cancellation,
loss, or ambiguity yields `ExternalEffectUnknown`. Recovery records
`InterruptedBeforeStart` and releases the unstarted reservation atomically, or
records `ExternalEffectUnknown` for a started ambiguous action and never
retries it. It never recreates a reservation by running the action again.

The first policy scope uses these additional code-owned limits:

| Subject | Selected limit |
| --- | ---: |
| Policies in one project | 64 |
| Policies attached to one goal | 32 |
| Own or inherited policy references in one session | 32 |
| Rules in one policy revision | 32 |
| Typed input constraints in one rule or corridor selector | 16 |
| Unfinished corridors in one root tree | 16 |
| Awaiting policy confirmations in one root tree | 16 |
| Policy, revision, corridor, draft, or safe admission evidence | 512 KiB |
| Effective policy snapshot | 1 MiB |
| Pending draft per owner scope and record kind | 1, with evidence coalescing |

No reservation, counter, draft, provenance chain, or content is truncated or
partly committed to fit a bound. An unavailable or incompatible counter,
policy, snapshot, confirmation, or corridor blocks only the dependent action
before an external effect. It never falls back to the current policy, a current
default, a different counter, a live ancestor, or a fresh authorization.

## Run-selection compatibility and closed safe failures

The selected immutable execution meaning is first selected in
`run-execution-meaning-v3` for programmatic-caller policy and then extended by
the separately versioned `run-execution-meaning-v4` activity selection:

```text
ProgrammaticCallerPolicySelectionV1
  root_origin
  effective_policy_snapshot_reference
  policy_selection_digest
  inherited_scope_provenance
  fixed_run_limits
```

`RunExecutionMeaningDto.programmatic_caller_policy_selection` is `Disabled`
only for historical M4 and earlier post-M4 selection versions. Every new v3 or
v4 ordinary, goal-directed, verification, child, and harness run carries this
selection, including a selection that contains only the narrow interactive
direct-local-read baseline. `GoalRunSelectionV1`, `SubAgentDelegationSnapshotDto`,
`GoalDelegationSnapshotV1`, `ContinualHarnessSelectionV1`, and
`McpMethodCatalogSelectionV1` retain only safe typed references to it and to any
later admission evidence. No historical v1 or v2 selection, M4 snapshot, event,
`RunId`, replay, or `tool_execution_unavailable` result is rewritten or given a
synthetic policy record.

Live suspension, revocation, registry availability, counter state, and daemon
readiness remain outside immutable execution meaning. They may impose a
stricter present-time denial, but never rewrite historical semantics, reroute a
call, substitute a current policy snapshot, restore a corridor, or resume
external work.

At a minimum, the policy adds these closed safe failures through `ErrorDto`:

```text
programmatic_policy_limit_exceeded
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
programmatic_policy_corridor_unavailable
programmatic_policy_corridor_exhausted
programmatic_policy_input_constraint_mismatch
programmatic_policy_run_limit_exceeded
programmatic_policy_calendar_limit_exceeded
programmatic_policy_counter_unavailable
programmatic_policy_reservation_conflict
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
- The `Disabled` policy selection applies only to historical M4 and earlier
  post-M4 selection versions; it is never rewritten.
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
targets, coverage tiers, feature profiles, storage/wire schema, retention, and
bounds, then pass `make quick`, `make docs-check`, `make architecture`,
`make verify`, and Linux/Windows CI. Required evidence includes:

- root-origin and provenance fixtures with no third root and no raw-input
  leakage;
- policy scope/narrowing fixtures: intersection, most-restrictive-wins,
  child-narrowing-only, fork shared-calendar-counters, and
  inheritance-widening rejection;
- decision fixtures: `DirectLocalRead` baseline, exact confirmation
  single-binding, bounded corridor expiry, `execute`/`fetch_url`/`mcp`
  corridor constraints, and `ask_user` root-only;
- lifecycle fixtures: suspend/resume/revoke/archive, live-tightening, draft
  coalescing, and no-reactivation-after-revoke;
- reservation/calendar fixtures: atomic pre-start transaction, idempotent
  equal-replay, release-on-known-pre-effect, permanent-on-start, and
  `InterruptedBeforeStart`/`ExternalEffectUnknown` recovery;
- run-selection compatibility fixtures: v3/v4 selection, `Disabled` for
  historical M4, and no-current-state reconstruction;
- closed safe-failure and fake-secret regression across logs, errors,
  snapshots, events, and adapter DTOs.
