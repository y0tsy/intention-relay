# ADR 0037: M5+ Slice 2 control-plane activation

## Status

Accepted as the Slice 2 activating specification required by
[ADR 0035](0035-m5plus-complete-foundation-activation.md). It activates Slice 2
(Control plane) of ADR 0035 and is subordinate to ADR 0035: ADR 0035 remains the
activation home and slice-sequence authority, and this record activates only the
control-plane slice, not slices 3-4, M6, M7, M8, or M9.

## Scope and supersession

This record freezes and activates only M5+ Slice 2, Control plane. In scope is
the full control-plane list from the roadmap Slice 2 line: controlled live
reload; credential rotation; provider health checks; model discovery; pricing
policy; provider profile UI and raw-TOML/configuration editing; arbitrary
authentication headers; provider-native preservation controls; server-side
parser setup; session defaults and per-turn/fork overrides; unavailable-queue
promotion and reconciliation; `provider_profiles_v1`; pending-removal and
degraded recovery; and the provider reasoning/catalog surface.

Out of scope: M6-M9 behavior; any second runtime, registry, scheduler,
persistence authority, or sandbox; remote continuation; and any M3/M4 rewriting.
M3/M4 startup-only configuration, recorded revisions, persisted run snapshots,
queue tickets, sessions, runs, events, and bytes remain authoritative and
unchanged. Historical runs receive no synthetic post-M5 records.

## Version ledger

| Contract | Version/status |
| --- | --- |
| Local protocol | 1.1, unchanged |
| Public DTO schema | 1.1, additive, unchanged |
| TOML configuration schema | 1, unchanged |
| SQLite storage schema | Logical version 1: single live schema created directly on open; no migration chain and no version gate ([ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md)) |

## Negotiated capabilities and failure semantics

The active runtime capability is `provider_profiles_v1`. The control-plane
surface (reload, rotation, health, discovery, pricing, raw-TOML/typed editing)
is served through the daemon facade with typed commands and queries. Health,
discovery, and pricing are NON-AUTHORIZING: they create no `RunId`, reason, or
selection, perform no model-name routing, and pricing is never an admission
ceiling. The effective capability set remains the hello intersection of both
peers; unnegotiated dependent work fails closed before effect.

| Capability | Status | Scope in Slice 2 |
| --- | --- | --- |
| `provider_profiles_v1` | ACTIVE runtime capability | Session defaults; per-turn/fork override fields on `SendUserTurnCommandDto`/`ForkSessionCommandDto`/`StartForkRunCommandDto`; unavailable-queue promotion (max 8 per terminal transition) and reconciliation (max 32 per page); profile-keyed usage; pending-removal accept/reject (30-minute lifetime); degraded recovery; held recovered-run admission (`AdmitRecoveredRunCommandDto`) |
| Control-plane surface (reload, rotation, health, discovery, pricing, raw-TOML/typed editing) | Served through the daemon facade with typed commands/queries | Reload is explicit (no watcher, polling, or auto-restart); rotation replaces private material only; health/discovery/pricing are non-authorizing evidence; raw-TOML/typed editing produces a server-side validated candidate through the reload contract |
| Health, discovery, pricing | NON-AUTHORIZING | Create no `RunId`/reason/selection; no model-name routing; pricing is never an admission ceiling |

The Slice 2 failure semantics are closed. Dependent work fails before effect
with the family errors:

