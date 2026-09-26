# ADR 0044: M5+ Slice 3 harness activation

## Status

Accepted as the Slice 3 activating specification required by
[ADR 0035](0035-m5plus-complete-foundation-activation.md). It activates Slice 3
(Harness) of ADR 0035 and is subordinate to ADR 0035: ADR 0035 remains the
activation home and slice-sequence authority, and this record activates only
the harness slice, not slices 4-5, M6, M7, M8, or M9.

## Scope and supersession

This record freezes and activates only M5+ Slice 3, Harness: the continual
harness, programmatic-caller policy and admission, and the Goal domain with its
verification semantics (architectures 26/27/28, ADR 0021/0022/0023/0030/0031/
0033). In scope is the full Slice 3 deliver line: durable harness rules,
revisions, and triggers; dossiers and verified checkpoints; read-and-delegate
execution classes and code-owned bounds; the 15 closed `harness_*` safe
failures; the two closed root origins; policy identity, scope, and
narrowing-only inheritance, the four admission decisions, exact and bounded
confirmations, corridors, lifecycle, drafts, and per-run and calendar limits
with atomic reservations; the Goal tree, lifecycle, readiness, and user
decision; leading-goal run selection; delegated Verification Mandates; gates
and evidence; working memory, Skills, roles, and templates; model proposals
and user confirmation; conversation compaction; and the closed Goal-domain
safe failures. The activation also wires the Slice 3 contract records the
ledger reservations name: the tool-descriptor, tool-registry, and
model-tool-loop revisions (architecture 15, ADR 0025) and the
bridge-invocation and MCP-method-catalog selections (architectures 18/19, ADR
0027), and it delivers the harness-side accepted directions of
[ADR 0033](0033-accepted-m5plus-execution-directions.md): autonomous harness
goal mode and the post-disconnect work/requeue contract.

Out of scope: M6-M9 boundary behavior; the Mandate-track runtime beyond the
declared verifier authority boundary (Mandate-scoped authority transactions
stay Milestone 11); Build-mode Mandate continuation and its evidence EVD-027
(architecture 13, Milestone 10); Milestone 12 context projection and the MCP
and kernel implementations; Slice 4 exports, deletion/GC, clone/rebind, and
activity classification; the Slice 5 instruction channel; and TUI/REPL
implementation. Any second runtime, registry, scheduler, persistence
authority, or sandbox; remote continuation; a durable autonomous actor; and
any M3/M4 rewriting are out of scope. M3/M4 startup-only configuration,
recorded revisions, persisted run snapshots, queue tickets, sessions, runs,
events, and bytes remain authoritative and unchanged. Historical runs receive
no synthetic post-M5 records.

## Version ledger

| Contract | Version/status |
| --- | --- |
| Local protocol | 1.1, unchanged |
| Public DTO schema | 1.1, additive, unchanged |
| TOML configuration schema | 1, single shape; unversioned documents fail closed ([ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md)) |
| SQLite storage schema | Logical version 1: single live schema created directly on open; no migration chain and no version gate ([ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md)) |

No new negotiated capability is introduced: the effective capability set
remains the hello intersection of the Slice 1 and Slice 2 tokens, and every
Slice 3 selection is carried inside the existing `run-execution-meaning-v4`
record and the existing daemon facade. No new crate, dependency, feature
profile, coverage tier, or CI job is introduced.

## Negotiated capabilities and failure semantics

The Slice 3 surface adds no runtime capability token. The Slice 3 selections
are part of the single live execution-meaning record and are non-authorizing by
themselves: a harness rule, trigger, dossier, checkpoint, class, bound, policy,
revision, corridor, confirmation, counter, reservation, draft, Goal, gate,
memory card, Skill, role, template, proposal, or summary cannot create a
`RunId` except through the ordinary admission path, and cannot create a
Mandate reason, lifecycle transition, scheduler candidate, tool permission,
registry slot, child edge, verifier authority, MCP capability, bridge grant,
kernel epoch, context projection, branch, or reconciliation result. A harness
is a user-managed set of durable rules, not a free-running agent, a persistent
process, or a second runtime authority; every accepted trigger admits at most
one new independent run in a separate daemon-owned service session.

The Slice 3 failure semantics are closed. Dependent work fails before effect
with the three closed families below; no content is truncated or partly
committed, and no external provider, tool, kernel, process, network, or
scheduler action occurs in the failing transition.

The continual-harness family adds these 15 closed safe failures through
`ErrorDto` (architecture 26):

| Family | Error codes |
| --- | --- |
| Harness rules, sources, and schedules | `harness_rule_limit_exceeded`, `harness_source_limit_exceeded`, `harness_source_unavailable`, `harness_interval_too_short`, `harness_schedule_invalid` |
| Harness capacity and content bounds | `harness_concurrency_limit_exceeded`, `harness_dossier_too_large`, `harness_checkpoint_too_large`, `harness_checkpoint_unavailable`, `harness_result_too_large` |
| Harness lifecycle and cause graph | `harness_trigger_cycle`, `harness_not_active`, `harness_archived`, `harness_revision_conflict`, `harness_cause_chain_limit_exceeded` |

