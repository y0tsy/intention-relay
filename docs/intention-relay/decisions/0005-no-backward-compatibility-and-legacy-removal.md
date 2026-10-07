# ADR 0005: No backward compatibility and legacy removal

## Status

Accepted as the superseding specification that removes all backward-compatibility, legacy, fallback, and migration
machinery from the project, in accordance with the [AGENTS.md](../../../AGENTS.md) "No backward compatibility" policy
(main commit `b5fa71e`). It is subordinate to [ADR 0004](0004-m5plus-complete-foundation-activation.md), which remains
the activation home and slice-sequence authority.

Amended 2026-09-30: the removal program below has fully executed, and the transport, codec, root, and limit records of
ADRs 0011 to 0014 postdate this record's wave plan. The single-version policy and every wave removal stand.

## Decision

The single-version policy governs every versioned system in the project, and every execution path whose only purpose is
compatibility with a version that is no longer current is removed.

- Backward compatibility is neither required nor in demand. Nothing built or
run from this project exists outside the development machine: no deployed users, no externally persisted data, and no
third-party consumers.
- Outdated execution paths are removed. Compatibility layers, fallback
branches, and migration paths are not added to keep old behavior readable, replayable, or upgradeable.
- Until every roadmap milestone is complete and fully closed, each versioned
system keeps exactly one live version, version 1. Database schemas, protocol versions, wire formats, configuration
formats, and storage formats evolve in place within that single version.
- The existence of databases is not an argument for keeping DB migrations or
old-schema compatibility.
- Compatibility fixtures, golden files, and tests that assert behavior of any
version other than the current one are not maintained.

### Superseded commitments

| Removed commitment | Replacement |
| --- | --- |
| SQLite schema 3 to 4 additive migration, `user_version` tracking, and future-schema rejection (architecture 04; roadmap slice rows) | One live schema (logical version 1) created directly on open; no migration chain, no `user_version` gate, no opening of older schemas |
| M3/M4 byte-preservation evidence (schema-3 reopen fixtures, migration rollback fixtures, `TEST_SCHEMA_3_SQL`, standalone `M3_SCHEMA_SQL` fixture block) | Removed with the migration machinery; current-schema round-trip tests remain |
| Legacy M4 selection bridge (tag `legacy-m4-selection-binding` 0x020C; `LegacyM4SelectionBindingDto`; `legacy_m4_selection_bindings` table; `LegacyBindingRepositoryDto`; `load_config_revision_records`; application `LegacyM4Bridge`; composition `SnapshotBindingSource` mirror derivation) | Removed entirely; tag 0x020C returns to unallocated and is removed from the ledger and `PUBLIC_WIRE_CONTRACT_FAMILIES`; no synthetic bindings are ever materialized |
| Protocol same-major compatibility (1.0 to 1.1) via `ensure_compatible_with` on protocol and DTO schema versions | Exact current-version equality on negotiation; no minor tolerance (protocol 2.0 after [ADR 0011](0011-local-json-rpc-2-0-transport.md)) |
| 1.0 wire fixtures and legacy-shape deserializers (`ProtocolAcceptedDto`/`SessionSnapshotDto` additive-field tolerance, `protocol_fixtures` target, error-v1 legacy fixture, `hello-compatible-minor-v1.json` naming) | Current-version fixtures only; additive fields become required on the wire |
| TOML configuration v0 migration (`migrate_v0`, `RawV0Config`, `RawV0ModelConfig`, `model.api_key` credential fallback, `collect_v0_issues`) | Unversioned documents fail closed (`invalid_config_schema`); only the current `[provider]` shape parses |
| Historical reasoning wire defaults (uncategorized `ReasoningDelta` decoding as `Primary`) in domain and model crates | `category` is required on the wire; no defaulting |
| Deprecated Chat Completions `function_call` stream handling in the generic-chat provider (`legacy_tools`, `merge_legacy`, `finish_legacy`, `function_call` finish reasons, deprecated SDK fixture branches) | Modern `tool_calls` fragments only; `function_call` finish strings map to unknown |
| M3-era dual paths in the composition and client (synchronous `StopRun` dispatch, selection-less turn acceptance, no-tool-port denial, no-op post-commit publisher seam, unused `SessionSubscriptionRecovery` wrapper) | Single current path per operation; no fallback branches |
| Historical domain record/wire compatibility (execution-meaning V3 codec and golden, M1/M2 workspace-identity option on `SessionCreatedEventDto`, legacy tool-call and session-selection wire tests, M3/M4 byte-stability test framing) | Current record version only (V4); mandatory fields; current-record golden evidence |
| Tooling compatibility surfaces (`dispatch`/`invoke`/`invoke_with_context` bare-result APIs, legacy `exit_code:` rendering, `resolve_path_for_tool` alias) | Envelope APIs and typed statuses only |
| Migration-result wording in the protocol (`no_migrations_required`, later the constant `migration_result` wire field on `ReloadTransactionDto`) | Removed in the same change: the constant wire field is deleted from the current DTO shape; a wire object carrying the removed member may still decode by ignoring it, but no current producer emits it |