| Family | Error codes |
| --- | --- |
| Negotiation and readiness | `provider_profiles_capability_required`, `execution_not_ready`, `catalog_not_ready`, `provider_profile_runtime_unavailable`, `provider_configuration_unavailable`, `provider_profile_unavailable`, `provider_profile_tombstoned`, `provider_admission_not_found` |
| Profile revisions | `provider_profile_revision_invalid`, `provider_profile_override_invalid`, `provider_profile_revision_mismatch`, `session_profile_revision_mismatch`, `session_provider_default_stale`, `config_revision_mismatch` |
| Kind, endpoint, and credentials | `provider_kind_immutable_mismatch`, `provider_kind_has_dependents`, `invalid_provider_kind`, `invalid_endpoint`, `credentials_forbidden`, `invalid_digest` |
| Catalog | `legacy_config_cannot_represent_active_catalog`, `catalog_page_token_stale`, `provider_catalog_projection_invalid`, `candidate_too_large`, `catalog_change_requires_restart` |
| Reasoning | `provider_reasoning_stream_invalid`, `reasoning_history_unavailable`, `reasoning_history_incompatible`, `reasoning_history_too_large`, `reasoning_output_limit_exceeded` |
| Context | `context_source_manifest_invalid`, `model_context_projection_invalid`, `model_context_projection_too_large` |
| Rotation, health, and discovery | `credential_rotation_frozen_meaning_mismatch`, `credential_rotation_source_unavailable`, `provider_health_unavailable`, `provider_discovery_unavailable` |

The former `control_plane_unavailable` dispatch stub is not active in Slice 2.
A dispatch stub remains in `crates/intention/src/lib.rs`; its removal is owned
by the code zone and is reported to the controller as a required follow-up.

## Canonical codec and identity

`intention-domain` owns the canonical codec and semantic canonical records.
The format is `IRCR` / `typed-tlv-v1` / SHA-256, unchanged from Slice 1.
Digest text is `<namespace>:sha256:<64 lowercase hex>`; new identities are
`sha256-v1:<64 hex>`. A digest excludes its own field. Digest inputs exclude
credentials, paths, display data, readiness, and current state. The newly
wired canonical families (0x0206-0x020B) carry no numeric field tags on their
public DTO families; canonical record families derive from
`crates/intention-domain` and public DTO families from
`crates/intention-protocol/src/contract_families.rs`.

## Numeric tag registry

`intention-domain` owns this registry. The `TagStatus` enum has exactly the
variants `Wired`, `ReservedForSlice3`, and `ReservedForSlice4`. Three tags were
already `Wired` from Slice 1; Slice 2 newly wires the six control-plane tags
(0x0206-0x020B). All other ledger tags remain reserved and carry no production
codec in this slice.

| Tag | Value | Status |
| --- | --- | --- |
| `run-execution-meaning` | `0x0101` | Wired |
| `programmatic-caller-policy-selection-v1` | `0x0201` | Wired |
| `agent-activity-selection-v1` | `0x0202` | Wired |
| `model-capability-taxonomy-v1` | `0x0206` | Wired (Slice 2) |
| `provider-profile-revision-v1` | `0x0207` | Wired (Slice 2) |
| `provider-selection-v1` | `0x0208` | Wired (Slice 2) |
| `reasoning-history-manifest-v1` | `0x0209` | Wired (Slice 2) |
| `context-source-manifest-v1` | `0x020A` | Wired (Slice 2) |
| `model-context-projection-v1` | `0x020B` | Wired (Slice 2) |
| `goal-run-selection-v1` | `0x0203` | ReservedForSlice3 |
| `continual-harness-selection-v1` | `0x0204` | ReservedForSlice3 |
| `mcp-method-catalog-selection-v1` | `0x0205` | ReservedForSlice3 |
| `tool-descriptor-revision` | `0x0301` | ReservedForSlice3 |
| `tool-registry-revision` | `0x0302` | ReservedForSlice3 |
| `model-tool-loop-v1` | `0x0303` | ReservedForSlice3 |
| `bridge-invocation-v1` | `0x0304` | ReservedForSlice3 |
| `fork-base-snapshot-v1/v2` | `0x0401` | ReservedForSlice4 |
| `fork-preview-v1/v2` | `0x0402` | ReservedForSlice4 |
| `fork-command-v1` | `0x0403` | ReservedForSlice4 |
| `agent-activity-tree-v1` | `0x0501` | ReservedForSlice4 |
| `agent-activity-pair-v1` | `0x0502` | ReservedForSlice4 |
| `agent-message-v1` | `0x0503` | ReservedForSlice4 |
| `agent-activity-journal-record-v1` | `0x0504` | ReservedForSlice4 |
| `agent-notification-record-v1` | `0x0505` | ReservedForSlice4 |

