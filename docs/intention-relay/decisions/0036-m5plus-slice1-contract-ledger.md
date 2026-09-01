# ADR 0036: M5+ Slice 1 contract ledger

## Status

Accepted as the Slice 1 activating specification required by [ADR 0035](0035-m5plus-complete-foundation-activation.md).

## Scope and supersession

This record freezes and activates only M5+ Slice 1, Contracts and versions. It
extends ADR 0035 Slice 1 and does not activate slices 2–4, M6, M7, M8, or M9.

## Version ledger

| Contract | Version/status |
| --- | --- |
| Local protocol | 1.1 |
| Public DTO schema | 1.1, additive |
| TOML configuration schema | 1, unchanged |
| SQLite storage schema | 3, unchanged; no Slice 1 migration |

## Negotiated capabilities and failure semantics

The additive capabilities are `provider_profiles_v1`, `session_fork_v1`,
`normalized_reasoning_stream_v1`, `agent_activity_v1`,
`user_notifications_v1`, `daemon_tool_gateway_v1`, and `model_tool_loop_v1`.
The effective set is the intersection of both peers’ hello capabilities.
Duplicate entries reject with `duplicate_protocol_capability`. Dependent work
fails closed before effect when unsupported, using the family errors
`provider_profiles_capability_required`, `session_fork_capability_required`,
`normalized_reasoning_stream_required`, `agent_activity_capability_required`,
`user_notifications_capability_required`, `daemon_tool_gateway_capability_required`,
`model_tool_loop_required`, and `execution_meaning_capability_required`. There
is no partial contract or partial effect.

## Canonical codec and identity

`intention-domain` owns the canonical codec and semantic canonical records.
The format is `IRCR` / `typed-tlv-v1` / SHA-256. Digest text is
`<namespace>:sha256:<64 lowercase hex>`; new identities are
`sha256-v1:<64 hex>`. A digest excludes its own field. Digest inputs exclude
credentials, paths, display data, readiness, and current state.

## Numeric tag registry

`intention-domain` owns this registry:

| Tag | Value |
| --- | --- |
| `run-execution-meaning` (records v3, v4) | `0x0101` |
| `programmatic-caller-policy-selection-v1` | `0x0201` |
| `agent-activity-selection-v1` | `0x0202` |
| `goal-run-selection-v1` | `0x0203` |
| `continual-harness-selection-v1` | `0x0204` |
| `mcp-method-catalog-selection-v1` | `0x0205` |
| `model-capability-taxonomy-v1` | `0x0206` |
| `provider-profile-revision-v1` | `0x0207` |
| `provider-selection-v1` | `0x0208` |
| `reasoning-history-manifest-v1` | `0x0209` |
| `context-source-manifest-v1` | `0x020A` |
| `model-context-projection-v1` | `0x020B` |
| `legacy-m4-selection-binding` | `0x020C` |
| `tool-descriptor-revision` | `0x0301` |
| `tool-registry-revision` | `0x0302` |
| `model-tool-loop-v1` | `0x0303` |
| `bridge-invocation-v1` | `0x0304` |
| `fork-base-snapshot-v1/v2` | `0x0401` |
| `fork-preview-v1/v2` | `0x0402` |
| `fork-command-v1` | `0x0403` |
| `agent-activity-tree-v1` | `0x0501` |
| `agent-activity-pair-v1` | `0x0502` |
| `agent-message-v1` | `0x0503` |
| `agent-activity-journal-record-v1` | `0x0504` |
| `agent-notification-record-v1` | `0x0505` |

## Execution-meaning records

The historical `run-execution-meaning-v3` field table (tags 1–10) is removed
by [ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md); the single
live record is `run-execution-meaning-v4`, which has tags 1–11 and adds
`agent_activity_selection` at tag 11. Envelope tags 1–6 are, in order,
`execution_kind`, `meaning_record_tag`, `meaning_record_version`,
`canonicalization_version`, `canonical_meaning_bytes`, and
`canonical_meaning_digest`. Execution kinds are closed:
`Ordinary`, `Mandate`, and `VerifierMandate`.

`ProgrammaticCallerPolicySelectionV1` uses tags 1–5. `AgentActivitySelectionV1`
has Root and Descendant six-field variants. Its fixed limits are 1024 messages,
4 MiB aggregate, 4096 journal records, 64 KiB per record, 256 records/512 KiB
per page, 16 references, and a 60-minute clarification wait.

## Ownership and preservation invariants

Semantic canonical records/tags belong to `intention-domain`; public wire and
frames to `intention-protocol`; storage contracts/migrations to
`intention-storage` and `intention-storage-sqlite`; registry/typed tool
contracts to `intention-tools`; provider-private translation to provider
crates; process/publication to `intention-daemon`; concrete assembly to
`intention`; and adapters to `intention-client`, then TUI/Tauri. No new crate,
dependency, feature, coverage tier, or exclusion is introduced. Skeleton
`intention-headroom`, `intention-plans`, `intention-vfr`, and `intention-tauri`
remain untouched, and M6–M9 behavior is untouched.

M3/M4 config revisions, snapshots, sessions, runs, events, cursors, queue
tickets, and bytes remain authoritative and unchanged. Historical runs receive
no synthetic post-M5 records; current state is never reconstructed. The legacy
M4 selection bridge (tag `legacy-m4-selection-binding` 0x020C) is removed by
[ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md); no synthetic
binding is ever materialized for historical runs.

## Evidence and non-goals

Required evidence covers DTO round trips; canonical golden bytes/digests;
compatible-minor, incompatible-major, and unnegotiated fail-closed negotiation
fixtures; M3/M4 preservation; future-schema rejection; fake-secret absence;
and cross-platform determinism. Required gates are `make quick`, `make verify`,
`docs-check`, and Linux/Windows CI.

