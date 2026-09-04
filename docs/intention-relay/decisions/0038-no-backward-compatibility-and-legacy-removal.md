# ADR 0038: No backward compatibility and legacy removal

## Status

Accepted as the superseding specification that removes all backward-compatibility,
legacy, fallback, and migration machinery from the project, in accordance with the
[AGENTS.md](../../../AGENTS.md) "No backward compatibility" policy (main commit
`b5fa71e`). It is subordinate to [ADR 0035](0035-m5plus-complete-foundation-activation.md),
which remains the activation home and slice-sequence authority, and it supersedes
the specific preservation and migration commitments listed in the inventory below.

## Scope and supersession

This record activates the single-version policy for every versioned system in the
project and schedules the removal of every execution path whose only purpose is
compatibility with a version that is no longer current.

Rationale (binding, from AGENTS.md):

- Backward compatibility is neither required nor in demand. Nothing built or run
  from this project exists outside the development machine: no deployed users, no
  externally persisted data, and no third-party consumers.
- Outdated execution paths are removed. Compatibility layers, fallback branches,
  and migration paths are not added to keep old behavior readable, replayable, or
  upgradeable.
- Until every roadmap milestone is complete and fully closed, each versioned
  system keeps exactly one live version, version 1. Database schemas, protocol
  versions, wire formats, configuration formats, and storage formats evolve in
  place within that single version.
- The existence of databases is not an argument for keeping DB migrations or
  old-schema compatibility.
- Compatibility fixtures, golden files, and tests that assert behavior of any
  version other than the current one are not maintained.

### Superseded commitments

| Superseded commitment | Documented in | Replaced by |
| --- | --- | --- |
| SQLite schema 3 to 4 additive migration, `user_version` tracking, and future-schema rejection | ADR 0036 "Version ledger"; ADR 0037 "Version ledger"; architecture 04; roadmap Slice 1/2 rows | One live schema (logical version 1) created directly on open; no migration chain, no `user_version` gate, no opening of older schemas |
| M3/M4 byte-preservation evidence (schema-3 reopen fixtures, migration rollback fixtures, `TEST_SCHEMA_3_SQL`, standalone `M3_SCHEMA_SQL` fixture block) | ADR 0037 evidence; architecture 04; roadmap; reconciliation EVD-045/EVD-058, SL2-006 | Removed with the migration machinery; current-schema round-trip tests remain |
| Legacy M4 selection bridge (tag `legacy-m4-selection-binding` 0x020C; `LegacyM4SelectionBindingDto`; `legacy_m4_selection_bindings` table; `LegacyBindingRepositoryDto`; `load_config_revision_records`; application `LegacyM4Bridge`; composition `SnapshotBindingSource` mirror derivation) | ADR 0036 ownership/preservation invariants; ADR 0037 numeric tag registry and Appendix tables; ADR 0028 invariant 7; architecture 22 "Legacy M4 selection bridge" section | Removed entirely; tag 0x020C returns to unallocated and is removed from the ledger and `PUBLIC_WIRE_CONTRACT_FAMILIES`; no synthetic bindings are ever materialized |
| Protocol same-major compatibility (1.0 to 1.1) via `ensure_compatible_with` on protocol and DTO schema versions | ADR 0036/0037 resolution notes; architecture 02; compatibility-register | Exact current-version equality (1.1) on negotiation; no minor tolerance |
| 1.0 wire fixtures and legacy-shape deserializers (`ProtocolAcceptedDto`/`SessionSnapshotDto` additive-field tolerance, `protocol_fixtures` target, error-v1 legacy fixture, `hello-compatible-minor-v1.json` naming) | ADR 0036; architecture 02 additive-field policy; closeout m0-m1; compatibility-register | Current-version fixtures only; additive fields become required on the wire |
| TOML configuration v0 migration (`migrate_v0`, `RawV0Config`, `RawV0ModelConfig`, `model.api_key` credential fallback, `collect_v0_issues`) | architecture 09 "TOML parsing, migrations"; roadmap | Unversioned documents fail closed (`invalid_config_schema`); only the current `[provider]` shape parses |
| Historical reasoning wire defaults (uncategorized `ReasoningDelta` decoding as `Primary`) in domain and model crates | ADR 0028 invariant 8; architecture 22 | `category` is required on the wire; no defaulting |
| Deprecated Chat Completions `function_call` stream handling in the generic-chat provider (`legacy_tools`, `merge_legacy`, `finish_legacy`, `function_call` finish reasons, deprecated SDK fixture branches) | architecture 08 function-style tool-call normalization | Modern `tool_calls` fragments only; `function_call` finish strings map to unknown |
| M3-era dual paths in the composition and client (synchronous `StopRun` dispatch, selection-less turn acceptance, no-tool-port denial, no-op post-commit publisher seam, unused `SessionSubscriptionRecovery` wrapper) | ADR 0019 compatibility section; architecture 03/15; m4.md; closeout m2/m5 | Single current path per operation; no fallback branches |
| Historical domain record/wire compatibility (execution-meaning V3 codec and golden, M1/M2 workspace-identity option on `SessionCreatedEventDto`, legacy tool-call and session-selection wire tests, M3/M4 byte-stability test framing) | ADR 0003; ADR 0036 execution-meaning records; architecture 14 | Current record version only (V4); mandatory fields; current-record golden evidence |
| Tooling compatibility surfaces (`dispatch`/`invoke`/`invoke_with_context` bare-result APIs, legacy `exit_code:` rendering, `resolve_path_for_tool` alias) | architecture 05 | Envelope APIs and typed statuses only |
| Migration-result wording in the protocol (`no_migrations_required`, later the constant `migration_result` wire field on `ReloadTransactionDto`) | protocol source | Removed in the same change: the constant wire field is deleted from the current DTO shape; a wire object carrying the removed member may still decode by ignoring it, but no current producer emits it |