Reserved tags are not active capabilities and are not model-visible.

## Ownership and preservation invariants

Crate ownership for the Slice 2 surface is fixed:

| Surface | Owner |
| --- | --- |
| Canonical records, tags, digests, and validation | `intention-domain` |
| Wire families, negotiation, and typed commands/queries | `intention-protocol` |
| TOML parsing, validation, reload candidates, and configuration DTOs | `intention-config` |
| Schema-4 migrations, projections, and durable control-plane rows | `intention-storage` and `intention-storage-sqlite` |
| Provider-neutral reasoning surface (DTO-level) | `intention-model` |
| Provider translation and dialect decoding | provider crates (adapters) |
| Catalog, session-selection, and control-plane services | `intention-application` |
| Hosting, reload/rotation hosting, and degraded gate | `intention-daemon` |
| Typed client surface | `intention-client` |
| Composition and facade assembly | `intention` |
| Presentation adapters | TUI/Tauri adapters |

No new crate, dependency, feature, coverage tier, or exclusion is introduced.
Skeleton `intention-headroom`, `intention-plans`, `intention-vfr`, and
`intention-tauri` remain untouched, and M6-M9 behavior is untouched. M3/M4
config revisions, snapshots, sessions, runs, events, cursors, queue tickets,
and bytes remain authoritative and unchanged. The storage schema is the
single live schema (logical version 1) created directly on open under
[ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md): no migration
chain, no `user_version` gate, and no synthetic post-M5 record added to
historical runs. The legacy M4 selection
bridge (tag `legacy-m4-selection-binding` 0x020C) is removed by
[ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md): no synthetic
binding is ever materialized for historical runs.

## Evidence and non-goals

Required evidence covers the Slice 2 directions with per-direction test
anchors:

| Direction | Evidence anchor (test target) |
| --- | --- |
| Provider catalog lifecycle and capability resolution | `crates/intention-domain/tests/m5_control_plane_canonical.rs`; `crates/intention-application/tests/m5_catalog_runtime.rs` |
| Provider selection and rejection semantics | `crates/intention-domain/tests/m5_control_plane_rejections.rs`; `crates/intention-protocol/tests/control_plane_contracts.rs` |
| Session defaults and per-turn/fork overrides | `crates/intention-domain/tests/m5_session_selection_overrides.rs`; `crates/intention-client/tests/session_selection_client.rs` |
| Control-plane runtime (reload, rotation, health, discovery, pricing, raw-TOML/typed editing) | `crates/intention-application/tests/m5_control_plane_runtime.rs`; `crates/intention-client/tests/control_plane_client.rs` |
| Configuration reload and editing | `crates/intention-config/tests/m5_control_plane_config.rs` |
| Reasoning surface (DTO-level) | `crates/intention-model/tests/m6_reasoning_surface.rs` |
| SQLite single current storage schema | `crates/intention-storage-sqlite/tests/sqlite_contracts.rs` (current-schema tests) |

Required gates are `make quick`, `make verify`, `docs-check`, and Linux/Windows
CI.

This ledger does not implement M6-M9 behavior and does not introduce a second
runtime, registry, scheduler, persistence authority, or sandbox. It does not
implement remote continuation, live wire header injection (`SafeHeader`), a
user-kind parser, or provider-native live extraction beyond declared paths.

## Resolution notes

### Runtime version resolution

Protocol 1.1 and public DTO schema 1.1 remain the active advertised versions;
`intention-protocol` continues to define `CURRENT_PROTOCOL_VERSION` and
`CURRENT_DTO_SCHEMA_VERSION`, and the runtime crates reference them. TOML
configuration schema remains 1. SQLite storage is a single live schema
(logical version 1) created directly on open: the control-plane tables listed
in Appendix B are part of that schema, and the 3-to-4 migration chain and
`user_version` gate are removed by
[ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md). Historical
M3/M4 fixtures and
committed v1 protocol fixtures remain 1.0 and are not rewritten. Same-major
compatibility (1.0 to 1.1) is superseded by
[ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md): negotiation
accepts only the exact current protocol version 1.1 (the
`ensure_compatible_with` logic is removed).

