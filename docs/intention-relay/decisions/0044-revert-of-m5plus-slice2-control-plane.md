# ADR 0044: Revert of the M5+ Slice 2 control plane

## Status

Accepted 2026-09-26. It supersedes
[ADR 0037](0037-m5plus-slice2-control-plane.md), whose status becomes
`Superseded`, and it supersedes one implementation instruction of
[ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md) as recorded
below. It activates nothing: no crate, DTO, tag, wire, storage schema,
configuration field, capability, test target, feature profile, quality-policy
target, or production behavior is authorized here.

Amended 2026-09-30 by
[ADR 0045](0045-local-json-rpc-2-0-transport.md): its "Local protocol 1.1,
unchanged" version-ledger row and the compatibility sentence that accepts
exactly protocol 1.1 are superseded — the local protocol is JSON-RPC 2.0 over
NDJSON at protocol version 2.0, matched by exact equality with no capability
plane. The remainder of this record stands.

## Scope and supersession

This decision reverts the M5+ Slice 2 control-plane activation and restores the
pre-Slice-2 state of the ordinary runtime. The code, contracts, DTOs,
persistence, test targets, canonical goldens, and delivery claims that
[ADR 0037](0037-m5plus-slice2-control-plane.md) activated are removed, and the
ordinary turn-acceptance path is selection-less again.

Slice 2 is not "in progress" and it is not partially delivered: the activation
is withdrawn. The accepted post-M5 directions that Slice 2 was to deliver
remain accepted as documentation direction under their owning documents:

- the configuration/provider control-plane cluster of
  [ADR 0020](0020-configuration-provider-control-plane-directions.md) owned by
  [architecture 25](../architecture/25-configuration-provider-control-plane.md);
- the provider session-selection and profiles protocol of
  [ADR 0024](0024-provider-session-and-profiles-protocol-directions.md) owned by
  [architecture 29](../architecture/29-provider-session-and-profiles-protocol.md);
- the provider reasoning and catalog detail of
  [ADR 0028](0028-provider-reasoning-and-catalog-detail-directions.md) and the
  accepted execution directions of
  [ADR 0033](0033-accepted-m5plus-execution-directions.md) owned by
  [architecture 22](../architecture/22-provider-evolution-profiles-and-reasoning.md).

Those directions are again documentation-only and can return only through a new
activating specification. This record is that withdrawal, not a new activation.

This record supersedes ADR 0037 in full. ADR 0035 remains the activation home
and slice-sequence authority: slices 3, 4, and 5 are untouched, their reserved
tags and contracts are untouched, and the five-slice order is unchanged.
[ADR 0036](0036-m5plus-slice1-contract-ledger.md) remains accepted for the
Slice 1 contract ledger; its clauses that describe the six Slice 2 tags as wired
are read through this record and through the domain tag registry. This record
also supersedes the single implementation instruction of ADR 0038 Wave 7 that
required keeping only the selection-carrying turn variant: with provider
selection removed, the selection-less `ApplicationService::send_user_turn_and_schedule`
path is the only live turn-acceptance path again. Every other ADR 0038 removal
stands.

## Decision

### Reverted surfaces

The Slice 2 control plane is removed from the production tree:

- **Domain.** `intention-domain` drops the `provider_catalog`,
  `provider_selection`, `reasoning_history`, and `context_projection` modules
  together with their canonical codecs, validation, and rejection semantics for
  `model-capability-taxonomy-v1` (0x0206), `provider-profile-revision-v1`
  (0x0207), `provider-selection-v1` (0x0208),
  `reasoning-history-manifest-v1` (0x0209), `context-source-manifest-v1`
  (0x020A), and `model-context-projection-v1` (0x020B), and deletes the six
  canonical goldens `*-v1.txt` for those families.
- **Application.** `intention-application` drops `provider_catalog`,
  `provider_control_plane`, `provider_gate`, `provider_registry`, and
  `session_selection`: the catalog lifecycle and registry, the readiness gate,
  explicit reload, credential rotation, health checks, discovery, pricing,
  session defaults and per-turn/fork overrides, unavailable-queue promotion and
  reconciliation, pending-removal and degraded recovery, and held
  recovered-run admission.
