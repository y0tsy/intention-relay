# Goal Domain and Verification

**Approved future design. Not implemented; activation requires an activating specification.** Architecture 28 owns the
future Goal aggregate domain: Goal identity, scope, and links; Goal status; leading-goal run selection; verification
evidence; roles; model proposals and user confirmation; the conversation-compaction working form; and Goal-domain
bounds and closed safe failures.

Owner: architecture 28.

## Ownership and non-authorities

The removed run-execution meaning layer (its canonical codec was removed under [architecture
02](02-dto-and-contract-policy.md)) leaves no live path. Architecture 15 owns tool identity and the tool loop, architecture 18
MCP connection records, architecture 21 context selection and projection (Goal context selection, Skills, and compaction),
and architecture 03 activity-journal and notification projections.

This document owns the Goal aggregate domain and its verification semantics. A Goal, revision, link, role, proposal, or
summary creates no `RunId` except through the ordinary admission path, lifecycle transition, tool permission, tool
selection, child, MCP connection, kernel epoch, branch, or reconciliation result, and is not a second runtime,
registry, persistence authority, or sandbox.

## Goal identity, scope, and links

The daemon assigns `GoalId`, `GoalRevisionId`, and `GoalLinkId`; one immutable scope belongs to each Goal. A project
Goal enters a session only through an explicit durable link, never by implication; a session Goal belongs only to its
one session.

```text
GoalScopeDto
  Project { project_id }
  Session { project_id, session_id }

GoalDto
  goal_id
  scope
  active_revision
  status
  accepted
  parent_goal_id_when_present

GoalRevisionDto
  goal_id
  revision
  title
  objective
  local_rule_references
  revision_identity

GoalSessionLinkDto
  project_goal_id
  session_id
  effective_from_revision
  link_identity
```

Goals are flat: `parent_goal_id_when_present` is a display-only reference to another Goal, and it grants no
inheritance, readiness, requirement, or bound in either direction. Self-link and cycles fail before partial record. A
revision is credential-free, typed, bounded, and immutable; edits create new revisions and never rewrite admitted-run
meaning.

## Goal status

```text
GoalStatusDto
  Active
  Done
  Stopped
```

A Goal holds exactly one status. `Active` is the only status that admits a new goal-directed run; `Done` and `Stopped`
are terminal for new runs. `accepted` is an independent boolean recording explicit user acceptance of the exact active
revision's evidenced outcome, and a new active revision resets it. Stop prevents new ordinary and verification runs,
interrupts active runs under the cooperative signal, and commits a bounded `Partial` result with its notice for a
started external effect that cannot be proven terminal.

## Leading-goal run selection and durable transactions

A run is ordinary (no leading Goal) or goal-directed (exactly one leading Goal), never multiple coequal Goals after
admission. A verification run has that one Goal with kind `VerificationOnly`; it may attach verification evidence but
cannot alter Goal, Skill, role, or connection state.

A run's immutable selection is a typed serde JSON record rather than one canonical execution-meaning envelope; the
former `run-execution-meaning-v4` carrier was removed with the canonical codec under [architecture
02](02-dto-and-contract-policy.md). The applicable record is `GoalRunSelectionV1`; its absence means the run is ordinary,
and historical M4 records gain nothing.

`GoalRunSelectionV1` contains the leading `GoalId`, its exact revision, the session-link provenance, run kind, the
target-snapshot reference, immutable bounds, and the one frozen `SkillSelectionV1`. It holds no full Skill/role/summary,
credential, provider value, current machine state, raw transcript, process resource, or handle.

The admission transaction validates the Goal and session link, the complete target snapshot, all references and bounds,
provider selection, tool-selection revision, and applicable policy, then commits the run, selection, and audit evidence
in one transaction; no external action occurs inside it. An active run retains its frozen selection, and edits create
newer revisions for future admission only. Unknown, corrupt, unavailable, incompatible, or over-limit selected records
block dependent operations before external work; retry, child admission, fork, and recovery never substitute current
state. MCP work is admitted only through the run's frozen architecture-15 tool selection: `mcp` is unavailable when
absent from it, and every `mcp` call is an ordinary frozen-selection tool call recorded under architecture 15's tool
loop.

## Verification evidence

Verification evidence is one exact durable reference — a terminal run result or an accepted user declaration —
validated against the exact goal revision it attests. There is no gate DTO, no template, no `ExecutableGate`, no
create/edit/archive/restore surface, and no template proposal. Evidence is collected in the terminal verification
phase of an ordinary leading-goal run or by a separately user-admitted `VerificationOnly` run; failure, timeout,
cancellation, interruption, an output bound, a missing reference, or a stale revision are known typed outcomes, there
is no hidden retry, retries occur only before the first irreversible durable fact, and a started external action is
never repeated.

## Skills and roles

A Skill is a bounded textual procedure with typed references to roles and registered capabilities; no binary,
executable, install, plug-in, dynamic registration, arbitrary MCP schema, or hidden external action. A Role is a named
reusable `sub_agent` template (task, permitted class, tool subset, contextual limits, typed references); a concrete use
may only narrow task, class, context/result limits, and tool subset and cannot add a tool, increase a class, widen
scope, change a provider, or weaken policy. Skill and role cards are discovered first; full records require explicit
disclosure.