### Registry tag activation status

The numeric tag registry is owned by `intention-domain`
(`crates/intention-domain/src/canonical.rs`). Every ledger tag is classified in
the `TagRegistry::LEDGER` table by `TagStatus` as `Wired`, `ReservedForSlice3`,
or `ReservedForSlice4`. Nine tags are `Wired` (three from Slice 1 plus the six
Slice 2 additions 0x0206-0x020B); every other tag is reserved and carries no
production codec in this slice.

### Capability and delivery outcomes

`provider_profiles_v1` is the active runtime capability for the Slice 2
session-selection and profiles surface. The `normalized_reasoning_stream_v1`
contract surface is present at DTO level (the provider-neutral reasoning DTO
family in `intention-model`), but the `responses` kind is not activated in
Slice 2: its contracts are closed only, and activation is blocked until a
complete driver exists. Raw-TOML editing is accepted server-side only, through
the validated reload contract; credentials are never echoed. Credential
rotation without a configured credential source fails closed with
`credential_rotation_source_unavailable`. Catalog-affecting configuration
changes are rejected with `catalog_change_requires_restart` in Slice 2.

## Appendix A: Public wire-family field tables

This appendix is the normative field-level reference for the newly activated
Slice 2 public wire families. Public DTO families are JSON objects and carry no
numeric field tags (marked `—`). Every listed field is mandatory unless the
Required column says otherwise. No table cell spans multiple lines.

### model-capability-taxonomy-v1 (0x0206)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `model-capability-taxonomy-v1` (0x0206) | 1 | — | ModelCapabilitySetV1.taxonomy_version | String | Yes | Closed value `model-capability-taxonomy-v1` |
| `model-capability-taxonomy-v1` (0x0206) | 1 | — | ModelCapabilitySetV1.input | String | Yes | Closed value `TextOnly` |
| `model-capability-taxonomy-v1` (0x0206) | 1 | — | ModelCapabilitySetV1.text_streaming | String | Yes | Closed: `Enabled` or `Disabled` |
| `model-capability-taxonomy-v1` (0x0206) | 1 | — | ModelCapabilitySetV1.structured_output | String | Yes | Closed value `Unsupported` |
| `model-capability-taxonomy-v1` (0x0206) | 1 | — | ModelCapabilitySetV1.reasoning | String | Yes | Closed: `Disabled` or `TextualReasoningV1` |
| `model-capability-taxonomy-v1` (0x0206) | 1 | — | ModelCapabilitySetV1.tool_exchange | String | Yes | Closed: `Disabled` or `ModelToolLoopV1` |
| `model-capability-taxonomy-v1` (0x0206) | 1 | — | ModelCapabilitySetV1.context_preservation | String | Yes | Closed value `LocalDurableHistoryV1` |