### Settled bindings

1. Unknown-additive-field tolerance is retained as current decode behavior
(forward compatibility for future additive fields per architecture 02). The assertion formerly in `protocol_fixtures.rs`
was re-homed into `contracts.rs`; it is not removed.
2. The no-tool-port M4 denial is intentionally superseded: the
`tool_execution_unavailable` fallback branch in the model-tool-loop executor is removed and the tool executor is
mandatory. This is an intentional behavior change under this policy, not an accidental removal.
3. `SessionSubscriptionRecovery` is removed (test-only wrapper, no production
caller). The one-shot session-subscription surface itself is retained (current, TUI-consumed).
4. Retained current factories and surfaces: the `reasoning_delta` Primary
shorthand constructor, `fail_starting_run` (the preserve-accepted helper), the `compatibility_id` manifest fields
(current manifest identity, not version compatibility), and OpenRouter empty reasoning-details handling (`openrouter`
`lib.rs:749`).
5. `SnapshotBindingSource` is removed with the legacy chain; its five call sites in
`crates/intention/src/lib.rs` (including its test) were updated in the same change.

### Out of scope (retained)

- The ordinary runtime that ADRs 0011 to 0014 leave in place,
including the M4 normalized reasoning events of [ADR 0008](0008-same-run-reasoning-round-trip.md). The Slice 2 control
plane (catalog controller, private registry, control-plane gate, degraded readiness, unavailable-queue promotion and
reconciliation, usage aggregation, held recovered-run admission, session profile selection, provider control-plane
services, `provider_profiles_v1` gates, and the protocol 1.1 surface) is not activated; the capability plane is removed
by [ADR 0011](0011-local-json-rpc-2-0-transport.md); the canonical codec is removed by [ADR
0012](0012-typed-serde-json-contracts.md); and the speculative contract limits are removed by [ADR
0014](0014-limits-by-precedent-and-no-content-scanning.md).
- Roadmap scope for slices 3 and 4. The slice scope itself is untouched; the
reserved ledger tags with their `TagStatus` entries and the reserved contract-family DTOs are removed with the canonical
codec by [ADR 0012](0012-typed-serde-json-contracts.md).
- Approved M1 skeleton crates `intention-headroom`, `intention-plans`,
`intention-vfr`, and `intention-tauri` and their policy entries.
- Current quality tooling (`quality/` checkers, self-tests, run scripts,
policy files), Makefile targets, CI workflows, and the empty `outdated.toml` ignore list (a current negative assertion).
- Governance history: `closeout/` milestone evidence and research provenance
documents are retained, at most archived with link updates, never deleted outright.
- Legitimate optional-state fields that are part of the current wire format
(for example `ErrorDto` `correlation_id`/`detail`, `SessionProjectionDto` optional `config_revision_id`/`active_run`,
`ToolResultRecordedEventDto` `structured_metadata`) and ordinary optional-config defaults for fresh documents (for
example a TOML without an optional `[provider.execution]` table).

## Version ledger

| Contract | Version/status after this record |
| --- | --- |
| Local protocol | 2.0 over JSON-RPC 2.0 with NDJSON framing, exact equality on the handshake ([ADR 0011](0011-local-json-rpc-2-0-transport.md)); the recorded point, exact equality with no minor tolerance, stands |
| Public DTO schema | 1.1, additive fields are required fields |
| TOML configuration schema | 1, single shape |
| SQLite storage schema | Logical version 1, single live schema: the current physical DDL (previously labeled "schema 4") is retained as the one schema and created directly on open; no migrations, no version gate |
| Canonical records | Removed: no canonical record, tag, digest, or identity exists ([ADR 0012](0012-typed-serde-json-contracts.md)) |
| Reasoning wire format | one shape; `category` required |
| Provider tool-call wire handling | `tool_calls` fragments only |

## Removal program