This ledger does not implement M6–M9 behavior and does not introduce a second
runtime, registry, scheduler, persistence authority, or sandbox.

## Resolution notes

### Runtime version resolution

Protocol 1.1 and public DTO schema 1.1 are now the active advertised versions.
`intention-protocol` defines `CURRENT_PROTOCOL_VERSION` and
`CURRENT_DTO_SCHEMA_VERSION`, and the runtime crates `intention`,
`intention-client`, `intention-transport`, and `intention-daemon` reference
them. TOML configuration schema remains 1 and SQLite storage schema remains 3.
Historical M3/M4 fixtures and committed v1 protocol fixtures remain 1.0 and are
not rewritten. Same-major compatibility (1.0 ↔ 1.1) is preserved by the
existing `ensure_compatible_with` logic, which accepts any peer with an equal
major version and rejects a differing major with `incompatible_protocol_version`.

### Registry tag activation status

The numeric tag registry is owned by `intention-domain`
(`crates/intention-domain/src/canonical.rs`). Every ledger tag is classified in
the `TagRegistry::LEDGER` table by `TagStatus` as `Wired` or
`ReservedForSlice2`. Exactly three tags are `Wired`; every other tag is
`ReservedForSlice2` and carries no production codec in this slice.

| Tag | Value | Status |
| --- | --- | --- |
| `run-execution-meaning` | `0x0101` | Wired |
| `programmatic-caller-policy-selection-v1` | `0x0201` | Wired |
| `agent-activity-selection-v1` | `0x0202` | Wired |
| `goal-run-selection-v1` | `0x0203` | ReservedForSlice2 |
| `continual-harness-selection-v1` | `0x0204` | ReservedForSlice2 |
| `mcp-method-catalog-selection-v1` | `0x0205` | ReservedForSlice2 |
| `model-capability-taxonomy-v1` | `0x0206` | ReservedForSlice2 |
| `provider-profile-revision-v1` | `0x0207` | ReservedForSlice2 |
| `provider-selection-v1` | `0x0208` | ReservedForSlice2 |
| `reasoning-history-manifest-v1` | `0x0209` | ReservedForSlice2 |
| `context-source-manifest-v1` | `0x020A` | ReservedForSlice2 |
| `model-context-projection-v1` | `0x020B` | ReservedForSlice2 |
| `legacy-m4-selection-binding` | `0x020C` | ReservedForSlice2 |
| `tool-descriptor-revision` | `0x0301` | ReservedForSlice2 |
| `tool-registry-revision` | `0x0302` | ReservedForSlice2 |
| `model-tool-loop-v1` | `0x0303` | ReservedForSlice2 |
| `bridge-invocation-v1` | `0x0304` | ReservedForSlice2 |
| `fork-base-snapshot-v1/v2` | `0x0401` | ReservedForSlice2 |
| `fork-preview-v1/v2` | `0x0402` | ReservedForSlice2 |
| `fork-command-v1` | `0x0403` | ReservedForSlice2 |
| `agent-activity-tree-v1` | `0x0501` | ReservedForSlice2 |
| `agent-activity-pair-v1` | `0x0502` | ReservedForSlice2 |
| `agent-message-v1` | `0x0503` | ReservedForSlice2 |
| `agent-activity-journal-record-v1` | `0x0504` | ReservedForSlice2 |
| `agent-notification-record-v1` | `0x0505` | ReservedForSlice2 |

Reserved tags are not active capabilities and are not model-visible.

## Appendix A: Public wire-family field tables

This appendix is the normative field-level reference for the Slice 1 public
wire families. Canonical record families derive from
`crates/intention-domain/src/run_execution_meaning.rs`; public DTO families
derive from `crates/intention-protocol/src/contract_families.rs`. Field tags
apply to canonical typed-TLV records; public DTO families are JSON objects and
carry no numeric field tags (marked `—`). Every listed field is mandatory
unless the Required column says otherwise. No table cell spans multiple lines.

### run-execution-meaning (0x0101)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `run-execution-meaning` (0x0101) | 3 | 1 | selection record 1 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 3 | 2 | selection record 2 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 3 | 3 | selection record 3 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 3 | 4 | selection record 4 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 3 | 5 | selection record 5 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 3 | 6 | selection record 6 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 3 | 7 | selection record 7 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 3 | 8 | selection record 8 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 3 | 9 | selection record 9 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 3 | 10 | selection record 10 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 4 | 1 | selection record 1 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 4 | 2 | selection record 2 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 4 | 3 | selection record 3 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 4 | 4 | selection record 4 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 4 | 5 | selection record 5 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 4 | 6 | selection record 6 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 4 | 7 | selection record 7 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 4 | 8 | selection record 8 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 4 | 9 | selection record 9 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 4 | 10 | selection record 10 | Record | Yes | Mandatory nested M3/M4 selection record; owner-defined semantics; opaque to the Slice 1 codec |
| `run-execution-meaning` (0x0101) | 4 | 11 | agent_activity_selection | Record | Yes | v4-only addition; nested `AgentActivitySelectionV1` canonical record (0x0202) |
| `run-execution-meaning envelope` (0x0102) | 1 | 1 | execution_kind | U64 | Yes | Closed `ExecutionKind`: Ordinary (0), Mandate (1), VerifierMandate (2) |
| `run-execution-meaning envelope` (0x0102) | 1 | 2 | meaning_record_tag | U64 | Yes | Canonical record tag, `0x0101` |
| `run-execution-meaning envelope` (0x0102) | 1 | 3 | meaning_record_version | U64 | Yes | Single live record version 4 (v3 removed by ADR 0038) |
| `run-execution-meaning envelope` (0x0102) | 1 | 4 | canonicalization_version | U64 | Yes | 1 |
| `run-execution-meaning envelope` (0x0102) | 1 | 5 | canonical_meaning_bytes | Bytes | Yes | Exact canonical meaning record bytes |
| `run-execution-meaning envelope` (0x0102) | 1 | 6 | canonical_meaning_digest | Digest | Yes | SHA-256 of field 5; mismatch rejects with `DigestMismatch` |

