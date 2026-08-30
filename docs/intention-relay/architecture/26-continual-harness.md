# Continual Harness

## Status and scope

## Traceability

- Normative owner: architecture 26.
- Decision record: [`0021`](../decisions/0021-continual-harness-directions.md).
- Detail decision: [`0030`](../decisions/0030-continual-harness-safe-failures-and-selection-record-detail.md) (closed safe failures and selection-record detail).
- Reconciliation topics: `CHR-001..010`.
- Research provenance: [`m4plus_concept2.md`](../m4plus_concept2.md).
- Status: documentation-approved; implementation-authorized work requires a later activating specification under [Milestone 5+](11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment).

**Approved future architecture, documentation-only.** This document is the sole
detailed owner for the future continual-harness model: user-managed durable
rules, trigger capture, schedule and time semantics, delegated dossiers,
verified checkpoints, read-and-delegate execution classes, code-owned bounds,
and harness recovery. It does not authorize a crate, implementation, storage
migration, public protocol change, configuration schema, or delivery scope.

It applies to future fresh runs only. M3/M4 bytes, queue tickets, sessions,
runs, events, snapshots, replay, recovery, and `ToolCallRecorded ->
tool_execution_unavailable` retain their recorded ordinary semantics. A
continual harness is not a free-running autonomous agent, a persistent process,
or a second runtime authority; it is a user-managed set of durable rules, and
each accepted trigger may admit one new independent run in a separate
daemon-owned service session.

## Ownership and non-authorities

Architecture 13 owns Mandate lifecycle and fresh admission. Architecture 15
owns the registry, tool loop, and tool admission. Architecture 16 owns
scheduler readiness reevaluation; calendar/interval/time-zone semantics for
harness scheduling are owned here. Architecture 17 owns child creation and
verifier authority. Architecture 22 owns provider selection. Architecture 24
owns activity/UI projections. Architecture 27 owns programmatic-caller policy
and admission, including the corridor that gates harness `sub_agent` use.

The daemon remains the sole owner of rule definition, trigger capture,
admission, launch, durable memory, journal, publication, and recovery. A
harness rule, trigger, dossier, checkpoint, class, or bound cannot create a
`RunId` (except through the ordinary admission path), Mandate reason, lifecycle
transition, scheduler candidate, tool permission, registry slot, child edge,
verifier authority, MCP capability, bridge grant, kernel epoch, context
projection, branch, or reconciliation result. It is not a second runtime,
registry, scheduler, persistence authority, or sandbox.

## Rule model and lifecycle

A continual harness exists at exactly two scopes: a project and an ordinary
user session. Each rule owns a separate service session that does not share the
user's queued turns and preserves at most one active run.

```text
ContinualHarnessRuleDto
  harness_id
  scope
  active_revision
  lifecycle_state
  service_session_id

ContinualHarnessRevisionDto
  harness_id
  revision
  task
  class_reference
  source_references
  presentation_mode
  immutable_schedule_and_trigger_parts

HarnessTriggerReasonDto
  reason_id
  source_kind
  first_observed_at
  last_observed_at
  coalesced_count
  bounded_typed_references
  originating_rule_revision
  cause_chain_reference
```

A rule supports typed create, read, update-as-new-revision, pause, resume,
explicit launch, cancel-active-run, and archive operations. Updating a rule
creates a new immutable revision; a run already admitted under a revision keeps
that revision, while a coalesced but not-yet-admitted reason uses the newest
active revision.

Archiving a session-linked harness is pause with retention: the rule, journal,
pending coalesced reason, and verified checkpoint remain durable; no automatic
launch occurs while archived, and restoring the linked session never launches
work by itself. Archiving is rejected while the harness has an active run;
cancellation follows the ordinary two-step path first. Physical deletion,
export, and garbage collection remain outside this package.

## Sources and trigger capture

One rule may contain up to sixteen explicitly named sources. The closed source
kinds are:

1. explicit user launch;
2. project-time-zone calendar time;
3. a fixed equal interval;
4. a selected known terminal outcome of another harness or a selected ordinary
   session.

A completion link chooses its allowed known terminal outcomes explicitly and is
rejected if adding it would create a cause cycle. The first scope permits only
the known closed run outcomes; it does not react to partial output, provider
fragments, process signals, or unverified external effects.