Waves ran in order, except Waves 1 and 8, which are independent and ran in parallel. Each wave is one atomic commit: the
code removal, its replacement tests, and its documentation/policy updates landed in the same change. Before a wave
completed, a repository-wide symbol search showed zero remaining references to the removed symbols, and the targeted
package tests plus `make quick` passed; `make verify`, `docs-check`, `check_architecture.py`, and the final validation
matrix ran after each wave.

| Wave | System | Atomic unit | Removed |
| --- | --- | --- | --- |
| Legacy M4 selection bridge chain (0x020C) | ONE atomic change across domain + protocol + storage(-sqlite) + application + composition | Domain `legacy_bridge` module, protocol 0x020C family and its error codes, storage legacy-binding table and codecs, application `LegacyM4Bridge`, composition `SnapshotBindingSource`, `DEFAULT_PROFILE_ID`, and the `COMPOSITION_*` constants |
| Domain current records | intention-domain only | `RunExecutionMeaningV3Record` codec and golden, the M1/M2 workspace-identity option (now required `WorkspaceId`), the historical-M4 reasoning default (`category` mandatory), and the legacy wire-compat tests |
| Protocol exact version | intention-protocol + intention-types + call-site updates in transport/client/config | `ProtocolVersionDto::ensure_compatible_with`, `SchemaVersionDto::ensure_compatible_with`, `ProtocolAcceptedDto::new`, `SessionSnapshotDto::new`, the 1.0 wire fixtures and `tests/protocol_fixtures.rs`, `error-v1-legacy.json`, `hello-compatible-minor-v1.json`, and `no_migrations_required` |
| Storage single schema | intention-storage-sqlite + intention-storage | `CURRENT_STORAGE_SCHEMA`, the `schema_m3_sql!`/`schema_m4_sql!`/`schema_m4_tool_results_sql!` macros and their `SCHEMA_*_SQL` constants, `MIGRATIONS`, `rusqlite_migration`, the `user_version` read and future-schema check, `TEST_SCHEMA_3_SQL`, `SCHEMA_M5_SQL`, `hydrate_model_run_snapshots`, and the migration and preservation fixtures |
| Configuration single TOML shape | intention-config | `migrate_v0`, `RawV0Config`, `RawV0ModelConfig`, the `model.api_key` credential fallback, `collect_v0_issues`, old-snapshot acceptance, and additive-field defaults |
| Reasoning and providers | intention-model + provider-generic-chat + provider-openrouter + domain reasoning part | The `function_call` stream handling (`legacy_tools`, `merge_legacy`, `finish_legacy`, `map_native_finish`), `ReasoningDialectDecoder` and `with_reasoning_dialect`, the dead thinking-activation builders (`with_thinking`, `with_enable_thinking`, `with_think`, `with_think_effort`, `with_thinking_budget`, `with_thinking_token_budget`), and OpenRouter `with_preservation_controls` |
| Application/runtime/daemon/composition/client single path | intention-application + intention-runtime + intention-daemon + intention + intention-client | The dead `send_user_turn` variants and `tests/m4_application_scheduling.rs`, the `resolve_for_turn` selection-less fallback, `resolve_for_override`, `invoke_local_tool_once`, the synchronous `StopRun` dispatch arm, the no-tool-port M4 denial branch, `PostCommitPublisher`/`NoopPostCommitPublisher`, and `SessionSubscriptionRecovery` |
| Tooling and meta | intention-tools + intention-workspace + tests/ + docs archive | The bare-result compatibility trio (`dispatch`, `invoke`, `invoke_with_context`), the legacy `exit_code:` rendering and `-1` sentinel, `resolve_path_for_tool`, and the orphan top-level `tests/` tree |
| Policy/docs consolidation | quality/ + docs registers + final validation | The `quality/architecture.toml` target and wording rows, self-test fixtures, reconciliation registers, coverage re-verification, and the final validation matrix |

Later state (2026-09-30): the legacy M4 bridge removal stands, and the tag ledger it edited is itself removed by
[ADR 0012](0012-typed-serde-json-contracts.md); the whole execution-meaning codec and its goldens are removed by [ADR
0012](0012-typed-serde-json-contracts.md), not only the V3 record; the capability gates named in Wave 3 are removed by
[ADR 0011](0011-local-json-rpc-2-0-transport.md), the protocol version is 2.0, and the "keep the 1.1 negotiation gates"
instruction is void; the single live SQLite schema stays, while the control-plane tables and
`control_plane::SCHEMA_M5_SQL` belong to the not-activated M5+ Slice 2 control plane; the single-TOML-shape removal
stands, and the control-plane candidate machinery in its keep list belongs to that same not-activated slice; the
provider and reasoning single-path removals stand, while the Slice 2 reasoning/catalog families belong to the
not-activated slice; Wave 7's selection-carrying-only instruction and its Slice 2 keep list (catalog, queue promotion
and reconciliation, held-run admission) are superseded, and selection-less turn acceptance is the only live path; the
tooling and meta removals stand, with the removed glob match cap covered by [ADR
0014](0014-limits-by-precedent-and-no-content-scanning.md); and the policy and pin
consolidation stands under the protocol, codec, root, and limit changes of ADRs 0011 to 0014. The wave rows name the
removed surfaces at family granularity; the exact per-symbol inventories live in the wave commits.