### programmatic-caller-policy-selection-v1 (0x0201)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 1 | root_origin | U64 | Yes | Closed `ExecutionKind`: Ordinary (0), Mandate (1), VerifierMandate (2) |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 2 | effective_policy_snapshot_reference | Uuid | Yes |  |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 3 | policy_selection_digest | Digest | Yes | SHA-256 digest |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 4 | inherited_scope_provenance | List | Yes | Ordered list of UUIDs |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 5 | fixed_run_limits | Record | Yes | Nested `FixedRunLimits` record; tags 1-6 below |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 1 | fixed_run_limits.max_attempts | U64 | Yes | Nested field |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 2 | fixed_run_limits.max_total_seconds | U64 | Yes | Nested field |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 3 | fixed_run_limits.max_actions | U64 | Yes | Nested field |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 4 | fixed_run_limits.max_concurrent_actions | U64 | Yes | Nested field |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 5 | fixed_run_limits.max_retained_bytes | U64 | Yes | Nested field |
| `programmatic-caller-policy-selection-v1` (0x0201) | 1 | 6 | fixed_run_limits.max_clarification_seconds | U64 | Yes | Nested field |

### agent-activity-selection-v1 (0x0202)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `agent-activity-selection-v1` (0x0202) | 1 | 1 | activity_tree_id | Uuid | Yes | Root variant |
| `agent-activity-selection-v1` (0x0202) | 1 | 2 | root_origin | U64 | Yes | Root variant; closed `ExecutionKind` |
| `agent-activity-selection-v1` (0x0202) | 1 | 3 | activity_exchange_revision | U64 | Yes | Root variant |
| `agent-activity-selection-v1` (0x0202) | 1 | 4 | activity_journal_revision | U64 | Yes | Root variant |
| `agent-activity-selection-v1` (0x0202) | 1 | 5 | user_projection_revision | U64 | Yes | Root variant |
| `agent-activity-selection-v1` (0x0202) | 1 | 6 | fixed_activity_limits | Record | Yes | Nested `FixedActivityLimits` record; tags 1-8 below |
| `agent-activity-selection-v1` (0x0202) | 2 | 1 | activity_tree_id | Uuid | Yes | Descendant variant |
| `agent-activity-selection-v1` (0x0202) | 2 | 2 | direct_parent_link_reference | Uuid | Yes | Descendant variant |
| `agent-activity-selection-v1` (0x0202) | 2 | 3 | activity_exchange_revision | U64 | Yes | Descendant variant |
| `agent-activity-selection-v1` (0x0202) | 2 | 4 | activity_journal_revision | U64 | Yes | Descendant variant |
| `agent-activity-selection-v1` (0x0202) | 2 | 5 | user_projection_revision | U64 | Yes | Descendant variant |
| `agent-activity-selection-v1` (0x0202) | 2 | 6 | fixed_activity_limits | Record | Yes | Nested `FixedActivityLimits` record; tags 1-8 below |
| `agent-activity-selection-v1` (0x0202) | 1/2 | 1 | fixed_activity_limits.max_messages | U64 | Yes | Frozen: 1024 |
| `agent-activity-selection-v1` (0x0202) | 1/2 | 2 | fixed_activity_limits.max_aggregate_bytes | U64 | Yes | Frozen: 4 MiB |
| `agent-activity-selection-v1` (0x0202) | 1/2 | 3 | fixed_activity_limits.max_journal_records | U64 | Yes | Frozen: 4096 |
| `agent-activity-selection-v1` (0x0202) | 1/2 | 4 | fixed_activity_limits.max_record_bytes | U64 | Yes | Frozen: 64 KiB |
| `agent-activity-selection-v1` (0x0202) | 1/2 | 5 | fixed_activity_limits.max_page_records | U64 | Yes | Frozen: 256 |
| `agent-activity-selection-v1` (0x0202) | 1/2 | 6 | fixed_activity_limits.max_page_bytes | U64 | Yes | Frozen: 512 KiB |
| `agent-activity-selection-v1` (0x0202) | 1/2 | 7 | fixed_activity_limits.max_typed_references | U64 | Yes | Frozen: 16 |
| `agent-activity-selection-v1` (0x0202) | 1/2 | 8 | fixed_activity_limits.max_clarification_wait_seconds | U64 | Yes | Frozen: 3600 (60 minutes) |

### goal-run-selection-v1 (0x0203)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `goal-run-selection-v1` (0x0203) | — | — | — | — | — | Domain-owned canonical family; wire linkage via `PUBLIC_WIRE_CONTRACT_FAMILIES`; no separate protocol DTO in Slice 1; ledger status ReservedForSlice2; no field table in this slice |

### continual-harness-selection-v1 (0x0204)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `continual-harness-selection-v1` (0x0204) | — | — | — | — | — | Domain-owned canonical family; wire linkage via `PUBLIC_WIRE_CONTRACT_FAMILIES`; no separate protocol DTO in Slice 1; ledger status ReservedForSlice2; no field table in this slice |