- **Configuration.** `intention-config` drops the `control_plane` module:
  typed-edit candidate rendering, private credential restore, the
  `configuration_edit_invalid` failure, and the `reload_status` projection with
  its closed `ConfigurationReloadStatusDto` vocabulary.
- **Storage.** `intention-storage-sqlite` drops the `control_plane` module and
  the fifteen control-plane tables inventoried in ADR 0037 Appendix B
  (`provider_kind_descriptor_revisions`, `provider_profile_revisions`,
  `provider_profile_tombstones`, `provider_kind_tombstones`,
  `provider_catalog_state`, `provider_catalog_profile_projection`,
  `configuration_audit`, `session_provider_defaults`,
  `resolved_run_provider_selections`, `unavailable_provider_queue`,
  `unavailable_queue_reconciliation_markers`, `provider_usage_aggregates`,
  `provider_usage_facts`, `provider_catalog_removal_candidates`, and
  `held_recovered_runs`).
- **Protocol.** `intention-protocol` drops the control-plane command, query,
  projection, and event DTO families and their semantic decode-time validation,
  including `SetSessionProviderProfileCommandDto`,
  `GetSessionProviderProfileQueryDto`, the catalog status/removal commands, the
  reconciliation and held-run admission commands,
  `ProviderHealthEvidenceDto`, and `SessionProviderProfileChangedEventDto`.
  The daemon no longer advertises or serves `provider_profiles_v1`, and the
  shared client requires only the M3 baseline capabilities again
  (`session_subscriptions`, `correlated_requests`, `daemon_health`).
- **Model.** `intention-model` drops the Slice 2 authentication-header policy
  (`AuthenticationHeaderPolicyV1`), the credential transport mode, the
  reasoning effort levels, and the reasoning categories/summaries
  (`ReasoningFragmentCategoryDto`, `ReasoningSummaryDelta`); the M4 normalized
  reasoning events remain, including the textless reasoning-channel presence
  marker (`ModelEventDto::reasoning_presence`) that
  [ADR 0041](0041-same-run-reasoning-round-trip.md) requires the continuation
  request to carry beside the assistant tool calls.
- **Daemon and client.** `intention-daemon` drops catalog startup/prepare/accept,
  the readiness gate, and the catalog-driven client surface; `intention-client`
  drops the control-plane and session-selection methods and their test targets.
- **Turn acceptance.** `ApplicationService::send_user_turn_and_schedule`
  accepts and schedules a user turn without provider selection; the
  selection-carrying variant and the selection-resolution service are removed.
- **Tests, goldens, and policy.** The twelve Slice 2 integration test targets
  are removed from the machine-readable policy and the tree
  (`m5_control_plane_canonical`, `m5_control_plane_rejections`,
  `m5_session_selection_overrides`, `control_plane_contracts`,
  `m5_control_plane_config`, `m5_catalog_runtime`, `m5_control_plane_runtime`,
  `m5_session_selection`, `control_plane_client`, `session_selection_client`,
  `m6_reasoning_surface`, and `m5_control_plane_repos`), and the six canonical
  goldens are deleted with them. No new crate, dependency,
  feature, coverage tier, or exclusion is introduced by this revert.

### Deliberately surviving decisions and surfaces

This revert is scoped to Slice 2 only. The following remain in force:

- every legacy, fallback, migration, and dead-variant removal of
  [ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md), including
  the `invoke_local_tool_once` and dead-variant deletions and the mandatory
  tool-executor execution path; the single-version rule for every versioned
  system; and the single live SQLite schema (logical version 1);
- the ordinary request-side tool advertisement of
  [ADR 0039](0039-request-side-tool-advertisement.md);
- the opt-in live-provider end-to-end channel of
  [ADR 0040](0040-opt-in-live-provider-e2e.md): `real_api_e2e.rs`,
  `make e2e-real-api`, and the manual workflow stay exactly as recorded, and
  remain the controller's manual live-evidence obligation;
- the same-run provider reasoning round-trip of
  [ADR 0041](0041-same-run-reasoning-round-trip.md);
- the project script library for kernel cells of
  [ADR 0042](0042-project-script-library-for-kernel-cells.md);
- the instruction sources and system context of
  [ADR 0043](0043-instruction-sources-and-system-context.md), whose control-plane
  editing and preview surface remains accepted future work for the fifth slice;