Retained by design in the same program: the incompatible-major rejection path with its golden
`hello-incompatible-major-v2.json`; `ensure_run_journals`, `snapshot_model_runs`, and the direct full-schema create path
in `open()` with its regression test; modern `tool_calls` merge/finish, `provider_reasoning_stream_invalid`, and
`with_reasoning_effort`; `stop_run_for_daemon_host`, `terminalize_cancelling_run_for_daemon`, the daemon async host
path, and the protocol `StopRunCommandDto`; `committed_tool_result_evidence` and the `HostCommitObserver` publication
path; and `dispatch_with_cancellation` / `invoke_enveloped_with_cancellation` with the typed `ToolProcessStatus`, to
which the coverage call sites migrated.

The Slice 2 candidate machinery that Wave 5's keep list named (`parse_candidate`, `semantic_equivalence`,
`classify_changed_fields`, `reject_catalog_affecting_edits`, and the candidate DTOs) belongs to the not-activated
control plane and is not implemented; the credential-free `redacted_safe_digest`, the dead
`CandidateAcceptanceOutcomeDto` projection, and config's private SHA-256 module have no production consumer.

The final validation matrix: `cargo test --workspace`, `cargo nextest --workspace --all-targets --locked
--no-fail-fast`, `cargo clippy --workspace --all-targets --locked -- -Dwarnings`, `cargo fmt --all -- --check`, `make
quick`, `make verify`, `make docs-check`, `check_architecture.py`, plus targeted package tests for every changed crate.
The quality self-test suite named by the original matrix was removed 2026-10-02 (architecture 12).

## Ownership

Semantic canonical records/tags belonged to `intention-domain`; public wire and frames to `intention-protocol`; storage
to `intention-storage` and `intention-storage-sqlite`; registry/typed tool contracts to `intention-tools`;
provider-private translation to provider crates; process/publication to `intention-daemon`; concrete assembly to
`intention`; adapters to `intention-client`, then TUI/Tauri. No new crate, dependency, feature, coverage tier, or
exclusion is introduced.

## Evidence

Per wave: the removed path had no production caller or only compatibility consumers; the current-path tests still pass;
`make quick` and the targeted package tests pass; the wave's documentation updates are in the same change; and the final
validation matrix above passes. Coverage is re-verified with `make verify` after the removals, per [ADR
0016](0016-per-crate-coverage-tiers.md); documentation checks (`docs-check`, `check_architecture.py`) must pass after the
same-change documentation updates listed per wave.

## Non-goals

This record does not implement M6-M9 behavior, does not introduce a second runtime, registry, scheduler, persistence
authority, or sandbox, and does not remove any roadmap reservation, approved skeleton crate, or current Slice 1
functionality.

## Resolution notes

- **Why this policy now governs:** AGENTS.md is the authoritative engineering
context and its "No backward compatibility" section (main `b5fa71e`) conflicts with preservation commitments previously
recorded in now-deleted decision records and in the architecture documents. Those commitments are superseded here as
recorded above.
- **ADR 0004 authority:** ADR 0004 remains the activation home and
slice-sequence authority. ADR 0005 changes implementation and documentation obligations without renumbering or
reauthorizing slices 1/2 and without reopening closed milestones; closeout evidence stays immutable provenance, and only
active indexes/links and command/evidence rows that reference removed test targets may change.
- **Physical version interpretation:** the concrete protocol version is 2.0
after [ADR 0011](0011-local-json-rpc-2-0-transport.md), which raised the constant with the JSON-RPC wire change; the
recorded point, "exact equality" removes minor tolerance, not the version constant itself, stands. The SQLite physical
DDL previously labeled "schema 4" is retained as the one current schema (logical version 1); no version marker or
migration machinery accompanies it.
- **Transition of reconciliation/evidence rows:** rows that record removed
behavior (migration, preservation, legacy bridge, 1.0 compatibility) are rewritten or retired in the same change as the
code removal; rows recording retained current behavior stay.