### mcp-method-catalog-selection-v1 (0x0205)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `mcp-method-catalog-selection-v1` (0x0205) | — | — | — | — | — | Domain-owned canonical family; wire linkage via `PUBLIC_WIRE_CONTRACT_FAMILIES`; no separate protocol DTO in Slice 1; ledger status ReservedForSlice2; no field table in this slice |

### model-capability-taxonomy-v1 (0x0206)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `model-capability-taxonomy-v1` (0x0206) | — | — | — | — | — | Domain-owned canonical family; wire linkage via `PUBLIC_WIRE_CONTRACT_FAMILIES`; no separate protocol DTO in Slice 1; ledger status ReservedForSlice2; no field table in this slice |

### provider-profile-revision-v1 (0x0207)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `provider-profile-revision-v1` (0x0207) | 1 | — | ProviderProfileRevisionV1.profile_id | String | Yes | ≤ 256 scalar values; `provider_profile_revision_invalid` otherwise |
| `provider-profile-revision-v1` (0x0207) | 1 | — | ProviderProfileRevisionV1.revision_id | String | Yes | ≤ 256 scalar values |
| `provider-profile-revision-v1` (0x0207) | 1 | — | ProviderProfileRevisionV1.provider_kind_id | String | Yes | ≤ 256 scalar values |
| `provider-profile-revision-v1` (0x0207) | 1 | — | ProviderProfileRevisionV1.model_id | String | Yes | ≤ 256 scalar values |
| `provider-profile-revision-v1` (0x0207) | 1 | — | ProviderProfileRevisionV1.endpoint | String | Yes | No userinfo, query, or fragment; `invalid_endpoint` otherwise |
| `provider-profile-revision-v1` (0x0207) | 1 | — | ProviderProfileRevisionV1.credential_transport_mode | CredentialTransportMode | Yes | Closed: `Bearer` or `SafeHeader` |
| `provider-profile-revision-v1` (0x0207) | 1 | — | ProviderProfileRevisionV1.safe_header_name | Option<String> | No | ≤ 128 scalar values when present |
| `provider-profile-revision-v1` (0x0207) | 1 | — | ProviderProfileRevisionV1.capability_taxonomy_revision | String | Yes | ≤ 256 scalar values |
| `provider-profile-revision-v1` (0x0207) | 1 | — | ProviderProfileRevisionV1.reasoning_compatibility_id | Option<String> | No | ≤ 256 scalar values when present |

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
| `provider-selection-v1` (0x0208) | 1 | — | ResolvedRunProviderSelectionDto.selection_source | Option<String> | No |  |

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
| `model-context-projection-v1` (0x020B) | 1 | — | ModelContextProjectionV1.ordered_messages | Vec<String> | Yes | 1..=1024 nonblank entries; ≤ 1 MiB aggregate; `model_context_projection_invalid`/`_too_large` |
| `model-context-projection-v1` (0x020B) | 1 | — | ModelContextProjectionV1.model_context_digest | String | Yes | 64 lowercase hex |

### legacy-m4-selection-binding (0x020C)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `legacy-m4-selection-binding` (0x020C) | 1 | — | LegacyM4SelectionBindingDto.legacy_config_revision_id | String | Yes |  |
| `legacy-m4-selection-binding` (0x020C) | 1 | — | LegacyM4SelectionBindingDto.legacy_snapshot_schema | String | Yes |  |
| `legacy-m4-selection-binding` (0x020C) | 1 | — | LegacyM4SelectionBindingDto.legacy_safe_selection | String | Yes | Normative invariant: must be `legacy-uuid:<canonical UUID>`; legacy bytes are never rewritten; `legacy_selection_reference_invalid` otherwise |
| `legacy-m4-selection-binding` (0x020C) | 1 | — | LegacyM4SelectionBindingDto.default_profile_id | String | Yes |  |
| `legacy-m4-selection-binding` (0x020C) | 1 | — | LegacyM4SelectionBindingDto.default_profile_revision_id | String | Yes |  |
| `legacy-m4-selection-binding` (0x020C) | 1 | — | LegacyM4SelectionBindingDto.kind_descriptor_revision_id | String | Yes |  |
| `legacy-m4-selection-binding` (0x020C) | 1 | — | LegacyM4SelectionBindingDto.capability_subset | Vec<String> | Yes |  |
| `legacy-m4-selection-binding` (0x020C) | 1 | — | LegacyM4SelectionBindingDto.execution_policy | String | Yes |  |
| `legacy-m4-selection-binding` (0x020C) | 1 | — | LegacyM4SelectionBindingDto.driver_contract_revision | String | Yes |  |

### tool-descriptor-revision (0x0301)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `tool-descriptor-revision` (0x0301) | 1 | — | ToolDescriptorRevision.tool_id | String | Yes | ≤ 256 scalar values; credential-shaped values reject with `credentials_forbidden` |
| `tool-descriptor-revision` (0x0301) | 1 | — | ToolDescriptorRevision.descriptor_revision | String | Yes | ≤ 256 scalar values |
| `tool-descriptor-revision` (0x0301) | 1 | — | ToolDescriptorRevision.intended_owner | String | Yes | ≤ 256 scalar values |
| `tool-descriptor-revision` (0x0301) | 1 | — | ToolDescriptorRevision.input_schema_reference | String | Yes | ≤ 256 scalar values |
| `tool-descriptor-revision` (0x0301) | 1 | — | ToolDescriptorRevision.result_schema_reference | String | Yes | ≤ 256 scalar values |
| `tool-descriptor-revision` (0x0301) | 1 | — | ToolDescriptorRevision.required_capability_binding | String | Yes | ≤ 256 scalar values |
| `tool-descriptor-revision` (0x0301) | 1 | — | ToolDescriptorRevision.mode_relation | String | Yes | ≤ 256 scalar values |
| `tool-descriptor-revision` (0x0301) | 1 | — | ToolDescriptorRevision.model_function_schema_revision | String | Yes | ≤ 256 scalar values |
| `tool-descriptor-revision` (0x0301) | 1 | — | ToolDescriptorRevision.safe_result_projection_revision | String | Yes | ≤ 256 scalar values |
| `tool-descriptor-revision` (0x0301) | 1 | — | ToolDescriptorRevision.observation_contract_revision | String | Yes | ≤ 256 scalar values |
| `tool-descriptor-revision` (0x0301) | 1 | — | ToolDescriptorRevision.stream_shape | String | Yes | ≤ 256 scalar values |