- the unconsumed-surface audit removals of 2026-09 (the typed
  preservation-control, server-side-parser, Responses reasoning-mode,
  reasoning-usage, and model-capability-envelope contracts, the protocol-only
  reasoning/header/parser duplicates, the eight producer-less control-plane
  event DTOs, and the `provider_profile_tombstoned` wire code); the revert does
  not restore any audited-away surface;
- every review-remediation item recorded in this branch's history
  (`bounded_contracts`, transport stale-socket reclaim, the
  `opaque_json_guard` storage boundary guard, and the `run_coverage` dedup).

### Version ledger

| Contract | Version/status after this revert |
| --- | --- |
| Local protocol | 1.1, unchanged |
| Public DTO schema | 1.1, unchanged |
| TOML configuration schema | 1, single shape; unversioned documents fail closed ([ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md)) |
| SQLite storage schema | Logical version 1, unchanged: one live schema created directly on open, now without the fifteen control-plane tables; no migration chain and no version gate |

The revert changes no protocol version, no DTO schema version, no configuration
format version, and no storage schema version. It removes records from the
single live schema rather than opening a second version.

### Numeric tag and wire-family consequence

Tags 0x0206-0x020B return to `ReservedForSlice2` in the domain-owned
`TagRegistry::LEDGER` and carry no production codec; the `ReservedForSlice3`
and `ReservedForSlice4` entries are untouched. The six family names remain in
the numeric ledger and in the wire-family descriptor table as reserved ledger
entries, so the ledger parity test keeps its inventory; no canonical record,
public DTO, or serving surface exists for them. The frozen ADR 0037 tag table
and the ADR 0036 Slice-2 clauses that describe the six tags as wired are
historical text in superseded records; the registry is the authoritative state.

### Quality-policy, test-target, and golden consequence

The twelve Slice 2 integration test targets no longer exist, and the six
canonical `*-v1.txt` goldens are deleted. `make quick`, `make verify`,
docs-check, architecture-check, and Linux/Windows CI remain the acceptance gate
for the revert, with no new CI job, Makefile target, crate, dependency, feature
profile, coverage tier, or exclusion. The frozen ADR 0037 record stays on disk
and indexed as the superseded Slice 2 ledger.

### Re-introduction condition

Slice 2 can return only through a new activating specification, prepared and
accepted under the milestone rules of
[ADR 0035](0035-m5plus-complete-foundation-activation.md) and the quality policy:
it must declare the exact crates, contracts, DTO/wire/storage versions, feature
profiles, coverage tiers, test targets, fixtures, evidence anchors, and
documentation updates atomically, restore the tags through the domain registry,
and pass `make quick`, `make verify`, docs-check, architecture-check, and
Linux/Windows CI. Until such a specification is accepted, the control plane is
not delivered, not in progress, and not authorized.

## Invariants

1. No selection. Ordinary turn acceptance is selection-less; no session
   default, per-turn override, fork override, profile resolution, or catalog
   lookup exists on the live path.
2. Startup-only configuration. M3/M4 TOML application remains the only
   configuration behavior; no reload, rotation, typed edit, or catalog accept
   path exists.
3. Reserved tags. Tags 0x0206-0x020B carry no production codec and remain
   reserved for Slice 2; no future activation reuses them without a new
   activating specification.
4. One live schema. SQLite remains one logical schema version 1 created
   directly on open, now without the control-plane tables; histories and
   historical bytes are never rewritten.
5. Unchanged versions. Protocol 1.1, public DTO schema 1.1, and TOML schema 1
   are the only live versions before and after the revert.
6. Surviving decisions. ADRs 0038 (except the Wave 7 instruction superseded
   here), 0039, 0040, 0041, 0042, and 0043, the 2026-09 unconsumed-surface audit
   removals, and every review-remediation item remain in force.
7. No partial activation. The reverted surface must not reappear in parts;
   re-introduction is one new activating specification covering the whole slice.

## Compatibility

Historical M3/M4 and M5 bytes, runs, sessions, events, snapshots, queue
tickets, cursors, selections, and tool evidence keep their recorded meaning; no
synthetic control-plane, catalog, profile, or selection state is fabricated for
them. The revert removes only Slice 2 production surfaces and their tests. The
removed durable tables are part of the single current schema and are removed in
place; they never carried M3/M4 rows. Cross-version negotiation still accepts
exactly protocol 1.1 and rejects an incompatible major version as before.