### Binding decisions (resolving previously conditional items)

1. **Unknown-additive-field tolerance is retained as current decode behavior**
   (forward compatibility for future additive fields per the current
   architecture 02 policy). The assertion currently in
   `protocol_fixtures.rs` is re-homed into `contracts.rs` before that target is
   deleted; it is not removed.
2. **The no-tool-port M4 denial is intentionally superseded.** The
   `tool_execution_unavailable` fallback branch in the model-tool-loop executor
   is removed and the tool executor becomes mandatory. `m4.md` (line 104) and
   architecture 15 (line 339) are updated in the same change. This is an
   intentional behavior change under this policy, not an accidental removal.
3. **`SessionSubscriptionRecovery` is removed** (test-only wrapper; no production
   caller). The one-shot session-subscription surface itself is retained
   (current, TUI-consumed). Closeout evidence `m2-closure-evidence.md` is
   updated in the same change.
4. **`reasoning_delta` Primary shorthand constructor is retained** as a current
   factory (used by provider normalization and runtime tests); only the wire
   default is removed.
5. **`fail_starting_run` is retained** (current production helper for the
   preserve-accepted path).
6. **`compatibility_id` manifest fields are retained** (current manifest
   identity, not version compatibility).
7. **OpenRouter empty reasoning-details handling (openrouter lib.rs:749) is
   retained** as current empty-event handling; it is not legacy.
8. **`SnapshotBindingSource` is removed only after a catalog-runtime-backed
   binding source is implemented and tested** (see Wave 1); its five call sites
   (intention/src/lib.rs:1909, 1945, 1979, 2043, and test 4087) switch to the
   replacement.

### Out of scope (explicitly NOT superseded, NOT scheduled for removal)

- Slice 1/2 current functionality: catalog controller, private registry,
  control-plane gate, degraded readiness, unavailable-queue promotion and
  reconciliation, usage aggregation, held recovered-run admission, session
  profile selection, provider control-plane services (reload, rotation, health,
  discovery, pricing, editing), reasoning categories/summaries and bounds,
  `provider_profiles_v1` negotiation gates, and the current protocol 1.1 surface.
- Roadmap reservations: `TagStatus::ReservedForSlice3` and
  `ReservedForSlice4`, all reserved ledger tags (0x0203-0x0205, 0x0301-0x0304,
  0x0401-0x0403, 0x0501-0x0505), and reserved contract-family DTOs for slices
  3/4 (goal, harness, MCP, tool-loop, bridge, fork, activity).
- Approved M1 skeleton crates `intention-headroom`, `intention-plans`,
  `intention-vfr`, and `intention-tauri` and their policy entries.
- Current quality tooling (`quality/` checkers, self-tests, run scripts, policy
  files), Makefile targets, CI workflows, and the empty `outdated.toml` ignore
  list (a current negative assertion).
- Governance history: `closeout/` milestone evidence, `m4.md`, and research
  provenance documents (`m4plus_concept.md`, `legacy-baseline/`,
  `legacy-antibusy-prompts/`). These are retained; at most archived with link
  updates, never deleted outright.
- Legitimate optional-state fields that are part of the current wire format
  (for example `ErrorDto` `correlation_id`/`detail`, `SessionProjectionDto`
  optional `config_revision_id`/`active_run`, `ToolResultRecordedEventDto`
  `structured_metadata`) and ordinary optional-config defaults for fresh
  documents (for example a TOML without an optional `[provider.execution]`
  table).

## Version ledger

| Contract | Version/status after this record |
| --- | --- |
| Local protocol | 1.1, exact equality on negotiation (single live minor; no minor tolerance) |
| Public DTO schema | 1.1, additive fields are required fields |
| TOML configuration schema | 1, single shape |
| SQLite storage schema | Logical version 1, single live schema: the current physical DDL (previously labeled "schema 4") is retained as the one schema and created directly on open; no migrations, no version gate |
| Canonical records | one live record version per family (for example execution-meaning V4) |
| Reasoning wire format | one shape; `category` required |
| Provider tool-call wire handling | `tool_calls` fragments only |

## Numeric tag registry

`intention-domain` owns this registry. The `TagStatus` enum keeps exactly the
variants `Wired`, `ReservedForSlice3`, and `ReservedForSlice4`. Slice 2 wired
tags 0x0206-0x020B remain wired. Tag 0x020C (`legacy-m4-selection-binding`) is
removed from the ledger and from `PUBLIC_WIRE_CONTRACT_FAMILIES`; unallocated
tags are not represented in `TagStatus`. The `quality/self_test.py` tag-parity
fixture (lines 1532-1557) is updated in the same change. No future activation
reuses tag 0x020C without a new activating record.

| Tag | Value | Status after this record |
| --- | --- | --- |
| `run-execution-meaning` (record v4 only) | `0x0101` | Wired |
| `programmatic-caller-policy-selection-v1` | `0x0201` | Wired |
| `agent-activity-selection-v1` | `0x0202` | Wired |
| `model-capability-taxonomy-v1` | `0x0206` | Wired |
| `provider-profile-revision-v1` | `0x0207` | Wired |
| `provider-selection-v1` | `0x0208` | Wired |
| `reasoning-history-manifest-v1` | `0x0209` | Wired |
| `context-source-manifest-v1` | `0x020A` | Wired |
| `model-context-projection-v1` | `0x020B` | Wired |
| `legacy-m4-selection-binding` | `0x020C` | Removed from ledger (unallocated) |
| 0x0203-0x0205, 0x0301-0x0304, 0x0401-0x0403, 0x0501-0x0505 | — | ReservedForSlice3/4 (unchanged) |