### tool-registry-revision (0x0302)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `tool-registry-revision` (0x0302) | 1 | — | ToolRegistryRevision.registry_revision_id | String | Yes |  |
| `tool-registry-revision` (0x0302) | 1 | — | ToolRegistryRevision.descriptors | Vec<ToolDescriptorRevision> | Yes | ≤ 256 descriptors; ≤ 512 KiB aggregate; duplicate tool IDs reject with `duplicate_tool_descriptor` |
| `tool-registry-revision` (0x0302) | 1 | — | ToolRegistryRevision.admission_engine_revision | String | Yes |  |
| `tool-registry-revision` (0x0302) | 1 | — | ToolRegistryRevision.hook_pipeline_revision | String | Yes |  |

### model-tool-loop-v1 (0x0303)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `model-tool-loop-v1` (0x0303) | 1 | — | ModelToolLoopV1.tool_registry_revision_id | String | Yes |  |
| `model-tool-loop-v1` (0x0303) | 1 | — | ModelToolLoopV1.admission_engine_revision | String | Yes |  |
| `model-tool-loop-v1` (0x0303) | 1 | — | ModelToolLoopV1.hook_pipeline_revision | String | Yes |  |
| `model-tool-loop-v1` (0x0303) | 1 | — | ModelToolLoopV1.active_descriptors | Vec<ActiveToolDescriptorSelectionV1> | Yes | One entry per active descriptor |
| `model-tool-loop-v1` (0x0303) | 1 | — | ModelToolLoopV1.model_tool_loop_required | bool | Yes |  |
| `model-tool-loop-v1` (0x0303) | 1 | — | ModelToolLoopV1.translation_revision | String | Yes |  |
| `model-tool-loop-v1` (0x0303) | 1 | — | ModelToolLoopV1.stream_shape | String | Yes |  |
| `model-tool-loop-v1` (0x0303) | 1 | — | ActiveToolDescriptorSelectionV1.tool_id | String | Yes | Nested descriptor |
| `model-tool-loop-v1` (0x0303) | 1 | — | ActiveToolDescriptorSelectionV1.intended_owner | String | Yes | Nested descriptor |
| `model-tool-loop-v1` (0x0303) | 1 | — | ActiveToolDescriptorSelectionV1.descriptor_revision | String | Yes | Nested descriptor |
| `model-tool-loop-v1` (0x0303) | 1 | — | ActiveToolDescriptorSelectionV1.input_schema_reference | String | Yes | Nested descriptor |
| `model-tool-loop-v1` (0x0303) | 1 | — | ActiveToolDescriptorSelectionV1.result_schema_reference | String | Yes | Nested descriptor |
| `model-tool-loop-v1` (0x0303) | 1 | — | ActiveToolDescriptorSelectionV1.required_capability_binding | String | Yes | Nested descriptor |
| `model-tool-loop-v1` (0x0303) | 1 | — | ActiveToolDescriptorSelectionV1.mode_relation | String | Yes | Nested descriptor |
| `model-tool-loop-v1` (0x0303) | 1 | — | ActiveToolDescriptorSelectionV1.model_function_schema_revision | String | Yes | Nested descriptor |
| `model-tool-loop-v1` (0x0303) | 1 | — | ActiveToolDescriptorSelectionV1.safe_result_projection_revision | String | Yes | Nested descriptor |
| `model-tool-loop-v1` (0x0303) | 1 | — | ActiveToolDescriptorSelectionV1.observation_contract_revision | String | Yes | Nested descriptor |
| `model-tool-loop-v1` (0x0303) | 1 | — | ActiveToolDescriptorSelectionV1.stream_shape | String | Yes | Nested descriptor |
| `model-tool-loop-v1` (0x0303) | 1 | — | ModelToolExchangeDto.assistant_ordered_calls | Vec<String> | Yes | At most 16 calls per group; `provider_tool_group_invalid` otherwise |
| `model-tool-loop-v1` (0x0303) | 1 | — | ModelToolExchangeDto.canonical_call_identities | Vec<String> | Yes |  |
| `model-tool-loop-v1` (0x0303) | 1 | — | ModelToolExchangeDto.completed_result_records | Vec<String> | Yes |  |
| `model-tool-loop-v1` (0x0303) | 1 | — | ModelToolExchangeDto.safe_model_visible_projections | Vec<String> | Yes |  |

The tool-call group rule is closed: at most 16 assistant-ordered calls per
group, and any group shape violating the rule fails before local effect with
`provider_tool_group_invalid`. The terminal outcome set is closed to exactly 8
values: `Succeeded`, `DeniedBeforeExecution`, `FailedBeforeExternalEffect`,
`CancelledBeforeStart`, `InterruptedBeforeStart`, `OutputLimitExceeded`,
`ExecutionUnavailable`, and `ExternalEffectUnknown`.