The programmatic-caller policy family adds these 22 closed safe failures
through `ErrorDto` (architecture 27):

| Family | Error codes |
| --- | --- |
| Policy identity, snapshots, and inheritance | `programmatic_policy_limit_exceeded`, `programmatic_policy_snapshot_too_large`, `programmatic_policy_snapshot_unavailable`, `programmatic_policy_revision_conflict`, `programmatic_policy_inheritance_widening_forbidden`, `programmatic_policy_origin_invalid`, `programmatic_policy_not_applicable` |
| Policy lifecycle, confirmations, and corridors | `programmatic_policy_suspended`, `programmatic_policy_revoked`, `programmatic_policy_confirmation_required`, `programmatic_policy_confirmation_expired`, `programmatic_policy_root_only_interaction`, `programmatic_policy_corridor_unavailable`, `programmatic_policy_corridor_exhausted`, `programmatic_policy_input_constraint_mismatch` |
| Policy limits, counters, and reservations | `programmatic_policy_run_limit_exceeded`, `programmatic_policy_calendar_limit_exceeded`, `programmatic_policy_counter_unavailable`, `programmatic_policy_reservation_conflict` |
| Harness delegation and drafts | `programmatic_policy_harness_delegation_forbidden`, `programmatic_policy_draft_conflict`, `programmatic_policy_draft_too_large` |

The Goal-domain family adds this closed safe failure set through `ErrorDto`
(architecture 28, "Bounds and closed safe failures"; 15 `goal_*` codes plus the
memory, Skill, delegation, compaction, and refinement draft codes of the same
section):

| Family | Error codes |
| --- | --- |
| Goal identity, tree, and lifecycle | `goal_limit_exceeded`, `goal_tree_depth_limit_exceeded`, `goal_child_limit_exceeded`, `goal_session_link_limit_exceeded`, `goal_cycle_detected`, `goal_not_active`, `goal_revision_conflict`, `goal_archive_not_terminal` |
| Goal snapshots, gates, and acceptance | `goal_snapshot_too_large`, `goal_snapshot_unavailable`, `goal_gate_limit_exceeded`, `goal_gate_unavailable`, `goal_gate_failed`, `goal_not_ready`, `goal_acceptance_exception_invalid` |
| Memory, Skill, role, and compaction | `memory_entry_limit_exceeded`, `memory_entry_too_large`, `memory_reference_unavailable`, `memory_replacement_conflict`, `skill_entry_too_large`, `skill_reference_unavailable`, `delegation_role_invalid`, `delegation_role_widening_forbidden`, `compaction_summary_too_large`, `compaction_summary_unavailable`, `compaction_history_unavailable` |
| Proposals | `refinement_draft_conflict`, `refinement_draft_too_large` |

Every listed failure is a typed known pre-effect rejection that discloses no
credential, path, dossier content, policy body, raw input, Python value, grant,
provider resource, process topology, raw transcript, or implementation detail.
Unknown-effect evidence is retained for work that had already started; a later
cancellation or recovery preserves the independently selected
`ExternalEffectUnknown` evidence and never re-runs, retries, reattaches, or
resumes the action. Exact confirmations are single-binding and never replay;
bounded corridors expire on root terminalization, cancellation, interruption,
policy suspension or revocation, daemon restart, or their own bounded terminal
evidence. The harness-side post-disconnect contract adds no new error code: an
unfinished harness run becomes `Interrupted` on restart, and a later attempt is
a separately admitted launch with new identities and new capacity, never a
silent resumption of old external work.

## Canonical codec and identity

`intention-domain` owns the canonical codec and semantic canonical records.
The format is `IRCR` / `typed-tlv-v1` / SHA-256, unchanged from Slice 1.
Digest text is `<namespace>:sha256:<64 lowercase hex>`; new identities are
`sha256-v1:<64 hex>`. A digest excludes its own field. Digest inputs exclude
credentials, paths, display data, readiness, and current state. Every Slice 3
record is credential-free, typed, versioned, and immutable; records carry safe
identity, revision, digest, and bound values only, never raw content,
credentials, paths, grants, provider resources, process handles, or
implementation state. Historical M4 and non-harness runs acquire no synthetic
record.

The Slice 3 canonical selection families derive from
`crates/intention-domain/src/slice3_selections.rs`: the goal-run selection
(`0x0203`), the continual-harness selection (`0x0204`), the MCP method catalog
selection (`0x0205`), and the nested typed `ProgrammaticCallerRootOriginV1` of
the programmatic-caller policy selection (`0x0201`). The reworked `0x0201`
outer table derives from
`crates/intention-domain/src/run_execution_meaning.rs`; the frozen public DTO
families (`0x0301`-`0x0304`) derive from
`crates/intention-protocol/src/contract_families.rs`.

### run-execution-meaning-v4 slot map