## Execution order and atomicity

Removals execute in waves. Each wave is one atomic commit: the code removal,
its replacement tests, and its documentation/policy updates land in the same
change (AGENTS.md rule). Before each wave completes, a repository-wide symbol
search must show zero remaining references to the removed symbols, and the
targeted package tests plus `make quick` must pass. `make verify`, `docs-check`,
`check_architecture.py`, and `self_test.py` must pass after each wave; a final
full validation matrix (all gates) runs after Wave 9.

| Wave | System | Atomic unit |
| --- | --- | --- |
| 1 | Legacy M4 selection bridge chain (0x020C) | ONE atomic change across domain + protocol + storage(-sqlite) + application + composition |
| 2 | Domain current records | intention-domain only |
| 3 | Protocol exact version | intention-protocol + intention-types + call-site updates in transport/client/config |
| 4 | Storage single schema | intention-storage-sqlite + intention-storage |
| 5 | Configuration single TOML shape | intention-config |
| 6 | Reasoning and providers | intention-model + provider-generic-chat + provider-openrouter + domain reasoning part |
| 7 | Application/runtime/daemon/composition/client single path | intention-application + intention-runtime + intention-daemon + intention + intention-client |
| 8 | Tooling and meta | intention-tools + intention-workspace + tests/ + docs archive |
| 9 | Policy/docs consolidation | quality/ + docs registers + final validation |

Waves run in order; a later wave may start once the previous wave's workspace
is green. Waves 1 and 8 are independent and may run in parallel. All work lands
on the PR #24 branch (`impl/m5plus-slice2-control-plane`).

## Removal plan by wave

### Wave 1 — Legacy M4 selection bridge chain (0x020C)

Remove, in one coordinated, compile-safe change:

- **Domain:** `crates/intention-domain/src/legacy_bridge.rs` (whole module), its
  re-exports in `lib.rs`, `CanonicalError::LegacySelectionReferenceInvalid`, the
  `LEGACY_M4_SELECTION_BINDING` constant and ledger entry, the golden
  `tests/fixtures/goldens/legacy-m4-selection-binding.txt`, the tag-registry
  membership in `run_execution_meaning.rs:2986,3110`, and the domain tests
  covering the bridge (`m5_control_plane_canonical.rs:179-190,300,329,396,449-450,
  559-565,676-690,1375-1433`, `m5_control_plane_rejections.rs:1231-1370,1468-1469`).
- **Protocol:** `contract_families.rs` 0x020C family: `LegacyM4SelectionBindingDto`,
  its validation, descriptor, membership in `PUBLIC_WIRE_CONTRACT_FAMILIES`,
  error codes `legacy_selection_reference_invalid` / `legacy_selection_binding_invalid`,
  unit tests (`:4634-4725,5506-5535`), tag-parity tests (`:6425,6516`), and the
  `control_plane_contracts.rs` fixtures.
- **Storage:** `crates/intention-storage-sqlite/src/control_plane.rs`
  `legacy_m4_selection_bindings` table DDL and index, `LegacyBindingJson` /
  `CorruptLegacyBindingJson` codecs and scanners, `parse_legacy_validation_status`,
  the `LegacyBindingRepositoryDto` implementation (`:3246-3398`); the
  `sqlite_contracts.rs` legacy-binding tests (`:1865-1972`, `:2622-2714`) and the
  legacy-binding block in `schema4_tables_never_persist_fake_secrets`
  (`:1975-2156`, keep the rest); `crates/intention-storage/src/lib.rs`
  `LegacyBindingValidationStatusDto`, `AppendLegacyM4SelectionBindingInputDto`,
  `LegacyM4SelectionBindingRecordDto`, `LegacyBindingRepositoryDto`, and
  `load_config_revision_records` / `PersistedConfigRevisionRecordDto`
  (`:984-1005`, `:1435-1470`, `:1780-1802`); update the `m5_control_plane_repos.rs:5`
  doc comment.
- **Application:** `crates/intention-application/src/legacy_m4_bridge.rs` (whole
  module), the `pub mod`/`pub use` entries in `lib.rs`, the three bridge tests
  (`tests/m5_catalog_runtime.rs:1931-2032`) and the `FakeLegacy` fixture
  (`:802-940`), and the exhaustive-match arm on
  `CanonicalError::LegacySelectionReferenceInvalid` in `session_selection.rs:95`.
- **Composition:** implement and test a catalog-runtime-backed binding source
  (the application catalog/registry owns provider binding identity per
  ADR 0037), add composition tests proving identical current behavior for the
  five `SnapshotBindingSource` call sites (`crates/intention/src/lib.rs:1909,
  1945, 1979, 2043` and test `:4087`), then remove `SnapshotBindingSource`,
  `DEFAULT_PROFILE_ID`, and the `COMPOSITION_*` constants
  (`crates/intention/src/lib.rs:631-712`).

Docs in the same change: ADR 0037 tag registry and Appendix tables, ADR 0036
ownership/preservation invariants, ADR 0028 invariant 7, architecture 22
"Legacy M4 selection bridge" section (lines 579-592) and its reasoning
compatibility passages (648-664) where they reference the bridge,
`quality/self_test.py` tag-parity fixture, reconciliation registers
(SL1-007, EVD-017 wording), and roadmap rows referencing the bridge
(roadmap 11:1321 and adjacent).