### bridge-invocation-v1 (0x0304)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeOperationV1.bridge_operation_id | String | Yes |  |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeOperationV1.run_id | String | Yes |  |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeOperationV1.mandate_id | String | Yes |  |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeOperationV1.mandate_revision | String | Yes |  |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeOperationV1.model_step_id | String | Yes |  |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeOperationV1.tool_id | String | Yes |  |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeOperationV1.descriptor_revision | String | Yes |  |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeOperationV1.typed_input_digest | String | Yes | 64 lowercase hex |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeOperationV1.tool_call_id | String | Yes |  |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeOperationV1.admission_outcome | String | Yes |  |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeOperationV1.attempt_reference | Option<String> | No |  |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeRunGrantDto.opaque_grant_identity | String | Yes | Supporting DTO |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeRunGrantDto.issued_protocol_revision | String | Yes | Supporting DTO |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeAttachmentResponseDto.bridge_run_grant | BridgeRunGrantDto | Yes | Supporting DTO |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeAttachmentResponseDto.negotiated_capabilities | Vec<ProtocolCapabilityDto> | Yes | Supporting DTO |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeAttachmentResponseDto.initial_run_cursor | u64 | Yes | Supporting DTO |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeInvocationCommandDto.bridge_run_grant | BridgeRunGrantDto | Yes | Supporting DTO |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeInvocationCommandDto.bridge_operation_id | String | Yes | Supporting DTO |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeInvocationCommandDto.typed_tool_invocation | String | Yes | Supporting DTO |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeInvocationAcceptedDto.bridge_operation_id | String | Yes | Supporting DTO |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeInvocationAcceptedDto.tool_call_id | String | Yes | Supporting DTO |
| `bridge-invocation-v1` (0x0304) | 1 | — | BridgeInvocationAcceptedDto.admission_state | String | Yes | Supporting DTO |

### fork-base-snapshot-v1/v2 (0x0401)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `fork-base-snapshot-v1/v2` (0x0401) | 1 | — | ForkBaseSnapshotV1.schema_version | String | Yes |  |
| `fork-base-snapshot-v1/v2` (0x0401) | 1 | — | ForkBaseSnapshotV1.context_schema_version | String | Yes |  |
| `fork-base-snapshot-v1/v2` (0x0401) | 1 | — | ForkBaseSnapshotV1.source_session_id | String | Yes |  |
| `fork-base-snapshot-v1/v2` (0x0401) | 1 | — | ForkBaseSnapshotV1.conversation_tree_id | String | Yes |  |
| `fork-base-snapshot-v1/v2` (0x0401) | 1 | — | ForkBaseSnapshotV1.boundary | ForkBoundary | Yes | Closed: `CommittedUserTurn` or `CompletedAssistantTurn` |
| `fork-base-snapshot-v1/v2` (0x0401) | 1 | — | ForkBaseSnapshotV1.source_boundary_sequence | u64 | Yes |  |
| `fork-base-snapshot-v1/v2` (0x0401) | 1 | — | ForkBaseSnapshotV1.source_run_cursors | Vec<u64> | Yes |  |
| `fork-base-snapshot-v1/v2` (0x0401) | 1 | — | ForkBaseSnapshotV1.effective_instruction_projection | String | Yes |  |
| `fork-base-snapshot-v1/v2` (0x0401) | 1 | — | ForkBaseSnapshotV1.materialized_model_messages | Vec<String> | Yes | ≤ 1,024 entries and ≤ 1 MiB aggregate; `fork_snapshot_too_large` otherwise |
| `fork-base-snapshot-v1/v2` (0x0401) | 1 | — | ForkBaseSnapshotV1.inherited_future_defaults | Vec<String> | Yes |  |
| `fork-base-snapshot-v1/v2` (0x0401) | 1 | — | ForkBaseSnapshotV1.historical_config_policy_references | Vec<String> | Yes |  |
| `fork-base-snapshot-v1/v2` (0x0401) | 1 | — | ForkBaseSnapshotV1.safe_usage_provenance | Vec<String> | Yes |  |
| `fork-base-snapshot-v1/v2` (0x0401) | 1 | — | ForkBaseSnapshotV1.terminal_tool_result_references | Vec<String> | Yes |  |
| `fork-base-snapshot-v1/v2` (0x0401) | 1 | — | ForkBaseSnapshotV1.policy_decision_references | Vec<String> | Yes |  |
| `fork-base-snapshot-v1/v2` (0x0401) | 1 | — | ForkBaseSnapshotV1.terminal_child_result_references | Vec<String> | Yes |  |
| `fork-base-snapshot-v1/v2` (0x0401) | 1 | — | ForkBaseSnapshotV1.workspace_state | String | Yes | Closed value `unverified`; `fork_snapshot_unsupported` otherwise |
| `fork-base-snapshot-v1/v2` (0x0401) | 2 | — | ForkBaseSnapshotV2.inherited_reasoning_history_references | Vec<String> | Yes | Sole additive v2 field; ≤ 4,096 references and ≤ 1 MiB aggregate; `fork_snapshot_too_large`/`fork_reference_unavailable` otherwise |

### fork-preview-v1/v2 (0x0402)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `fork-preview-v1/v2` (0x0402) | 1 | — | ForkPreviewV1.preview_digest | String | Yes | 64 lowercase hex |
| `fork-preview-v1/v2` (0x0402) | 1 | — | ForkPreviewV1.source_head_sequence | u64 | Yes |  |
| `fork-preview-v1/v2` (0x0402) | 1 | — | ForkPreviewV1.page_count | u32 | Yes | ≤ 64 pages; `fork_snapshot_too_large` otherwise |
| `fork-preview-v1/v2` (0x0402) | 1 | — | ForkPreviewV1.snapshot_size_bytes | u64 | Yes | ≤ 1 MiB; `fork_snapshot_too_large` otherwise |
| `fork-preview-v1/v2` (0x0402) | 1 | — | ForkPreviewV1.workspace_state | String | Yes | Closed value `unverified`; `fork_snapshot_unsupported` otherwise |
| `fork-preview-v1/v2` (0x0402) | 2 | — | ForkPreviewV2.inherited_reasoning_history_references | Vec<String> | Yes | Sole additive v2 field; ≤ 4,096 references and ≤ 1 MiB aggregate; `fork_snapshot_too_large`/`fork_reference_unavailable` otherwise |