| Slot | Selection | Slice 3 status |
| --- | --- | --- |
| 1 | resolved provider selection | Unchanged M3/M4 field |
| 2 | model capability set | Unchanged M3/M4 field |
| 3 | context projection selection | Unchanged M3/M4 field |
| 4 | tool execution selection | Unchanged M3/M4 field |
| 5 | reasoning history manifest reference | Unchanged M3/M4 field |
| 6 | terminal provenance references | Unchanged M3/M4 field |
| 7 | continual-harness selection (`0x0204`) | Typed `DisabledOr<ContinualHarnessSelectionV1>`; `Disabled` for runs not admitted by a harness |
| 8 | goal-run selection (`0x0203`) | Typed `DisabledOr<GoalRunSelectionV1>`; `Disabled` for runs without a leading Goal |
| 9 | MCP method catalog selection (`0x0205`) | Typed `DisabledOr<McpMethodCatalogSelectionV1>`; `Disabled` when `mcp` is absent from the frozen model-tool selection |
| 10 | programmatic-caller policy selection (`0x0201`) | Typed `DisabledOr<ProgrammaticCallerPolicySelectionV1>`; every new v4 ordinary, goal-directed, verification, child, and harness run carries a selection |
| 11 | agent activity selection (`0x0202`) | Unchanged from Slice 1 |