### Wave 2 — Domain current records

- Remove the `RunExecutionMeaningV3Record` codec (`run_execution_meaning.rs:
  214-221, 589-635`) and its V3-only test legs (`fixture_v3_record` 809-813,
  round-trip 884-890, `v3_decodes_without_synthetic_activity_selection...`
  1125-1131, missing-field/wrong-wire legs 1150-1200, unknown-field/tag legs
  1263/1430-1460, trailing-bytes leg 1563-1575, `canonical_encoding_is_byte_deterministic`
  legs 2389-2412, `golden_v3_record` 2549-2553,
  `golden_execution_meaning_v3_fixture...` 2676-2690); delete the golden
  `execution-meaning-v3.txt`; repin identity golden digests
  (`identity-v1.txt`, `namespaced-digest-v1.txt`, `identity-exclusion-v1.txt`)
  to V4 inputs (2300, 2890-2960); drop the v3 entries from
  `m5_control_plane_canonical.rs:1440,1456-1461,1486,1530-1531` and reframe
  `m3_m4_execution_meaning_bytes_remain_unchanged` as current-record
  byte-stability (keep v4/envelope/agent-activity/policy-provenance goldens).
- Make `workspace_id` required on `SessionCreatedEventDto`
  (`lib.rs:1510,1526,1577-1579`): change the field to non-optional
  `WorkspaceId`, drop the serde default, update the raw DTO/deserializer and
  accessor together, add `workspace_id` to `tests/fixtures/event-envelope-v1.json`,
  delete `tests/m3_contracts.rs:71-90`, and check downstream consumers
  (`intention-storage-sqlite/src/lib.rs:738` supplies it via `new()`; mirrored
  event family in `intention-protocol/contract_families.rs` if present).
- Remove the historical-M4 reasoning default in `model_facts.rs`
  (`:346,388`): `category` becomes mandatory; delete the legacy assertion block
  in `tests/m4_durable_facts.rs:240-257` (domain part; model part in Wave 6).
- Remove legacy wire-compat tests: `tests/m4_durable_fact_wires.rs:300-307`
  (legacy tool-call wire; shape equals current `ToolCallRecorded` — the test
  itself is removed, the current-shape assertions in the same file stay) and
  `tests/m5_session_selection_overrides.rs:103-109` (old JSON without override
  fields deserializing).
- Reword the architecture 10:139 policy sentence
  ("documented compatible fixtures remain decodable") in the same change.

Docs: ADR 0003, ADR 0035, ADR 0036 execution-meaning records (v3 rows),
architecture 14 (historical compatibility passages: lines 20, 242-249,
264-267, 298-301, 326-331), architecture 27 (line 434), roadmap 11
(lines 429, 1136), evidence-register EVD-043.

### Wave 3 — Protocol exact version

- Remove `ProtocolVersionDto::ensure_compatible_with` and
  `SchemaVersionDto::ensure_compatible_with` (`intention-protocol/src/lib.rs:
  63-72`; `intention-types/src/lib.rs:136-144`) and ALL call sites, including
  the internal ones: protocol negotiation tests (`negotiation.rs:363,378,460,
  467`), protocol unit tests (`lib.rs:1605,1610`), intention-types unit tests
  (`lib.rs:759,764`), transport (`intention-transport/src/lib.rs:377,402,434,
  468,493,643,658`), client (`intention-client/src/lib.rs:769,846`), and config
  (`intention-config/src/lib.rs:437,526,551,591,593`). Replace with exact
  current-version equality. Keep the incompatible-major rejection path and its
  golden `hello-incompatible-major-v2.json`; rename
  `hello-compatible-minor-v1.json` to reflect current-version equality and
  update `control_plane_contracts.rs:334-350`.
- Remove the legacy-shape deserializers and `None` constructors:
  `ProtocolAcceptedDto::new` and `SessionSnapshotDto::new`
  (`lib.rs:742-766,757,784,1012-1028,1035,1091-1093`); make `result` and
  `projection` required wire fields; update all construction call sites
  (`lib.rs:1664,1676,1887,1932`, `tests/contracts.rs:116,189,192,261`,
  `tests/m3_contracts.rs:28-57,64-86,169`, `tests/accessors.rs:24`,
  `intention-client/tests/client_contract.rs:397,553,608`). Keep `ErrorDto`
  optional fields (current format).
- Remove 1.0 wire fixtures (`tests/fixtures/protocol-hello-v1.json`,
  `protocol-request-envelope-v1.json`, `protocol-session-snapshot-v1.json`,
  `protocol-subscribe-session-v1.json`, `protocol-accepted-v1.json`,
  `protocol-daemon-health-v1.json`, `protocol-subscribe-session-run-v1.json`)
  and the whole `tests/protocol_fixtures.rs`; drop `protocol_fixtures` from
  `quality/architecture.toml` test_targets; re-home the unknown-additive-field
  tolerance assertion (binding decision 1) into `contracts.rs`.
- Remove the legacy error fixture `error-v1-legacy.json` and its test
  (`crates/intention-types/tests/error_contracts.rs:17-24`).
- Remove/replace the `no_migrations_required` protocol value
  (`intention-protocol/src/lib.rs:2127`) with current wording.