## Security and failure behavior

No Slice 2 control-plane failure code is reachable after the revert: the
catalog, readiness, rotation, health, discovery, pricing, session-selection,
promotion/reconciliation, and pending-removal families are removed with their
surfaces, and no code path emits them. The surviving failure behavior is the
M3/M4 ordinary path: credentials remain non-serde, non-`Debug`, and absent from
public/durable surfaces; unavailability is typed and non-authorizing; recovery
interrupts unfinished work and resumes nothing. Removing the typed-edit and
credential-restore paths also removes the only Slice 2 surface that handled
private credential material server-side.

## Non-goals

No partial restoration, compatibility shim, feature flag, or dormant module for
the reverted surface; no new runtime, registry, scheduler, persistence
authority, or sandbox; no re-activation of any audited-away contract; no
M6-M9 boundary implementation; no change to slices 3, 4, or 5; no rewriting of
historical ADR text beyond the status change recorded here; and no reopening of
closed milestones M0-M5.

## Affected documents

- [ADR 0037](0037-m5plus-slice2-control-plane.md) is superseded and its status
  is `Superseded`; its frozen ledger, appendices, and test-target declarations
  remain on disk as the historical Slice 2 record.
- [ADR 0035](0035-m5plus-complete-foundation-activation.md) remains the
  activation home; slices 3-5 are unchanged.
- [ADR 0036](0036-m5plus-slice1-contract-ledger.md) remains accepted; its
  Slice-2 tag clauses are read through this record.
- [ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md) stays in
  force except for Wave 7's selection-carrying-only instruction, which this
  record supersedes.
- [ADR 0020](0020-configuration-provider-control-plane-directions.md),
  [ADR 0024](0024-provider-session-and-profiles-protocol-directions.md),
  [ADR 0028](0028-provider-reasoning-and-catalog-detail-directions.md), and
  [ADR 0033](0033-accepted-m5plus-execution-directions.md) remain accepted
  documentation directions awaiting a new activating specification.
- [Architecture 00](../architecture/00-principles-and-scope.md),
  [02](../architecture/02-dto-and-contract-policy.md),
  [09](../architecture/09-configuration-security-and-observability.md),
  [11](../architecture/11-implementation-roadmap.md),
  [12](../architecture/12-quality-gates-and-makefile.md),
  [22](../architecture/22-provider-evolution-profiles-and-reasoning.md),
  [25](../architecture/25-configuration-provider-control-plane.md),
  [29](../architecture/29-provider-session-and-profiles-protocol.md), and the
  [architecture README](../architecture/README.md) record the revert.
- The reconciliation registers, the root
  [README](../../../README.md), and
  `m4plus_concept.md` record the reverted state.

## Evidence

The revert is accepted only together with:

- the removal of the Slice 2 production modules, DTOs, methods, test targets,
  and goldens listed above, with no remaining reference to a removed surface;
- the restoration of `ReservedForSlice2` for tags 0x0206-0x020B in the domain
  registry, with `ReservedForSlice3`/`ReservedForSlice4` untouched and the
  ledger parity inventory intact;
- one live SQLite schema (logical version 1) created directly on open without
  the fifteen control-plane tables, with the M3/M4 current-schema round trips
  passing;
- selection-less turn acceptance covered by the ordinary application,
  runtime, and daemon integration tests, including the real-binary facade E2E
  scenario;
- the surviving ADR 0038, 0039, 0040, 0041, 0042, and 0043 surfaces and their
  tests passing unchanged;
- `python3 quality/self_test.py`, `python3 quality/check_docs.py`, the
  architecture check, `make quick`, `make verify`, and Linux/Windows CI;
- reconciliation rows and register edits that describe the reverted state and
  cite this record.

## Research provenance

- [ADR 0037](0037-m5plus-slice2-control-plane.md): the reverted Slice 2
  activating specification; its decisions, appendices, and evidence anchors are
  retained as the historical record of what was withdrawn.
- [ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md): the
  single-version and legacy-removal program whose surviving waves define the
  post-revert baseline.
- `m4plus_concept.md`: research provenance only; its
  single Slice 2 sentence was updated to past tense with a pointer to this
  record, and its research content is otherwise untouched, as the roadmap
  requires.