The `DisabledOr` wrapper is the one-byte closed/open presence marker (`0` with
no nested bytes, or `1` followed by the nested record's canonical bytes); a
malformed marker or a noncanonical nested body fails closed. Slots 1-6 and 11
keep their Slice 1/M3-M4 meaning and byte layout.

## Numeric tag registry

`intention-domain` owns this registry. The `TagStatus` enum has exactly the
variants `Wired`, `ReservedForSlice3`, and `ReservedForSlice4`. After Slice 3
the ledger holds 24 tags: 16 `Wired`, 0 `ReservedForSlice3`, and 8
`ReservedForSlice4`. Slice 3 newly wires `0x0203`-`0x0205` and
`0x0301`-`0x0304`, and reworks field 1 of the already-wired `0x0201`.

| Tag | Value | Status |
| --- | --- | --- |
| `run-execution-meaning` | `0x0101` | Wired |
| `programmatic-caller-policy-selection-v1` | `0x0201` | Wired (Slice 3 field-1 rework) |
| `agent-activity-selection-v1` | `0x0202` | Wired |
| `goal-run-selection-v1` | `0x0203` | Wired (Slice 3) |
| `continual-harness-selection-v1` | `0x0204` | Wired (Slice 3) |
| `mcp-method-catalog-selection-v1` | `0x0205` | Wired (Slice 3) |
| `model-capability-taxonomy-v1` | `0x0206` | Wired |
| `provider-profile-revision-v1` | `0x0207` | Wired |
| `provider-selection-v1` | `0x0208` | Wired |
| `reasoning-history-manifest-v1` | `0x0209` | Wired |
| `context-source-manifest-v1` | `0x020A` | Wired |
| `model-context-projection-v1` | `0x020B` | Wired |
| `tool-descriptor-revision` | `0x0301` | Wired (Slice 3) |
| `tool-registry-revision` | `0x0302` | Wired (Slice 3) |
| `model-tool-loop-v1` | `0x0303` | Wired (Slice 3) |
| `bridge-invocation-v1` | `0x0304` | Wired (Slice 3) |
| `fork-base-snapshot-v1/v2` | `0x0401` | ReservedForSlice4 |
| `fork-preview-v1/v2` | `0x0402` | ReservedForSlice4 |
| `fork-command-v1` | `0x0403` | ReservedForSlice4 |
| `agent-activity-tree-v1` | `0x0501` | ReservedForSlice4 |
| `agent-activity-pair-v1` | `0x0502` | ReservedForSlice4 |
| `agent-message-v1` | `0x0503` | ReservedForSlice4 |
| `agent-activity-journal-record-v1` | `0x0504` | ReservedForSlice4 |
| `agent-notification-record-v1` | `0x0505` | ReservedForSlice4 |

The `0x0301`-`0x0304` tags are frozen public wire families: their DTO field
tables are fixed by the Slice 1 ledger and hermetic DTO contract evidence
exists, while their behavior stays with architectures 15/19 and is delivered
by Milestones 10-11, not by this activation. Reserved tags are not active
capabilities and are not model-visible.

## Ownership and preservation invariants

Crate ownership for the Slice 3 surface is fixed:

| Surface | Owner |
| --- | --- |
| Canonical Slice 3 selection records, tags, digests, and validation | `intention-domain` (`slice3_selections.rs`, `harness.rs`, `programmatic_policy.rs`, `goal_domain.rs`, `verification.rs`) |
| Wire families, negotiation, and typed commands/queries | `intention-protocol` |
| Current-schema DDL, projections, and durable harness, policy, and Goal rows | `intention-storage` and `intention-storage-sqlite` (`harness_repo.rs`, `programmatic_policy_repo.rs`, `goal_repo.rs`) |
| Harness runtime, policy admission, and Goal runtime services | `intention-application` (`harness.rs`, `programmatic_policy.rs`, `goal_domain.rs`) |
| Hosting, recovery, and no-resume | `intention-daemon` |
| Typed client surface | `intention-client` |
| Composition and facade assembly | `intention` |
| Presentation adapters | TUI/Tauri adapters |

No new crate, dependency, feature, coverage tier, or exclusion is introduced.
Skeleton `intention-headroom`, `intention-plans`, `intention-vfr`, and
`intention-tauri` remain untouched, and M6-M9 behavior is untouched. M3/M4
config revisions, snapshots, sessions, runs, events, cursors, queue tickets,
replay, recovery, and bytes remain authoritative and unchanged, and
`ToolCallRecorded -> tool_execution_unavailable` keeps its recorded ordinary
semantics. Historical
runs receive no synthetic harness, policy, Goal, gate, memory, proposal,
compaction, or verifier record; current state is never reconstructed. The
storage schema is the single live schema (logical version 1) created directly
on open under
[ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md): no migration
chain, no `user_version` gate, and no synthetic post-M5 record added to
historical runs. The legacy M4 selection bridge (tag
`legacy-m4-selection-binding` 0x020C) remains removed by
[ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md). Harness
limits are intrinsic/capacity/product-classified and never become Mandate
admission quotas or child-graph limits. A harness rule, policy, or Goal cannot
become a queue ticket or a Mandate reason.

## Evidence and non-goals

Required evidence covers the Slice 3 directions with per-direction test
anchors:

| Direction | Evidence anchor (test target) |
| --- | --- |
| Slice 3 canonical selections, tag registry, digests, bounds, and rejection semantics | `crates/intention-domain/tests/m5_slice3_canonical.rs`; updated `crates/intention-domain/tests/m5_control_plane_canonical.rs` |
| Continual-harness domain (rules, revisions, triggers, coalescing, catch-up, schedule/time, class resolution, bounds, autonomous harness goal mode, post-disconnect contract) | `crates/intention-domain/tests/m5_harness_domain.rs` |
| Programmatic-caller policy domain (two roots, provenance, scope, narrowing, decisions, confirmations, corridors, lifecycle, drafts, limits) | `crates/intention-domain/tests/m5_policy_domain.rs` |
| Goal domain (identity, scope, tree, lifecycle, readiness, gates, memory, Skills, roles, proposals, compaction, bounds) | `crates/intention-domain/tests/m5_goal_domain.rs` |
| Unified verification records (authority, audit baseline, evidence, verdict, operation set, stale-baseline fail-closed) | `crates/intention-domain/tests/m5_verification_domain.rs` |
| Frozen public wire families, tag parity, and full activation wiring | `crates/intention-protocol/tests/m5_slice3_wire_contracts.rs` |
| Durable Slice 3 repository DTO contracts and single current schema | `crates/intention-storage/tests/m5_slice3_contracts.rs`; `crates/intention-storage/tests/m5_slice3_goal_contracts.rs`; `crates/intention-storage-sqlite/tests/m5_slice3_repos.rs`; updated `crates/intention-storage-sqlite/tests/sqlite_contracts.rs` |
| Harness runtime (trigger capture, admission, cancellation cascade, restart `Interrupted`, publication) | `crates/intention-application/tests/m5_harness_runtime.rs` |
| Policy admission (atomic pre-start transaction, idempotent replay, reservations, recovery dispositions) | `crates/intention-application/tests/m5_policy_admission.rs` |
| Goal runtime (leading-goal selection, gates, verification runs, no-current-state reconstruction) | `crates/intention-application/tests/m5_goal_runtime.rs` |
| Daemon recovery and no-resume | `crates/intention-daemon/tests/m5_slice3_recovery.rs` |
| Typed client surface | `crates/intention-client/tests/m5_slice3_client.rs` |

Required gates are `make quick`, `make verify`, `docs-check`, and Linux/Windows
CI. M3/M4 byte, event, snapshot, replay, and recovery preservation and the
fake-secret regression across logs, errors, snapshots, events, and adapter DTOs
are part of the required evidence.

This ledger does not implement M6-M9 boundary behavior; it does not implement
the Mandate-track runtime beyond the declared verifier authority boundary
(Mandate-scoped authority transactions stay Milestone 11); it does not
implement Build-mode Mandate continuation (EVD-027, architecture 13, Milestone
10); it does not implement Milestone 12 context projection or the MCP and
kernel implementations; it does not implement Slice 4 exports, deletion/GC,
clone/rebind, or activity classification; it does not implement the Slice 5
instruction channel; and it does not implement TUI/REPL behavior. It does not
introduce a second runtime, registry, scheduler, persistence authority, or
sandbox, and it does not bump a protocol, DTO schema, configuration, or
storage-schema version. It adds no new negotiated capability, crate,
dependency, feature profile, coverage tier, or CI job.

## Resolution notes

### Runtime version resolution

Protocol 1.1 and public DTO schema 1.1 remain the active advertised versions;
`intention-protocol` continues to define `CURRENT_PROTOCOL_VERSION` and
`CURRENT_DTO_SCHEMA_VERSION`, and the runtime crates reference them. TOML
configuration schema remains 1. SQLite storage remains the single live schema
(logical version 1) created directly on open: the Slice 3 table families
listed in Appendix B are part of that schema, and no migration chain or
`user_version` gate is reintroduced. No negotiated capability token is added or
removed; the Slice 3 selections ride the existing v4 execution-meaning record
and the existing daemon facade. No new crate, dependency, feature profile,
coverage tier, or CI job is introduced.

### Full activation rather than contract-only

The roadmap Slice 3 exit criteria require the tool-descriptor,
tool-registry, and model-tool-loop revisions and the bridge-invocation and
MCP-method-catalog contract records the ledger reservations name, with their
implementations delivered later by the Mandate-track milestones. This record
goes further, as approved: it fully activates the harness, programmatic-caller
policy, and Goal-domain selections, semantics, repositories, services, and
recovery behavior, not only the reserved contract records. The `0x0301`-
`0x0304` wire families remain frozen contract records; their hermetic DTO
contract evidence exists, and their behavior stays with architectures 15/19 in
Milestones 10-11.

### Root-origin rework and typed v4 slots

`ProgrammaticCallerPolicySelectionV1.root_origin` (`0x0201` field 1) becomes
the typed closed nested record `ProgrammaticCallerRootOriginV1`, with exactly
two variants: `InteractiveUser { originating_turn_id }` (nested version 1),
the root of an ordinary user-admitted run and all of its descendants, and
`ContinualHarness { harness_id, rule_revision, trigger_reason_id }` (nested
version 2), the root of one separately admitted harness launch and
descendants. No protocol peer, detached Python task, child agent, MCP service,
provider, bridge channel, queue item, replay, or daemon recovery becomes an
independent root. The Slice 1 `ExecutionKind` U64 shape of field 1 is removed;
under the single-version policy there is no dual decode, and the former shape
fails closed. `AgentActivitySelectionV1.root_origin` (`0x0202`) stays
`ExecutionKind` (Ordinary/Mandate/VerifierMandate) and is unchanged.

v4 slots 7-10 become typed `DisabledOr` selections. Slot 7 is `Selected` only
for a run admitted by a continual-harness launch, slot 8 only for a run with
exactly one leading Goal (including a `VerificationOnly` run), and slot 9 only
when `mcp` is present in the frozen model-tool selection. Slot 10 is `Selected`
for every new v4 ordinary, goal-directed, verification, child, and harness run,
including a selection that contains only the narrow interactive
direct-local-read baseline; the `Disabled` form exists only as the historical
M4 and earlier post-M4 selection marker and is never synthesized for a new run.

### One verifier model with two surfaces

Slice 3 unifies the verifier records into one model with two surfaces.
Architecture 17's canonical records remain the substantive owner:
`VerifierAuthorityV1` and `VerifierAuditBaselineV1` with the five delegated
operation set `MarkNeedsRework`, `MarkComplete`, `Stop`, `ReviseFull`, and
`ResolveUnknownEffect`. `Pause` and `Resume` are user-lifecycle operations
only and carry no verifier authority. Architecture 28's Goal-facing public
projection DTOs are the public surface over those canonical records:
`VerificationMandateAuthorityDto`, `VerificationTargetSetDto`,
`VerificationTargetDto`, `VerificationAuditContractDto`,
`VerificationAuditEvidenceDto`, `VerificationTargetOperationDto`,
`VerificationTargetMutationDto`, and `VerificationAuditVerdictDto`. Authority
is usable only while its revision is active, unrevoked, unexpired, and
unconsumed where required; a mutation commits atomically or not at all; a
stale or missing baseline fails closed; and recovery preserves
authority/audit/mutation history without replaying verifier evidence work or
reapplying a committed mutation. Mandate-scoped authority transactions stay
Milestone 11; Slice 3 provides the records, validation, and Goal-facing
projections.

### Harness-side accepted directions and the Build-mode boundary

This activation delivers the harness-side accepted directions of
[ADR 0033](0033-accepted-m5plus-execution-directions.md): EXC-041 (autonomous
harness goal mode; goal-directed rule continuation that is separately admitted
and never a free-running agent) and EXC-042 (work/requeue after client
disconnection; an explicit durable contract that never silently resumes old
external work). The Build-mode Mandate continuation direction of
[ADR 0031](0031-autonomous-continuation-direction.md), owned by architecture
13, and its evidence EVD-027 stay with Milestone 10 (Mandate core) and are not
delivered by this activation.

## Appendix A: Slice 3 canonical selection field tables

This appendix is the normative field-level reference for the Slice 3 canonical
selection records. Field tags apply to canonical typed-TLV records. Nested
anonymous records carry tag 0 with their own variant version; every listed
field is mandatory unless the Required column says otherwise. Selection text
values are at most 256 characters, non-blank, and free of control or NUL
characters; a credential-shaped value rejects with
`CanonicalError::CredentialsForbidden`, and an over-limit record, list, or
bound rejects with `CanonicalError::OverLimit`. No table cell spans multiple
lines.

### programmatic-caller-policy-selection-v1 (0x0201)

Field 1 is reworked by Slice 3 to the typed closed nested record
`ProgrammaticCallerRootOriginV1`; the former `ExecutionKind` U64 shape is
removed and fails closed. Fields 2-5 and the nested `FixedRunLimits` field
table are unchanged from the Slice 1 ledger.

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 1 | root_origin | Record | Yes | Nested `ProgrammaticCallerRootOriginV1` (tag 0); closed variants below; the Slice 1 `ExecutionKind` U64 shape is removed |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 2 | effective_policy_snapshot_reference | Uuid | Yes | The immutable effective policy snapshot selected at admission |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 3 | policy_selection_digest | Digest | Yes | SHA-256 digest |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 4 | inherited_scope_provenance | List | Yes | Ordered list of UUIDs |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 5 | fixed_run_limits | Record | Yes | Nested `FixedRunLimits` (tag 0); tags 1-6 unchanged from the Slice 1 table |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 1 | root_origin.InteractiveUser.originating_turn_id | Uuid | Yes | Nested variant version 1; the originating user turn identity |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 1 | root_origin.ContinualHarness.harness_id | Uuid | Yes | Nested variant version 2; the harness identity |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 2 | root_origin.ContinualHarness.rule_revision | U64 | Yes | Nested variant version 2; nonzero; the active immutable rule revision that admitted the launch |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 3 | root_origin.ContinualHarness.trigger_reason_id | Uuid | Yes | Nested variant version 2; the durable trigger reason identity |

### goal-run-selection-v1 (0x0203)

The whole record is bounded at 1 MiB.

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `goal-run-selection-v1` (0x0203) | 1 | 1 | leading_goal_id | Uuid | Yes | The leading Goal identity |
| `goal-run-selection-v1` (0x0203) | 1 | 2 | goal_revision | U64 | Yes | Nonzero; the exact immutable Goal revision |
| `goal-run-selection-v1` (0x0203) | 1 | 3 | scope_link_provenance | Record | Yes | Nested `GoalScopeLinkProvenanceV1` (tag 0, version 1); fields below |
| `goal-run-selection-v1` (0x0203) | 1 | 4 | parent_revision_chain | List | Yes | Ordered nested `GoalRevisionReferenceV1` records; at most 16 |
| `goal-run-selection-v1` (0x0203) | 1 | 5 | obligatory_component_references | List | Yes | Ordered UUIDs; at most 32 |
| `goal-run-selection-v1` (0x0203) | 1 | 6 | selected_gate_revisions | List | Yes | Nested `GoalGateRevisionReferenceV1` records; at most 32 |
| `goal-run-selection-v1` (0x0203) | 1 | 7 | valid_evidence_references | List | Yes | UUIDs; at most 512 |
| `goal-run-selection-v1` (0x0203) | 1 | 8 | selected_memory_cards | List | Yes | Nested `GoalCardReferenceV1` records; count at most `bounds.max_memory_cards` |
| `goal-run-selection-v1` (0x0203) | 1 | 9 | selected_skill_cards | List | Yes | Nested `GoalCardReferenceV1` records; count at most `bounds.max_skill_role_cards` |
| `goal-run-selection-v1` (0x0203) | 1 | 10 | selected_role_cards | List | Yes | Nested `GoalCardReferenceV1` records; count at most `bounds.max_skill_role_cards` |
| `goal-run-selection-v1` (0x0203) | 1 | 11 | revealed_full_record_references | List | Yes | UUIDs; at most 512 |
| `goal-run-selection-v1` (0x0203) | 1 | 12 | policy_snapshot_reference | Uuid | Yes | The selected effective programmatic-caller policy snapshot |
| `goal-run-selection-v1` (0x0203) | 1 | 13 | activity_selection_reference | Uuid | Yes | The selected agent-activity selection reference |
| `goal-run-selection-v1` (0x0203) | 1 | 14 | run_kind | U64 | Yes | Closed: `GoalDirectedOrdinary` (0), `VerificationOnly` (1) |
| `goal-run-selection-v1` (0x0203) | 1 | 15 | target_snapshot_digest | Digest | Yes | Canonical target-snapshot digest |
| `goal-run-selection-v1` (0x0203) | 1 | 16 | bounds | Record | Yes | Nested `GoalRunSelectionBoundsV1` (tag 0, version 1); fields below |
| `goal-run-selection-v1` (0x0203) | 1 | 1 | scope_link_provenance.project_id | Uuid | Yes | Nested field; the project identity |
| `goal-run-selection-v1` (0x0203) | 1 | 2 | scope_link_provenance.session_id | Optional | Yes | Nested field; one-byte presence marker plus a UUID when present |
| `goal-run-selection-v1` (0x0203) | 1 | 3 | scope_link_provenance.link_id | Optional | Yes | Nested field; a link identity requires an owner session identity, otherwise `InvalidField` |
| `goal-run-selection-v1` (0x0203) | 1 | 1 | parent_revision_chain[].goal_id | Uuid | Yes | Nested `GoalRevisionReferenceV1` field |
| `goal-run-selection-v1` (0x0203) | 1 | 2 | parent_revision_chain[].revision | U64 | Yes | Nested field; nonzero |
| `goal-run-selection-v1` (0x0203) | 1 | 1 | selected_gate_revisions[].gate_reference | Uuid | Yes | Nested `GoalGateRevisionReferenceV1` field |
| `goal-run-selection-v1` (0x0203) | 1 | 2 | selected_gate_revisions[].revision | U64 | Yes | Nested field; nonzero |
| `goal-run-selection-v1` (0x0203) | 1 | 1 | selected_memory_cards[].card_reference | Uuid | Yes | Nested `GoalCardReferenceV1` field; also used by selected Skill and role cards |
| `goal-run-selection-v1` (0x0203) | 1 | 2 | selected_memory_cards[].revision | U64 | Yes | Nested field; nonzero |
| `goal-run-selection-v1` (0x0203) | 1 | 1 | bounds.max_memory_cards | U64 | Yes | Nested field; 1..=128 |
| `goal-run-selection-v1` (0x0203) | 1 | 2 | bounds.max_skill_role_cards | U64 | Yes | Nested field; 1..=32 |
| `goal-run-selection-v1` (0x0203) | 1 | 3 | bounds.target_snapshot_bytes | U64 | Yes | Nested field; 1..=1 MiB |
| `goal-run-selection-v1` (0x0203) | 1 | 4 | bounds.context_bytes | U64 | Yes | Nested field; 1..=4 MiB |

### continual-harness-selection-v1 (0x0204)

The whole record is bounded at 512 KiB.

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `continual-harness-selection-v1` (0x0204) | 1 | 1 | harness_id | Uuid | Yes | The harness identity |
| `continual-harness-selection-v1` (0x0204) | 1 | 2 | rule_revision | U64 | Yes | Nonzero; the active immutable rule revision that admitted the launch |
| `continual-harness-selection-v1` (0x0204) | 1 | 3 | trigger_reason | Record | Yes | Nested `HarnessTriggerReasonV1` (tag 0, version 1); fields below |
| `continual-harness-selection-v1` (0x0204) | 1 | 4 | class_resolution | Record | Yes | Nested `HarnessClassResolutionV1` (tag 0, version 1); fields below |
| `continual-harness-selection-v1` (0x0204) | 1 | 5 | dossier_digest | Digest | Yes | The dossier digest built at admission |
| `continual-harness-selection-v1` (0x0204) | 1 | 6 | checkpoint_reference | Optional | Yes | One-byte presence marker plus a UUID when present; the closed `None` form otherwise |
| `continual-harness-selection-v1` (0x0204) | 1 | 7 | time_zone_application | Utf8 | Yes | 1..=256 characters; non-blank and control-free |
| `continual-harness-selection-v1` (0x0204) | 1 | 8 | bounds | Record | Yes | Nested `HarnessSelectionBoundsV1` (tag 0, version 1); fields below |
| `continual-harness-selection-v1` (0x0204) | 1 | 1 | trigger_reason.reason_id | Uuid | Yes | Nested field; the stable daemon-assigned reason identity |
| `continual-harness-selection-v1` (0x0204) | 1 | 2 | trigger_reason.source_kind | U64 | Yes | Nested field; closed: `ExplicitUserLaunch` (0), `CalendarTime` (1), `FixedInterval` (2), `TerminalOutcomeLink` (3) |
| `continual-harness-selection-v1` (0x0204) | 1 | 3 | trigger_reason.first_observed_at_ms | U64 | Yes | Nested field; first observation time in Unix milliseconds |
| `continual-harness-selection-v1` (0x0204) | 1 | 4 | trigger_reason.last_observed_at_ms | U64 | Yes | Nested field; at least `first_observed_at_ms` |
| `continual-harness-selection-v1` (0x0204) | 1 | 5 | trigger_reason.coalesced_count | U64 | Yes | Nested field; nonzero |
| `continual-harness-selection-v1` (0x0204) | 1 | 1 | class_resolution.class | U64 | Yes | Nested field; closed: `Light` (0), `Medium` (1), `Heavy` (2) |
| `continual-harness-selection-v1` (0x0204) | 1 | 2 | class_resolution.narrowed_tool_ids | List | Yes | Nested field; ordered registered tool identifiers; at most 16; each 1..=256 characters |
| `continual-harness-selection-v1` (0x0204) | 1 | 1 | bounds.max_cause_depth | U64 | Yes | Nested field; 1..=8 |
| `continual-harness-selection-v1` (0x0204) | 1 | 2 | bounds.max_concurrent | U64 | Yes | Nested field; 1..=16 |
| `continual-harness-selection-v1` (0x0204) | 1 | 3 | bounds.max_total_launches | U64 | Yes | Nested field; 1..=256 |
| `continual-harness-selection-v1` (0x0204) | 1 | 4 | bounds.dossier_bytes | U64 | Yes | Nested field; 1..=512 KiB |
| `continual-harness-selection-v1` (0x0204) | 1 | 5 | bounds.checkpoint_bytes | U64 | Yes | Nested field; 1..=512 KiB |
| `continual-harness-selection-v1` (0x0204) | 1 | 6 | bounds.conclusion_bytes | U64 | Yes | Nested field; 1..=512 KiB |

### mcp-method-catalog-selection-v1 (0x0205)

The whole record is bounded at 512 KiB.

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `mcp-method-catalog-selection-v1` (0x0205) | 1 | 1 | method_catalog_revision | Utf8 | Yes | 1..=256 characters; non-blank and control-free |
| `mcp-method-catalog-selection-v1` (0x0205) | 1 | 2 | connection_reference | Uuid | Yes | The selected MCP connection reference |
| `mcp-method-catalog-selection-v1` (0x0205) | 1 | 3 | server_revision_digest | Digest | Yes | The discovered server revision digest |
| `mcp-method-catalog-selection-v1` (0x0205) | 1 | 4 | method_reference | Utf8 | Yes | The exact selected method reference; 1..=256 characters |
| `mcp-method-catalog-selection-v1` (0x0205) | 1 | 5 | method_schema_revision | Utf8 | Yes | 1..=256 characters; non-blank and control-free |
| `mcp-method-catalog-selection-v1` (0x0205) | 1 | 6 | typed_input_constraint_family | Optional | Yes | Optional text, 1..=256 characters when present |

## Appendix B: SQLite Slice 3 table-family inventory

The Slice 3 DDL is part of the single current storage schema (logical version
1, [ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md)) and
creates the following durable table families directly on open through the
three new repository modules; M3/M4 tables and rows are never rewritten.

| Table family | Repository module | Purpose |
| --- | --- | --- |
| Harness rules | `harness_repo.rs` | Durable user-managed harness rules with scope, lifecycle state, and active revision |
| Harness revisions | `harness_repo.rs` | Append-only immutable rule revisions (task, class reference, sources, schedule and trigger parts) |
| Harness trigger reasons | `harness_repo.rs` | Durable coalesced trigger reasons captured before admission with stable reason identity |
| Harness checkpoints | `harness_repo.rs` | Verified checkpoint records and references; the previous checkpoint is retained on failure |
| Harness journal | `harness_repo.rs` | Durable versioned harness journal records readable after recovery |
| Harness counters | `harness_repo.rs` | Cause-chain, concurrency, and launch counters backing the code-owned harness bounds |
| Policy records | `programmatic_policy_repo.rs` | Durable policy identities with scope, lifecycle state, and active revision |
| Policy revisions | `programmatic_policy_repo.rs` | Immutable policy revisions (root-origin rules, admission rules, limits, calendar period) |
| Policy snapshots | `programmatic_policy_repo.rs` | Immutable effective policy snapshots frozen at admission |
| Policy confirmations | `programmatic_policy_repo.rs` | Exact durable user confirmations bound to one tool call, root tree, and typed input digest |
| Policy corridors | `programmatic_policy_repo.rs` | Bounded confirmation corridors bound to one active root tree |
| Policy drafts | `programmatic_policy_repo.rs` | Inactive model-prepared policy drafts awaiting a user decision, with evidence coalescing |
| Policy counters | `programmatic_policy_repo.rs` | Per-policy calendar and run counters shared across scopes, sessions, branches, children, and corridors |
| Policy reservations | `programmatic_policy_repo.rs` | Atomic admission-time reservations, released on known pre-effect or made permanent at `ToolCallStarted` |
| Goal records | `goal_repo.rs` | Goal identities with scope, lifecycle, readiness, and user-decision state |
| Goal revisions | `goal_repo.rs` | Immutable Goal revisions, links, and canonical digests |
| Gate records | `goal_repo.rs` | Reference and executable gate definitions, typed templates, revisions, and results |
| Memory records | `goal_repo.rs` | Durable memory cards with kind, owner scope, and immutable revisions |
| Skill and role records | `goal_repo.rs` | Durable Skill and role cards with immutable revisions and narrowing rules |
| Proposal records | `goal_repo.rs` | Coalesced model proposals (refinement drafts) awaiting a user decision |
| Compaction records | `goal_repo.rs` | Conversation-summary working-form records with predecessor references |
| Verification records | `goal_repo.rs` | Verification authority, audit baseline, evidence, verdict, and target-mutation records with Goal-facing projections |

## Version and tag linkage

`intention-domain::canonical::TagRegistry` is the sole numeric owner of the
ledger tag values. `intention-protocol` references those constants through
nonserialized `ContractFamilyDescriptor` entries in
`PUBLIC_WIRE_CONTRACT_FAMILIES`; there is no protocol-side numeric mirror. The
protocol parity tests fail on any missing, duplicate, or mismatched tag: the
wire families must cover exactly the 23 wire-ledger tags, may duplicate only
for the versioned fork snapshot and preview families, and must match the domain
registry values by name.