- Bump remaining fixture values from `ProtocolVersionDto::new(1,0)` /
  `SchemaVersionDto::new(1,0)` to the current constants in
  `tests/contracts.rs`, `tests/m3_contracts.rs`, `tests/accessors.rs`,
  `tests/m4_run_stream_contracts.rs`, protocol unit tests, and the
  production/test-support sites in `intention-storage-sqlite/src/lib.rs:400,
  2230`, `intention-runtime/src/lib.rs:1431`, `intention-daemon/src/lib.rs:1547`,
  `intention-test-support/src/lib.rs:62`, `intention-test-support/tests/
  fixture_host.rs:76,101,105`, and `intention-tui/tests/tui_contract.rs:53,117,
  128` (distinguish current schema constants from obsolete fixture values;
  do not touch live version constants).
- Do not touch `SubscribeSessionCommandDto::new` (current session-wide
  subscription mode), the current 1.1 negotiation gates
  (`require_provider_profiles` / `require_capability` /
  `require_gateway_tool_loop`), or the reserved Slice 3/4 families.

Docs: ADR 0036 (lines 122-129), ADR 0037 (lines 181-183), architecture 02
(versioning section 157-178), architecture 10 (lines 93-94, 108-112, 139-140),
compatibility-register (rows 39, 42, 44, 45), closeout `m0-m1-closure-evidence.md:78`.

### Wave 4 — Storage single schema

Remove from `crates/intention-storage-sqlite`:

- The migration chain: `CURRENT_STORAGE_SCHEMA`, the `schema_m3_sql!` /
  `schema_m4_sql!` / `schema_m4_tool_results_sql!` macros and
  `SCHEMA_M3_SQL` / `SCHEMA_M4_SQL` / `SCHEMA_M4_TOOL_RESULTS_SQL` constants
  (`lib.rs:33-149`), the `MIGRATIONS` lazy (`:144-151`), the
  `rusqlite_migration` dependency (`Cargo.toml:18`, `lib.rs:30`), and the
  `user_version` read plus `unsupported_storage_schema` future-schema check in
  `open()` (`:186-207`).
- `hydrate_model_run_snapshots` (`:210-236`); keep `ensure_run_cursors`
  (`:571-579`) and `snapshot_model_runs` (`:476-530`).
- The hidden `TEST_SCHEMA_3_SQL` export (`:133-142`) and the `pub use
  control_plane::SCHEMA_M5_SQL` re-export (`:613-615`); the current schema DDL
  text stays and becomes the single create path.
- The standalone legacy-schema fixture block in `tests/m4_durable_facts.rs:
  23-136` (`M3_SCHEMA_SQL`, `legacy-m3.sqlite`, migration assertions) plus the
  preservation tests `m3_database_migrates_to_cursor_zero_snapshots_without_synthetic_facts`
  (`:129-227`) and `slice1_schema_three_reopen_preserves_all_m3_m4_bytes`
  (`:228-460`) and their helpers (`capture_rows`, `fact_envelope`,
  `tool_lifecycle_envelope`, `workspace_root_path`).
- The preservation/migration-failure fixtures in `tests/sqlite_contracts.rs`:
  `schema4_migration_preserves_schema_three_rows_byte_for_byte` (`:1451-1748`),
  `schema4_migration_failure_rolls_back_to_schema_three_atomically`
  (`:1781-1863`), the future-schema half of
  `migration_rejects_future_schema_and_config_snapshot_is_safe_to_persist`
  (`:816-836`), and helpers `CapturedValue`/`capture_rows` (`:1245-1275`),
  `reasoning_delta_envelope` (`:1276-1303`), `tool_lifecycle_envelope`
  (`:1749-1780`), plus the `rusqlite_migration` import (`:40`).

Rewrite `open()` to create the full current schema directly (one
`execute_batch` of the concatenated current DDL, including
`control_plane::SCHEMA_M5_SQL`; add a regression test proving the complete
current schema is created and usable, and that all DDL/indexes/tables are
included exactly once). Rename `slice1_storage_schema_four_remains_authoritative`
(`sqlite_contracts.rs:839-919`) to the single-version schema name and drop the
`user_version` assertion (keep the table-inventory assertions). Update
migration-oriented comments in `control_plane.rs:1-7,61-62,3387` and
`lib.rs` in the same change.

Docs: ADR 0037 (lines 35, 143, 161), ADR 0036 (lines 89-111), architecture 04
(lines 104-105, 285-286), architecture 03 (line 309), architecture 10 (line
140), architecture 12 (lines 279-286), roadmap 11 (lines 490, 511), registers:
SL1-001, SL1-006, SL2-006 (source-of-truth-matrix:60,65,78), EVD-045/EVD-058/
EVD-047 (evidence-register:57,60,62,73), compatibility-register (12, 14, 30,
42, 45), ownership-and-dependency-map:57, `quality/architecture.toml:106,
208-209`, `quality/outdated.toml` comment header, closeout m3 (historical,
leave).

### Wave 5 — Configuration single TOML shape

- Remove the v0 migration path: the `None => Self::migrate_v0(document)?`
  branch in `parse_resolve` (`lib.rs:356`), `migrate_v0` (`:390-404`),
  `RawV0Config` (`:701-705`), `RawV0ModelConfig` (`:707-710`); unversioned
  documents fail closed with `invalid_config_schema`.
- Remove the legacy credential fallback in `parse_credential` (`:737-757`,
  v0 branch `:750-757`); only `provider.credential` is read.
- Remove the v0 branch in the candidate control plane: the
  `None => collect_v0_issues` dispatch (`control_plane.rs:563`),
  `collect_v0_issues` (`:615-658`), and the `model.api_key` legitimate-key
  exemption in `has_forbidden_credential_shaped_content` (`:776-790`).