Every trigger is durably captured before admission and has a stable reason
identity. Redelivery never creates a second run. While one launch from the same
rule remains non-terminal or while daemon-wide concurrency is full, later
reasons coalesce into at most one pending reason for that rule. The record
keeps the source kind, first and last observation, a coalesced count, bounded
typed references, and the cause-chain reference.

After daemon downtime or paused automation, at most one coalesced catch-up
reason is admitted; a burst of every missed slot is forbidden. A pause is
automation paused: automatic sources are captured and coalesced but do not
launch, while an explicit user launch remains allowed as a separate user-origin
operation.

## Schedule and time rules

An equal-interval source uses a fixed cadence from a durable anchor; the
minimum interval is one minute. A long run does not shift the schedule grid:
missed slots coalesce into one pending or catch-up reason.

Calendar rules use the project time zone. A non-archived rule follows a future
project time-zone change, while each revision and admitted reason records the
applied zone. Both a closed typed calendar form and a standard five-part
calendar expression are accepted, but they canonicalize to one record;
contradictory equivalent inputs are rejected before admission.

For daylight-saving transitions the daemon selects the nearest valid local
time: a nonexistent local time moves forward to the first valid time, and a
repeated local time fires once. A system-clock change never repeats an already
captured durable trigger.

## Dossier, memory, and result

An admitted launch receives a two-layer dossier. The first layer is the
immutable task from the active rule revision. The second layer is a fresh
bounded safe summary built at admission from the rule's explicit durable
sources, the direct trigger reason, and the latest verified checkpoint when
present.

A dossier excludes live conversation context, a full transcript, live kernel
namespace, unfinished operations, raw provider items, reasoning text,
credentials, grants, paths, and implementation resources. It may contain up to
sixteen unique explicit sources and up to sixty-four typed references. The
complete dossier is at most 512 KiB and is rejected rather than truncated.

A rule has a separate optional verified checkpoint, distinct from the
user-visible conclusion. It is typed, versioned, digest-protected, linked to its
producing run, and at most 512 KiB. A successful run may replace it only after
complete validation. Failure, cancellation, interruption, `ExternalEffectUnknown`,
or an oversized or invalid checkpoint retains the previous verified checkpoint;
an older checkpoint is never presented as the state of the current run.

The user-visible safe conclusion is also at most 512 KiB and is rejected rather
than truncated. Rule presentation selects either journal-only output or also a
compact safe entry in the linked activity. That entry is not a user message and
does not enter a later model request by itself.

## Execution boundary and classes

Every harness launch resolves a configured harness class that references one of
the selected `Light`, `Medium`, or `Heavy` classes by inherited narrowing. The
inherited base supplies the provider profile and existing step/model limits;
the harness class may only narrow tools, context/result limits, checkpoint
rules, and timing. It never weakens WorkspaceRoot, Plan/Build, hooks,
confirmation, redaction, admission, or `model_stream_progress_timeout_v1`.

The direct run and its whole descendant subtree are read-and-delegate only.
When the selected class permits them, the allowed registered tools are `read`,
`glob`, `grep`, `expand`, `retrieve`, and `sub_agent`. Direct write, edit,
process start, network retrieval, user interaction, and model-created rule
changes are outside this first scope. `sub_agent` is admitted only through the
user-confirmed typed corridor selected by that harness rule under
[architecture 27](27-programmatic-caller-policy-and-admission.md). The corridor
fixes the permitted class, tool subset, depth, child count, input constraints,
and all applicable limits; a launch creates a fresh run-bound use of that
selection and cannot widen it. A harness never calls `ask_user`, prepares a new
corridor, or receives a fallback authorization when the corridor is absent,
expired, suspended, revoked, exhausted, or incompatible.

## Bounds

The first scope uses these code-owned limits:

| Subject | Selected limit |
| --- | ---: |
| Harness rules in one daemon | 64 |
| Concurrent non-terminal work in one harness subtree | 16, including its sub-agents |
| Sources in one rule | 16 |
| Minimum equal interval | 1 minute |
| Dossier | 512 KiB |
| Verified checkpoint | 512 KiB |
| Safe conclusion | 512 KiB |
| Explicit sources in one dossier | 16 |
| Typed references in one dossier | 64 |
| Completion-cause chain depth | 8 |
| Direct successors of one terminal outcome | 16 |
| Total launches from one original cause | 256 |