### provider-profile-revision-v1 (0x0207)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `provider-profile-revision-v1` (0x0207) | 1 | — | ProviderProfileRevisionV1.profile_id | String | Yes | Up to 256 scalar values; `provider_profile_revision_invalid` otherwise |
| `provider-profile-revision-v1` (0x0207) | 1 | — | ProviderProfileRevisionV1.revision_id | String | Yes | Up to 256 scalar values |
| `provider-profile-revision-v1` (0x0207) | 1 | — | ProviderProfileRevisionV1.provider_kind_id | String | Yes | Up to 256 scalar values |
| `provider-profile-revision-v1` (0x0207) | 1 | — | ProviderProfileRevisionV1.model_id | String | Yes | Up to 256 scalar values |
| `provider-profile-revision-v1` (0x0207) | 1 | — | ProviderProfileRevisionV1.endpoint | String | Yes | No userinfo, query, or fragment; `invalid_endpoint` otherwise |
| `provider-profile-revision-v1` (0x0207) | 1 | — | ProviderProfileRevisionV1.credential_transport_mode | CredentialTransportMode | Yes | Closed: `Bearer` or `SafeHeader` |
| `provider-profile-revision-v1` (0x0207) | 1 | — | ProviderProfileRevisionV1.safe_header_name | Option<String> | No | Up to 128 scalar values when present |
| `provider-profile-revision-v1` (0x0207) | 1 | — | ProviderProfileRevisionV1.capability_taxonomy_revision | String | Yes | Up to 256 scalar values |
| `provider-profile-revision-v1` (0x0207) | 1 | — | ProviderProfileRevisionV1.reasoning_compatibility_id | Option<String> | No | Up to 256 scalar values when present |

### provider-selection-v1 (0x0208)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `provider-selection-v1` (0x0208) | 1 | — | ResolvedRunProviderSelectionDto.selection_canonicalization_version | String | Yes |  |
| `provider-selection-v1` (0x0208) | 1 | — | ResolvedRunProviderSelectionDto.profile_id | String | Yes |  |
| `provider-selection-v1` (0x0208) | 1 | — | ResolvedRunProviderSelectionDto.provider_profile_revision_id | String | Yes |  |
| `provider-selection-v1` (0x0208) | 1 | — | ResolvedRunProviderSelectionDto.kind_id | String | Yes | `openai` rejected; `invalid_provider_kind` |
| `provider-selection-v1` (0x0208) | 1 | — | ResolvedRunProviderSelectionDto.kind_descriptor_revision_id | String | Yes |  |
| `provider-selection-v1` (0x0208) | 1 | — | ResolvedRunProviderSelectionDto.model_id | String | Yes |  |
| `provider-selection-v1` (0x0208) | 1 | — | ResolvedRunProviderSelectionDto.normalized_effective_endpoint | String | Yes | No userinfo, query, fragment, or control characters; `invalid_endpoint` |
| `provider-selection-v1` (0x0208) | 1 | — | ResolvedRunProviderSelectionDto.credential_transport_mode | CredentialTransportMode | Yes | Closed: `Bearer` or `SafeHeader` |
| `provider-selection-v1` (0x0208) | 1 | — | ResolvedRunProviderSelectionDto.credential_transport_safe_header_name | Option<String> | No |  |
| `provider-selection-v1` (0x0208) | 1 | — | ResolvedRunProviderSelectionDto.declared_model_capability_subset | Vec<String> | Yes |  |
| `provider-selection-v1` (0x0208) | 1 | — | ResolvedRunProviderSelectionDto.resolved_reasoning_policy | String | Yes |  |
| `provider-selection-v1` (0x0208) | 1 | — | ResolvedRunProviderSelectionDto.effective_execution_policy | String | Yes |  |
| `provider-selection-v1` (0x0208) | 1 | — | ResolvedRunProviderSelectionDto.effective_loopback_policy_or_not_applicable | String | Yes |  |
| `provider-selection-v1` (0x0208) | 1 | — | ResolvedRunProviderSelectionDto.provider_driver_contract_revision | String | Yes |  |
| `provider-selection-v1` (0x0208) | 1 | — | ResolvedRunProviderSelectionDto.selection_source | Option<String> | No | Immutable provenance, outside execution digest |

### reasoning-history-manifest-v1 (0x0209)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `reasoning-history-manifest-v1` (0x0209) | 1 | — | ReasoningHistoryManifestDto.compatibility_id | String | Yes |  |
| `reasoning-history-manifest-v1` (0x0209) | 1 | — | ReasoningHistoryManifestDto.entries | Vec<String> | Yes |  |
| `reasoning-history-manifest-v1` (0x0209) | 1 | — | ReasoningHistoryManifestDto.manifest_digest | String | Yes | 64 lowercase hex |