- Remove schema-compat acceptance and additive-field defaults for old
  snapshots: require the exact current schema in `from_public_parts`
  (`:430-445`), `ConfigSnapshotDto::new` (`:544-557`), and
  `validate_for_persistence` (`:589-594`), and drop the `#[serde(default)]` on
  `provider_execution` in `ResolvedConfigDto` (`:351`). Keep the ordinary
  optional-config defaults for fresh documents that omit the optional
  `[provider.execution]` table (`from_raw` `:684-688`).
- Update/remove all tests accepting unversioned `[model]`/`api_key`
  documents or old snapshots: `tests/contracts.rs:74-86` and `:76`,
  `tests/m4_contracts.rs:83-90,108-127,116`, `tests/m5_control_plane_config.rs:
  268-271,523-546,614-658,700-762`, `tests/snapshot_fixtures.rs`, and the
  fixture `config-snapshot-v1.json` (include `provider_execution`); update the
  unit test `parse_and_migration_helpers_reject_schema_and_legacy_shape_mismatches`
  (`lib.rs:971-993`).
- Keep `parse_candidate`, `semantic_equivalence`, `classify_changed_fields`,
  `reject_catalog_affecting_edits`, and the candidate DTOs (current Slice 2
  behavior). The credential-free `redacted_safe_digest` and the dead
  `CandidateAcceptanceOutcomeDto` projection, together with config's private
  SHA-256 module, were removed in the PR 24 repair run because no production
  surface consumed them (PR24-037/038).

Docs: architecture 09 (lines 14-16, 64, 148-159), roadmap 11 (lines 130-131),
ADR 0037 M3/M4 preservation wording where it references snapshots.

### Wave 6 — Reasoning and providers

- **Domain/model:** make `category` required in `ReasoningDeltaRecorded`
  (`crates/intention-domain/src/model_facts.rs:346,388`) and
  `ModelEventDto::ReasoningDelta` (`crates/intention-model/src/lib.rs:490-532,
  502-503,527`); delete the legacy-assertion tests (`domain m4_durable_facts.rs:
  240-257`, `model m6_reasoning_surface.rs:15-30`). Keep
  `reasoning_delta_categorized` and the `reasoning_delta` Primary convenience
  constructor (binding decision 4).
- **Generic-chat:** remove the deprecated `function_call` stream handling
  (`legacy_tools` field `:540`, `#[allow(deprecated)]` branch in `accept_delta`
  `:655-660`, `merge_legacy` `:744-750`, `finish_legacy` `:739-743`, the legacy
  half of `finish()` `:605-618` and `generic_chat_conflicting_tool_call`), the
  `"function_call"` arm of `map_finish_reason` (`:776`), the
  `FinishReason::FunctionCall` arm of `map_native_finish` (`:782-784`), the
  deprecated-SDK fixture branches (`:1250-1251,1333-1354,1559-1560`), and the
  tests `legacy_function_fragments_complete_only_at_terminal` (`:1168-1204`),
  `conflicting_modern_and_legacy_calls_fail_without_duplicate_output`
  (`:1294-1325`), plus the `("function_call", ...)` row in
  `generic_chat_contracts.rs:214-219`. Keep modern `tool_calls` merge/finish.
- **Generic-chat dialect decoder:** remove the no-op `ReasoningDialectDecoder`
  and the `with_reasoning_dialect` option (`:43-150,153-207,192-207,499-504,
  630-650`); a declared dialect fails closed in `build()`. Keep
  `with_reasoning_effort` (applied). Remove the three dialect tests
  (`reasoning_dialect_accepts_each_closed_path_and_preserves_declared_order`,
  `reasoning_dialect_rejects_unknown_paths_and_duplicates`,
  `reasoning_dialect_decoder_accepts_the_pinned_typed_delta_for_every_path`
  `:1540-1604`). Keep `provider_reasoning_stream_invalid` (also fired by
  OpenRouter encrypted-detail handling).
- **Generic-chat thinking options:** remove the dead thinking-activation
  builders (`with_thinking`, `with_enable_thinking`, `with_think`,
  `with_think_effort`, `with_thinking_budget`, `with_thinking_token_budget`
  `:197-260`) and the `unsupported_thinking_configuration` rejection in
  `build()` (`:282-296`); keep `with_reasoning_effort`; trim
  `unapplicable_thinking_and_effort_declarations_reject_before_any_request`
  (`:1644-1679`).
- **OpenRouter:** remove the stored-but-unapplied `with_preservation_controls`
  (`:115-125`; fail closed in `build()` when present) and the legacy
  `"function_call"` finish-string arm (`:609-611`) plus its test row
  (`openrouter_contracts.rs:196-197`). Keep the empty reasoning-details
  handling (binding decision 7).

Docs: ADR 0028 (invariants 7-8), architecture 08 (function-style tool-call
section), architecture 22 (reasoning compatibility passages), m6_reasoning
test-target policy (unchanged name).

### Wave 7 — Application/runtime/daemon/composition/client single path

- Remove the dead user-turn variants `send_user_turn` (`lib.rs:885-913`),
  `send_user_turn_and_schedule` (`:915-997`), and
  `send_user_turn_with_provider_selection` (`:999-1021`); keep
  `send_user_turn_and_schedule_with_provider_selection` (`:1023-1096`). Delete
  the corresponding tests (`m3_application.rs:771,826,1137` and the turn call
  inside `:1229`; `m5_session_selection.rs:1898,1935,1961`) and the whole
  `tests/m4_application_scheduling.rs` (645 lines); keep shared helpers used by
  the live variant (`accepted_user_turn`, `started_run_committed_in`,
  `schedule_from_context`, `preserve_accepted_after_scheduling_failure`). Drop
  the `m4_application_scheduling` test target from BOTH
  `quality/architecture.toml:189` (intention-application) and `:196`
  (intention-runtime); update `quality/self_test.py:528,542` and the closeout
  rows `m5-closure-evidence.md:28,33,35`.