A limit failure produces a known typed pre-effect rejection. Waiting for a free
concurrency slot retains the coalesced reason rather than dropping it. No
external provider, tool, kernel, process, network, or scheduler action occurs
inside a durable transition transaction.

## Selection record and closed safe failures

A launch admitted by the selected continual-harness model carries a separately
versioned credential-free `ContinualHarnessSelectionV1` nested record in
`run-execution-meaning-v4` `harness_selection` (see
[architecture 28](28-goal-domain-and-verification.md)). It contains the harness
identity, active rule revision, durable trigger reason, class resolution,
dossier digest, checkpoint reference, time-zone application, and immutable
bounds. Historical M4 and other non-harness runs do not acquire a synthetic
harness record.

The harness adds these closed safe failures through `ErrorDto`:

```text
harness_rule_limit_exceeded
harness_source_limit_exceeded
harness_concurrency_limit_exceeded
harness_interval_too_short
harness_schedule_invalid
harness_trigger_cycle
harness_dossier_too_large
harness_source_unavailable
harness_checkpoint_too_large
harness_checkpoint_unavailable
harness_result_too_large
harness_not_active
harness_archived
harness_revision_conflict
harness_cause_chain_limit_exceeded
```

They disclose no credential, path, dossier content, Python value, grant,
provider resource, process topology, or raw transcript. Every listed failure is
known before an external effect; unknown-effect evidence is retained for work
that had already started.

## Cancellation, recovery, and publication

Run cancellation uses the existing two-step lifecycle and cascades through the
descendant subtree. Daemon restart marks an unfinished harness run
`Interrupted`; no provider request, tool, process, kernel, child agent, bridge
operation, or external action resumes, retries, or reruns. A later attempt is a
separately admitted launch with new identities and consumes new capacity.

The harness journal is durable, versioned, and readable after recovery.
Linked-activity output is published only after durable commit and an
independent reread. Historical M4 data remains byte-for-byte unchanged and
acquires no synthetic harness facts.

## Compatibility and historical preservation

- M3/M4 queue tickets, sessions, runs, events, snapshots, replay, and recovery
  remain authoritative and unchanged; no harness rule becomes a queue ticket or
  Mandate reason.
- Historical M4 and retained records gain no synthetic harness state.
- Harness limits are intrinsic/capacity/product-classified in the activating
  specification and never become Mandate admission quotas.
- All directions affect fresh runs only, activated under Milestone 5+.

## Dependencies and non-goals

This document depends on architectures 13, 15, 16, 22, 24, and 27 plus decisions
0001, 0008, 0014, 0021, and 0022. It does not define bounded autonomous
continuation or an autonomous harness goal mode; work, continuation, or requeue
after client disconnection; attachments, images, binary, rich-MIME, or
multimodal payloads; a plug-in, extension, skill/MCP installation, or dynamic
tool registration system; administration of long-lived processes, workers,
leases, attach/detach, force-kill, or supervisor recovery; physical deletion,
export, garbage collection, or destructive history cleanup. Each requires a
separate future decision.

A later activating specification must declare exact crates, dependencies, test
targets, coverage tiers, feature profiles, storage/wire schema, retention, and
bounds, then pass `make quick`, `make docs-check`, `make architecture`,
`make verify`, and Linux/Windows CI. Required evidence includes:

- rule lifecycle, revision immutability, pause/resume/archive, and
  archive-rejected-while-active fixtures;
- trigger capture, coalescing, catch-up, redelivery-no-second-run, and
  cause-cycle rejection fixtures;
- schedule/time fixtures: minimum interval, calendar time-zone, DST
  nearest-valid-local-time, clock-change no-repeat;
- dossier/checkpoint/conclusion bound and rejection fixtures with redaction;
- read-and-delegate execution-boundary and corridor admission fixtures;
- limit/concurrency/chain-depth fixtures with retained-reason waiting;
- cancellation cascade, restart `Interrupted`, no-resume, and post-commit
  reread publication fixtures;
- M3/M4 byte/meaning/replay/recovery preservation and fake-secret regression
  across logs, errors, snapshots, events, and adapter DTOs.