### context-source-manifest-v1 (0x020A)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `context-source-manifest-v1` (0x020A) | 1 | — | ContextSourceEntryV1.source_id | String | Yes | Nested entry DTO |
| `context-source-manifest-v1` (0x020A) | 1 | — | ContextSourceEntryV1.source_kind | String | Yes | Nested entry DTO |
| `context-source-manifest-v1` (0x020A) | 1 | — | ContextSourceEntryV1.revision | String | Yes | Nested entry DTO |
| `context-source-manifest-v1` (0x020A) | 1 | — | ContextSourceEntryV1.safe_label | Option<String> | No | Nested entry DTO |
| `context-source-manifest-v1` (0x020A) | 1 | — | ContextSourceManifestV1.compatibility_id | String | Yes |  |
| `context-source-manifest-v1` (0x020A) | 1 | — | ContextSourceManifestV1.source_entries | Vec<ContextSourceEntryV1> | Yes | 1..=256 entries; `context_source_manifest_invalid` otherwise |
| `context-source-manifest-v1` (0x020A) | 1 | — | ContextSourceManifestV1.manifest_digest | String | Yes | 64 lowercase hex |

### model-context-projection-v1 (0x020B)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `model-context-projection-v1` (0x020B) | 1 | — | ModelContextProjectionV1.projection_revision | String | Yes |  |
| `model-context-projection-v1` (0x020B) | 1 | — | ModelContextProjectionV1.context_schema_version | String | Yes |  |
| `model-context-projection-v1` (0x020B) | 1 | — | ModelContextProjectionV1.source_manifest_digest | String | Yes | 64 lowercase hex |
| `model-context-projection-v1` (0x020B) | 1 | — | ModelContextProjectionV1.ordered_messages | Vec<String> | Yes | 1..=1024 nonblank entries; up to 1 MiB aggregate; `model_context_projection_invalid`/`_too_large` |
| `model-context-projection-v1` (0x020B) | 1 | — | ModelContextProjectionV1.model_context_digest | String | Yes | 64 lowercase hex |

## Appendix B: SQLite control-plane table inventory

The control-plane DDL is part of the single current storage schema (logical
version 1, ADR 0038) and creates the following durable tables directly on
open; M3/M4 tables and rows are never rewritten.

| Table | Purpose |
| --- | --- |
| `provider_kind_descriptor_revisions` | Immutable provider-kind descriptor revisions |
| `provider_profile_revisions` | Append-only provider-profile revisions |
| `provider_profile_tombstones` | Permanent removed-profile identity tombstones |
| `provider_kind_tombstones` | Permanent removed-kind identity tombstones |
| `provider_catalog_state` | Active catalog revision and activation state |
| `provider_catalog_profile_projection` | Current safe catalog profile projection |
| `configuration_audit` | Separate configuration-audit sequence |
| `session_provider_defaults` | Durable per-session future provider defaults |
| `resolved_run_provider_selections` | Persisted `ResolvedRunProviderSelectionDto` per accepted run |
| `unavailable_provider_queue` | Unavailable selections awaiting promotion |
| `unavailable_queue_reconciliation_markers` | Queue-reconciliation-needed markers |
| `provider_usage_aggregates` | Profile-keyed usage aggregates |
| `provider_usage_facts` | Per-run usage facts |
| `provider_catalog_removal_candidates` | Pending-removal candidates |
| `held_recovered_runs` | Held recovery-promoted runs awaiting admission |

## Version and tag linkage

`intention-domain::canonical::TagRegistry` is the sole numeric owner of the
ledger tag values. `intention-protocol` references those constants through
nonserialized `ContractFamilyDescriptor` entries in
`PUBLIC_WIRE_CONTRACT_FAMILIES`; there is no protocol-side numeric mirror. The
protocol parity tests fail on any missing, duplicate, or mismatched tag: the
wire families must cover exactly the 23 wire-ledger tags, may duplicate only
for the versioned fork snapshot and preview families, and must match the domain
registry values by name.