- Remove the selection-less fallback: `resolve_for_turn`
  (`session_selection.rs:283-332`) returns a typed resolution error
  (`provider_profile_runtime_unavailable`) instead of `Ok(None)` when no
  profile applies; `accept_user_turn_input_with_selection` drops the
  `None => Ok(base)` branch (`lib.rs:1209-1235`); delete
  `resolve_for_turn_keeps_the_legacy_none_path_when_no_profile_applies`
  (`m5_session_selection.rs:2059`) and replace with a rejection test. Flip the
  composition tests (`crates/intention/src/lib.rs:4582-4595`,
  `:4744-4757`) to expect rejection or seed a catalog. (This record asserts
  no-profile deployments are not a supported state.)
- Remove `resolve_for_override` (`session_selection.rs:333-367`) and its four
  tests (`m5_session_selection.rs:2113,2122,2131,2142`); remove
  `invoke_local_tool_once` (`lib.rs:393-395`).
- Remove the synchronous M3 `StopRun` dispatch arm
  (`crates/intention/src/lib.rs:2119-2133`) and its accessor arms (`:2269,
  :2294`); keep `stop_run_for_daemon_host` (`:1290-1322`),
  `terminalize_cancelling_run_for_daemon` (`:1333-1357`), the daemon async host
  path (`intention-daemon/src/lib.rs:274-310,944-946`), and the protocol
  `StopRunCommandDto`. Re-point the affected tests
  (`intention/src/lib.rs:3137-3143,3346-3352`) to the host path; update
  `intention-test-support/tests/composition_contract.rs:80` if it exercises
  the synchronous path.
- Remove the no-tool-port M4 denial branch in the model-tool-loop executor
  (`crates/intention-runtime/src/lib.rs:726-746` and docs `:470-473,852-854`);
  make the tool executor mandatory and pass one at the daemon re-entry call
  (`intention-daemon/src/lib.rs:246-250`); delete
  `no_port_preserves_m4_denial` (`tests/m5_tool_loop.rs:1119-1159`). Update
  `m4.md:104` and architecture 15:339 in the same change (binding decision 2).
- Remove the retained M3 post-commit publisher seam: `PostCommitPublisher`,
  `NoopPostCommitPublisher`, the `publisher` field
  (`crates/intention/src/lib.rs:519-527,568-573`),
  `publish_after_durable_read` invocations in `command_result` and
  `recover_before_ready` (`:2226-2261`), and the seam fire in
  `DurableToolResultPublisher` (`:581-596`); update the constructors
  (`:1107,1128,1154,2897,3261`) and tests (`:2453-2469,3140-3180,3400-3430,
  3811-3865`). Keep `committed_tool_result_evidence` (`:611-641`) and the
  `HostCommitObserver` publication path. Update architecture 03:207-208.
- Remove `SessionSubscriptionRecovery` (`intention-client/src/lib.rs:1103-1157`)
  and its test (`client_contract.rs:582-666`, `intention/src/lib.rs:3222`);
  review whether `SessionSubscriptionReducer` (`:1159-1266`) is consumed by the
  live one-shot subscribe path before removing (remove only if test-only);
  update `closeout/m2-closure-evidence.md:61` in the same change (binding
  decision 3).
- Update ADR 0037 line 66: the `control_plane_unavailable` dispatch stub is
  already removed from `crates/intention/src/lib.rs`.

Keep: degraded gate `execution_not_ready`, catalog startup/seed, held-run
admission (`AdmitRecoveredRun`), pending-removal accept/reject,
unavailable-queue promotion/reconciliation, promotion on terminal commit,
recovery, `fail_starting_run` (binding decision 5), run streams, facade_e2e
evidence, and the client Slice 2 surfaces.

Docs: ADR 0019 (compatibility section), ADR 0037, architecture 03/15/16, m4.md,
closeout m2/m5.

### Wave 8 — Tooling and meta

- `crates/intention-tools`: remove the bare-result compatibility trio
  (`dispatch` `:1115-1117`, `invoke` `:1168-1170`, `invoke_with_context`
  `:1172-1185`) and the doc paragraph `:1193`; migrate ALL call sites,
  including the coverage tests (`tool_contracts.rs` ~50 `.dispatch(` sites,
  `:996` `.invoke(`, `tool_coverage_invocation.rs:65,144`,
  `tool_coverage_final.rs:30,73`, `tool_coverage_contracts.rs:122`) to
  `dispatch_with_cancellation(call, input, CancellationSignal::new())` or
  `invoke_enveloped*`; keep `dispatch_with_cancellation` and
  `invoke_enveloped_with_cancellation`. Remove the legacy `exit_code:`
  rendering comment and the arbitrary `-1` sentinel (`:220-221,1288`); update
  the four assertions (`tool_contracts.rs:278,850,1448,1586`); keep typed
  `ToolProcessStatus`.
- `crates/intention-workspace`: remove the unused `resolve_path_for_tool`
  alias (`:85-94`) and its equivalence test (`:309`).
- Remove the orphan top-level `tests/quality/README.md` and the empty
  `tests/` tree (verified: no references; invisible to all gates).