### fork-command-v1 (0x0403)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `fork-command-v1` (0x0403) | 1 | — | ForkSessionCommandDto.source_session_id | String | Yes |  |
| `fork-command-v1` (0x0403) | 1 | — | ForkSessionCommandDto.boundary | ForkBoundary | Yes | Closed: `CommittedUserTurn { source_turn_id, accepted_sequence }` or `CompletedAssistantTurn { source_run_id, final_assistant_turn_id, completed_sequence, final_run_cursor }` |
| `fork-command-v1` (0x0403) | 1 | — | ForkSessionCommandDto.expected_source_sequence | u64 | Yes |  |
| `fork-command-v1` (0x0403) | 1 | — | ForkSessionCommandDto.expected_preview_digest | String | Yes | 64 lowercase hex; `invalid_digest` otherwise |
| `fork-command-v1` (0x0403) | 1 | — | ForkSessionCommandDto.title_present | bool | Yes |  |
| `fork-command-v1` (0x0403) | 1 | — | ForkSessionCommandDto.requested_title | Option<String> | No | ≤ 128 scalar values; `invalid_title` otherwise |
| `fork-command-v1` (0x0403) | 1 | — | ForkSessionCommandDto.future_profile_override_present | bool | Yes |  |
| `fork-command-v1` (0x0403) | 1 | — | ForkSessionCommandDto.future_profile_override | Option<String> | No |  |

### agent-activity-tree-v1 (0x0501)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `agent-activity-tree-v1` (0x0501) | 1 | — | AgentActivityTreeV1.activity_tree_id | AgentActivityTreeId | Yes | String newtype |
| `agent-activity-tree-v1` (0x0501) | 1 | — | AgentActivityTreeV1.root_run_reference | String | Yes | ≤ 256 scalar values |
| `agent-activity-tree-v1` (0x0501) | 1 | — | AgentActivityTreeV1.activity_exchange_revision | String | Yes | ≤ 256 scalar values |
| `agent-activity-tree-v1` (0x0501) | 1 | — | AgentActivityTreeV1.activity_journal_revision | String | Yes | ≤ 256 scalar values |
| `agent-activity-tree-v1` (0x0501) | 1 | — | AgentActivityTreeV1.user_projection_revision | String | Yes | ≤ 256 scalar values |
| `agent-activity-tree-v1` (0x0501) | 1 | — | AgentActivityTreeV1.fixed_limits | AgentActivityLimitsV1 | Yes | Nested; frozen values below; any departure rejects with `agent_activity_limits_invalid` |
| `agent-activity-tree-v1` (0x0501) | 1 | — | AgentActivityLimitsV1.max_messages | u32 | Yes | Frozen: 1024 |
| `agent-activity-tree-v1` (0x0501) | 1 | — | AgentActivityLimitsV1.max_aggregate_bytes | u64 | Yes | Frozen: 4 MiB |
| `agent-activity-tree-v1` (0x0501) | 1 | — | AgentActivityLimitsV1.max_journal_records | u32 | Yes | Frozen: 4096 |
| `agent-activity-tree-v1` (0x0501) | 1 | — | AgentActivityLimitsV1.max_record_bytes | u32 | Yes | Frozen: 64 KiB |
| `agent-activity-tree-v1` (0x0501) | 1 | — | AgentActivityLimitsV1.max_page_records | u32 | Yes | Frozen: 256 |
| `agent-activity-tree-v1` (0x0501) | 1 | — | AgentActivityLimitsV1.max_page_bytes | u64 | Yes | Frozen: 512 KiB |
| `agent-activity-tree-v1` (0x0501) | 1 | — | AgentActivityLimitsV1.max_references | u32 | Yes | Frozen: 16 |

### agent-activity-pair-v1 (0x0502)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `agent-activity-pair-v1` (0x0502) | 1 | — | AgentActivityPairV1.pair_id | String | Yes | ≤ 256 scalar values |
| `agent-activity-pair-v1` (0x0502) | 1 | — | AgentActivityPairV1.activity_tree_id | AgentActivityTreeId | Yes | String newtype |
| `agent-activity-pair-v1` (0x0502) | 1 | — | AgentActivityPairV1.parent_run_reference | String | Yes | ≤ 256 scalar values |
| `agent-activity-pair-v1` (0x0502) | 1 | — | AgentActivityPairV1.child_run_reference | String | Yes | Must differ from parent; `agent_activity_pair_invalid` otherwise |
| `agent-activity-pair-v1` (0x0502) | 1 | — | AgentActivityPairV1.activity_exchange_revision | String | Yes | ≤ 256 scalar values |
| `agent-activity-pair-v1` (0x0502) | 1 | — | AgentActivityPairV1.activity_journal_revision | String | Yes | ≤ 256 scalar values |
| `agent-activity-pair-v1` (0x0502) | 1 | — | AgentActivityPairV1.user_projection_revision | String | Yes | ≤ 256 scalar values |
| `agent-activity-pair-v1` (0x0502) | 1 | — | AgentActivityPairV1.fixed_limits | AgentActivityLimitsV1 | Yes | Same frozen values as 0x0501 |