## Model proposals and user confirmation

After one of these durable Goal milestones the model may prepare at most one coalesced proposal of a particular owner
scope and record kind: user acceptance or stop of a goal, or recording of a verification evidence reference.
`RefinementDraftDto` contains a daemon-assigned identity, selected source
run/Goal and milestone, exact base revisions, bounded typed edit set, evidence references, safe rationale, and a typed
record identity. It is not a current record, does not appear in cards or model context, cannot be used by a
child/Skill/role/MCP method, and grants no execution authority. A later equal proposal adds evidence to the
one pending draft rather than producing an unbounded queue. The daemon records the proposal durably before asking the
user; via the normal `ask_user` path the user may accept, edit-and-accept, or reject, and the draft remains pending
until an explicit decision. Acceptance validates the exact base revision and creates a new immutable record revision; a
stale base is a typed conflict, and rejection changes no active record.

## Conversation compaction and progressive context disclosure

Compaction is a versioned model-context projection, not a replacement for the source transcript, model facts, tool
facts, reasoning, kernel checkpoint, child dossier, or Goal evidence. `ConversationSummaryDto`
covers one continuous completed durable-history range and contains schema/contract version, start/end references, a
previous-summary reference when present, bounded safe content, identity, and provenance; original facts remain readable.
The working form is one cumulative current summary plus the later uncompacted suffix. A new revision made inside an
already active run uses the previous selected summary plus the next bounded completed source range and becomes part of
the next model step in the same run; it executes no registered tool, creates no service run, and cannot run before a
user-admitted run, after terminalization, or after restart. A user or model may request it in an active run; if the
selected model-context bound would be exceeded, the daemon requires it in that same run rather than silently creating
another. Correcting a summary creates a separately immutable later revision with exact source and predecessor
references, and the earlier summary remains historical evidence. Fork creation stores only exact compatible selected
summary references in the frozen projection and never reads a current ancestor or imports a future summary. Headroom,
VFR, and `retrieve` retain their own semantics and never turn source content into summary text implicitly.

## Branches, children, interruption, and recovery

A new session fork receives a frozen copy of applicable project-Goal links, session Skill/role cards, exact revisions,
and the selected current summary reference in an independent fork snapshot. It does not receive a new session Goal,
active Goal run, kernel namespace, current ancestor record, local MCP process, external connection resource, or future
source change; the user explicitly creates any new session Goal in the branch.

An admitted child agent receives a frozen Goal context: parent task, leading-Goal identity and revision, effective
required constraints, applicable cards, selected role when any, and only the required safe references; not a full parent
transcript or live target context. Later
parent/Goal/Skill/role/activity edits do not change the child snapshot. A child uses independently assigned
session/run/class/tool/provider selections and its own interruption and no-resume rules. An interruption stops
only the in-flight call: it commits a bounded `Partial` result with its notice and the child continues with its next
model step.
On failure or daemon restart, no provider request, tool, MCP request, process, kernel action, child, or external effect
is retried, reattached, resumed, or rerun; unfinished work becomes `Interrupted` with already selected known pre-effect
evidence or a bounded partial result. Goals, summaries, proposals, and readable history survive recovery; a later user
attempt obtains a new run identity and target snapshot.

## Bounds and closed safe failures

The first scope uses these code-owned limits:

| Subject | Selected limit |
| --- | ---: |
| Goals in one project | 256 |
| Goals in one session | 64 |
| Goal-tree depth | 16 |
| Direct children of one goal | 32 |
| Session links of one project goal | 64 |
| Selected skills and roles in one run | 32 |
| Full skill, role, summary, or draft | 512 KiB |
| Canonical goal target snapshot | 1 MiB |
| Goal context and summaries in one run | 4 MiB |
| Pending proposal per owner scope and record kind | 1, with evidence coalescing |

No content is truncated or partly committed to satisfy a bound. The Goal-domain closed safe failures through `ErrorDto`
are:

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

They disclose no credential, path, raw external response, private process resource, full Skill/role content, model
proposal text, provider resource, or implementation detail.

## Compatibility and historical preservation

No historical record gains synthetic Goal, evidence, proposal, or compaction state. Goals are acceptance/evidence
records, not a work-authorization plane. All directions affect fresh runs only.

## Dependencies and non-goals

This document depends on architectures 03, 15, 18, and 21. Non-goals: Goal persistence,
search/index/vector retrieval, prompt assembly, SQL/wire tags, migrations, retention/deletion/encryption, source-page
sizes, resource values, provider evolution, session branching, activity-journal and UI delivery, Plan artifacts, MCP
administration, Python/Jupyter process behavior, Cargo, Makefile/CI, production activation, attachments/images/binary/
rich-MIME input, dynamic extensions and installation, dynamic tool registration, physical deletion, and
long-lived-worker administration (leases, attach/detach, force-kill, supervisor recovery). Work/continuation/requeue
after client disconnection is an accepted post-M5 future direction under an explicit durable contract that never
silently resumes old external work.

Evidence: activating specification per [architecture 12](12-quality-gates-and-makefile.md).