- Archive (do not delete) `docs/reference/legacy-antibusy-audit/` to
  `docs/reference/archive/legacy-antibusy-audit/` and update the linking
  READMEs (`docs/README.md:9`, `docs/intention-relay/README.md:16`) in the
  same change (docs-only). Keep `legacy-baseline/`, `legacy-antibusy-prompts/`,
  `m4plus_concept.md`, `production-ceiling-removal.md` (while rows 1-2 are
  open), and `closeout/` as provenance with current links.
- Migrate the remaining schema-1.0 construction sites to current constants
  (`intention-test-support/src/lib.rs:62`,
  `intention-test-support/tests/fixture_host.rs:76,101,105`,
  `intention-tui/tests/tui_contract.rs:53,117,128`).

Docs: architecture 05 (tooling API/rendering sections).

### Wave 9 — Policy/docs consolidation

- Update `quality/architecture.toml`: drop `protocol_fixtures` (Wave 3) and
  `m4_application_scheduling` in both crate entries (Wave 7); update the
  storage-sqlite responsibility/dependency entries (`:106,208-209`) to remove
  `rusqlite_migration` and migration wording; update the config responsibility
  (`:215`) to remove migration wording; update the protocol test-target
  description (`:286-287`) from "protocol compatibility fixtures".
- Update `quality/self_test.py`: the tag-parity fixture (Wave 1), the slice-2
  ADR existence / schema-four / protocol-version / declared-target assertions
  (`:1522-1611`), the architecture-policy mutation fixtures (`:458-929`), the
  coverage-policy mutation tests (`:969-1189`), and the `:1542` reference; run
  the self-test suite after each wave.
- Verify `quality/coverage.toml` has no removed test files listed in coverage
  groups/exclusions; re-verify all tiers (`make verify`) after each wave.
- Update `quality/check_architecture.py:626` fixtures if the coverage mapping
  changes.
- Update the reconciliation registers and evidence rows named in Waves 1-8
  (SL1-001, SL1-006, SL1-007, SL2-006, DUR-008; EVD-017, EVD-043, EVD-045,
  EVD-047, EVD-058, EVD-059, EVD-060; compatibility-register rows 12, 14, 30,
  39, 42, 44, 45; ownership-and-dependency-map:57; source-of-truth-matrix rows
  at 41, 60, 65-66, 78, 424, 434) and architecture 10/12 normative passages
  (`10:93-94,108-112,139-140,322-329`, `12:270-286,436-440`).
- Final validation matrix: `cargo test --workspace`, `cargo nextest
  --workspace --all-targets --locked --no-fail-fast`, `cargo clippy
  --workspace --all-targets --locked -- -Dwarnings`, `cargo fmt --all --
  --check`, `make quick`, `make verify`, `make docs-check`,
  `check_architecture.py`, `self_test.py`, plus targeted package tests for
  every changed crate. Repository-wide symbol searches must show zero
  remaining references to every removed symbol.

## Ownership

Semantic canonical records/tags belong to `intention-domain`; public wire and
frames to `intention-protocol`; storage to `intention-storage` and
`intention-storage-sqlite`; registry/typed tool contracts to `intention-tools`;
provider-private translation to provider crates; process/publication to
`intention-daemon`; concrete assembly to `intention`; adapters to
`intention-client`, then TUI/Tauri. No new crate, dependency, feature,
coverage tier, or exclusion is introduced by this record. Skeleton crates
`intention-headroom`, `intention-plans`, `intention-vfr`, and
`intention-tauri` remain untouched.

## Quality-policy changes

Covered per-wave in Wave 9; each wave additionally keeps its own
`make quick`/targeted tests green. Coverage tiers are unchanged; after each
wave, coverage is re-verified with `make verify` (removing code shrinks the
denominator; uncovered legacy lines that previously counted toward coverage
disappear). Documentation checks (`docs-check`, `check_architecture.py`,
`self_test.py`) must pass after the same-change documentation updates listed
per wave.

## Evidence and non-goals

Required evidence per wave: the removed path has no production caller or only
compatibility consumers; the current-path tests still pass; `make quick` and
the targeted package tests pass; the wave's documentation updates are in the
same change; final validation per Wave 9.

This record does not implement M6-M9 behavior, does not introduce a second
runtime, registry, scheduler, persistence authority, or sandbox, and does not
remove any roadmap reservation, approved skeleton crate, or current Slice 1/2
functionality.

## Resolution notes

- **Why this policy now governs:** AGENTS.md is the authoritative engineering
  context and its "No backward compatibility" section (main `b5fa71e`)
  conflicts with the preservation commitments previously recorded in
  ADR 0003/0036/0037 and the architecture documents. Those commitments are
  superseded here as recorded above; the same-change documentation updates
  keep every document consistent with the code.
- **ADR 0035 authority:** ADR 0035 remains the activation home and
  slice-sequence authority. ADR 0038 changes implementation and documentation
  obligations without renumbering or reauthorizing slices 1/2 and without
  reopening closed milestones; closeout evidence stays immutable provenance,
  only active indexes/links and command/evidence rows that reference removed
  test targets may change.
- **Physical version interpretation:** the concrete protocol version remains
  1.1 as the single live minor; "exact equality" removes minor tolerance, not
  the version constant. The SQLite physical DDL previously labeled "schema 4"
  is retained as the one current schema (logical version 1); no version marker
  or migration machinery accompanies it.
- **Transition of reconciliation/evidence rows:** rows that record removed
  behavior (migration, preservation, legacy bridge, 1.0 compat) are rewritten
  or retired in the same change as the code removal; rows recording retained
  current behavior stay.
- **Unknown-field tolerance:** retained as forward-compatible decode behavior
  (future additive fields), per binding decision 1.