### agent-message-v1 (0x0503)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `agent-message-v1` (0x0503) | 1 | — | AgentMessageDto.message_id | String | Yes |  |
| `agent-message-v1` (0x0503) | 1 | — | AgentMessageDto.activity_tree_id | AgentActivityTreeId | Yes | String newtype |
| `agent-message-v1` (0x0503) | 1 | — | AgentMessageDto.pair_id | String | Yes |  |
| `agent-message-v1` (0x0503) | 1 | — | AgentMessageDto.pair_order | u32 | Yes |  |
| `agent-message-v1` (0x0503) | 1 | — | AgentMessageDto.direction | AgentMessageDirection | Yes | Closed: `ParentToChild` or `ChildToParent` |
| `agent-message-v1` (0x0503) | 1 | — | AgentMessageDto.kind | AgentMessageKind | Yes | Closed: `Instruction`, `Report`, `ClarificationRequest`, or `ClarificationReply` |
| `agent-message-v1` (0x0503) | 1 | — | AgentMessageDto.sender_run_reference | String | Yes |  |
| `agent-message-v1` (0x0503) | 1 | — | AgentMessageDto.recipient_run_reference | String | Yes |  |
| `agent-message-v1` (0x0503) | 1 | — | AgentMessageDto.source_model_step_reference | String | Yes |  |
| `agent-message-v1` (0x0503) | 1 | — | AgentMessageDto.safe_text | Option<String> | No |  |
| `agent-message-v1` (0x0503) | 1 | — | AgentMessageDto.typed_references | Vec<String> | Yes | ≤ 16; `agent_message_reference_invalid` otherwise |
| `agent-message-v1` (0x0503) | 1 | — | AgentMessageDto.delivery_state | String | Yes |  |
| `agent-message-v1` (0x0503) | 1 | — | AgentMessageDto.canonical_message_digest | String | Yes | 64 lowercase hex |

### agent-activity-journal-record-v1 (0x0504)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `agent-activity-journal-record-v1` (0x0504) | 1 | — | AgentActivityJournalRecordDto.activity_tree_id | AgentActivityTreeId | Yes | String newtype |
| `agent-activity-journal-record-v1` (0x0504) | 1 | — | AgentActivityJournalRecordDto.record_id | String | Yes |  |
| `agent-activity-journal-record-v1` (0x0504) | 1 | — | AgentActivityJournalRecordDto.sequence | u64 | Yes | Zero-based; must be < 4096; `agent_activity_journal_limit_exceeded` otherwise |
| `agent-activity-journal-record-v1` (0x0504) | 1 | — | AgentActivityJournalRecordDto.occurred_at | u64 | Yes |  |
| `agent-activity-journal-record-v1` (0x0504) | 1 | — | AgentActivityJournalRecordDto.root_run_reference | String | Yes |  |
| `agent-activity-journal-record-v1` (0x0504) | 1 | — | AgentActivityJournalRecordDto.direct_pair_reference_when_present | Option<String> | No |  |
| `agent-activity-journal-record-v1` (0x0504) | 1 | — | AgentActivityJournalRecordDto.record_kind | String | Yes |  |
| `agent-activity-journal-record-v1` (0x0504) | 1 | — | AgentActivityJournalRecordDto.safe_user_projection | String | Yes |  |
| `agent-activity-journal-record-v1` (0x0504) | 1 | — | AgentActivityJournalRecordDto.typed_references | Vec<String> | Yes | ≤ 16; `agent_message_reference_invalid` otherwise |
| `agent-activity-journal-record-v1` (0x0504) | 1 | — | AgentActivityJournalRecordDto.canonical_record_digest | String | Yes | 64 lowercase hex |

The journal record string payload is bounded at 64 KiB
(`agent_activity_record_too_large`).

### agent-notification-record-v1 (0x0505)

| Family | Version | Field tag | Field | Type | Required | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| `agent-notification-record-v1` (0x0505) | 1 | — | AgentNotificationRecordDto.notification_cursor | AgentNotificationCursorDto | Yes | u64 newtype |
| `agent-notification-record-v1` (0x0505) | 1 | — | AgentNotificationRecordDto.activity_tree_id | AgentActivityTreeId | Yes | String newtype |
| `agent-notification-record-v1` (0x0505) | 1 | — | AgentNotificationRecordDto.activity_record_reference | String | Yes |  |
| `agent-notification-record-v1` (0x0505) | 1 | — | AgentNotificationRecordDto.level | NotificationLevel | Yes | Closed: `Urgent` or `Ordinary` |
| `agent-notification-record-v1` (0x0505) | 1 | — | AgentNotificationRecordDto.reason | String | Yes |  |
| `agent-notification-record-v1` (0x0505) | 1 | — | AgentNotificationRecordDto.safe_counts_and_states | String | Yes |  |
| `agent-notification-record-v1` (0x0505) | 1 | — | AgentNotificationRecordDto.occurred_at | u64 | Yes |  |

The notification text payload
(`activity_record_reference` + `reason` + `safe_counts_and_states`) is bounded
at 64 KiB (`agent_notification_summary_too_large`).

## Version and tag linkage

`intention-domain::canonical::TagRegistry` is the sole numeric owner of the
ledger tag values. `intention-protocol` references those constants through
nonserialized `ContractFamilyDescriptor` entries in
`PUBLIC_WIRE_CONTRACT_FAMILIES`; there is no protocol-side numeric mirror. The
protocol parity tests fail on any missing, duplicate, or mismatched tag: the
wire families must cover exactly the 23 wire-ledger tags, may duplicate only
for the versioned fork snapshot and preview families, and must match the
domain registry values by name.
