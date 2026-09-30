# Test audit report: remaining useless tests

Repository: `/home/data/intention-relay`, branch `quality/coverage-and-test-slimming`.
This audit follows the first slimming pass on this branch (~80 tests deleted or trimmed, 5 obsolete test files removed, all uncommitted) and covers what remains.

**Audit record:** the rows below were reviewed, approved in full, and executed on this branch; see [Execution status](#execution-status) for the outcome and deviations.

Method: seven read-only sub-agent audits over disjoint zones. Every "redundant" finding names the covering test, and the auditor read that test to confirm. The controller re-verified the headline findings by reading the cited code (see [Controller verification](#controller-verification)).

Categories:

| Category | Meaning | Recommended action |
|---|---|---|
| 1 checks-nothing | No assertion, vacuous assertion, tautology, or discarded result | delete |
| 2 redundant | Fully covered by the named test | delete |
| 3a misleading and redundant | Name lies and the behavior is covered elsewhere | delete |
| 3b misleading but unique | Name lies; the behavior is real and unique | repair (do not delete) |
| 4 outdated | Pins removed behavior, formats, or caps | delete or trim |

Status: all seven zones are complete and verified.

## Summary

| Zone | Rows | Delete-recommended | Repair / rename |
|---|---|---|---|
| Wire stack: `intention-protocol`, `intention-transport`, `intention-client` | 7 | ~111 LOC | 1 rename, 2 in-place repairs |
| Workspace plumbing: `intention-config`, `intention-tui`, `intention-test-support`, `intention-workspace`, `intention-hooks` | 13 | ~78 LOC (98 with the two dead `apply_result` arms) | 1 fixture repair, 3 renames |
| Tools: `intention-tools` | 12 | ~263 LOC + ~7 helper lines | 5 in-place repairs, 1 tail trim |
| Domain and types: `intention-types`, `intention-domain` | 14 | ~106 LOC | 4 renames, 1 trim (~25 LOC) |
| Application and runtime: `intention-application`, `intention-runtime` | 17 | 12 tests, ~391 LOC | 5 tests, ~234 LOC |
| Daemon: `intention-daemon` | 8 | 7 tests, ~340 LOC incl. one helper | 2 tests |
| Storage, model, providers, facade: `intention-storage`, `intention-storage-sqlite`, `intention-model`, providers, `intention` | 11 | ~165 LOC | ~25 LOC |
| **Total** | **82** | **≈ 1 450 LOC + 7 helper lines** | **≈ 25 repair/rename rows** |

## Wire stack: `intention-protocol`, `intention-transport`, `intention-client`

No checks-nothing findings and no deleted-binary-codec leftovers in these crates. `intention-client/src/lib.rs` has no in-file tests; its reducer is covered by `tests/run_stream_contract.rs`.

### Findings

| # | Location | Test / defect | Cat | Evidence | LOC | Action |
|---|---|---|---|---|---|---|
| 1 | `intention-protocol/src/lib.rs:1793-1863` | `protocol_round_trip_covers_all_closed_enum_shapes` | 2 | Every assertion is covered with full equality: all eight request payloads at `tests/contracts.rs:95-169` (payload equality in the loop), `Accepted(StopRun)` and both `Rejected` payloads at `contracts.rs:272/284/328`, `RunSubscription(Error)` at `intention-daemon/tests/m4_streaming_foundation.rs:924-941`. The body also discards decode results (1846, 1859) and a dead fixture (1862) | 71 | delete (or add two asserts to the response loop at `contracts.rs:227-331`) |
| 2 | `intention-protocol/src/lib.rs:1870-1886` | `bad_tail` block in `remaining_constructor_and_deserialization_error_paths_are_checked` | 4 (3a) | The fixture nests the event identity under `"metadata"`, but `EventEnvelopeDto` flattens `EventMetadataDto`, so deserialization fails on missing fields before the tail-contiguity rule it targets; the intended rejection is covered at `src/lib.rs:1671-1697` | 17 | delete, or repair the fixture to the flat shape to cover tail deserialization |
| 3 | `intention-protocol/src/lib.rs:1888-1902` | snapshot/tail session mismatch block (same test) | 2 | Covered with the exact code at `tests/contracts.rs:451-494` (`invalid_subscription_response` at 492) | 15 | delete |
| 4 | `intention-protocol/src/lib.rs:1904-1911` | `RunLiveBatchDto` `u64::MAX` + empty facts block (same test) | 3a | The empty-facts guard fires first, so the overflow branch never runs; the same rejection with its code is covered at `tests/m4_run_stream_contracts.rs:120-141`. Only `u64::MAX` run-cursor use in the workspace | 8 | delete; repair with non-empty facts if the overflow boundary matters |
| 5 | `intention-transport/src/lib.rs:975-1009` | `async_listener_enforces_private_permissions_and_removes_owned_socket` | 3b | Name says the socket is removed, but the body asserts it remains after drop ("leaves its endpoint for reclaim", 1003-1006) and only reclaims at the next bind; the sync twin (`:866-901`) names the same behavior correctly | 35 | rename only |
| 6 | `intention-client/tests/run_stream_contract.rs:401-464` | `request_replay_rejects_mismatched_correlation_and_error_reply_without_mutating_state` | 3b | The loop scripts two distinct failures but asserts a disjunction (458-461), so a swapped mapping passes both iterations | 64 | repair: assert the exact code per iteration |
| 7 | `intention-protocol/tests/m4_run_stream_contracts.rs:52-118` | `run_stream_dtos_round_trip_and_preserve_all_resync_reasons` | 3b | The `Replay`/`LiveBatch`/`Snapshot` decodes are discarded (`let _:` at 97, 99-101); only the resync loop compares equality | 67 | repair: assign and assert the decoded values against the originals |

### Keep-exemplars (do not touch)

- `intention-protocol/tests/contracts.rs:95-169` exhaustive method table; `:415-450` hello gate; `:451-494` boundary rejections with exact codes.
- `intention-protocol/tests/m4_run_stream_contracts.rs:120-150` invalid-batch table with additive tolerance.
- `intention-transport/src/lib.rs:1052-1093` socket-replacement race; `:1121-1145` silent-peer read bound; `tests/transport_integration.rs:178-231` 32-frame concurrent exchange.
- `intention-client/tests/run_stream_contract.rs:254-273` atomic replay-tail application.

## Workspace plumbing: `intention-config`, `intention-tui`, `intention-test-support`, `intention-workspace`, `intention-hooks`

`intention-tui` and `intention-test-support` (`tests/fixture_host.rs`) are clean; `intention-config` and `intention-hooks` are otherwise clean. No outdated findings in this zone.

### Findings

| # | Location | Test / defect | Cat | Evidence | LOC | Action |
|---|---|---|---|---|---|---|
| 1 | `intention-hooks/tests/hook_contracts.rs:68` | `rejection_is_typed_and_stops_before_execution` | 3a | Only asserts the `Reject` outcome; nothing checks that later hooks stop. Covered exactly by `hook_contracts.rs:87` (registers `RejectingHook` + `EffectHook`, asserts `!called`) | 18 | delete |
| 2 | `intention-hooks/src/lib.rs:644` | `input_transform_is_invalid_after_execution` | 2 | Same phase, fixture, and `is_err` assertion as `src/lib.rs:608`; the exact code is asserted by `hook_contracts.rs:339` | 22 | delete |
| 3 | `intention-config/tests/contracts.rs:88` | `generic_chat_preserves_configured_model_identifier` | 2 | Same fixture and kind/model assertions as `src/lib.rs:976`; the redaction assert is covered by `src/lib.rs:935` and `tests/contracts.rs:29` | 17 | delete the integration copy |
| 4 | `intention-workspace/tests/workspace_contract.rs:89` | `unnormalized_and_absolute_input_never_reaches_the_anchor` | 3a | Asserts `intention-types` parse rejection (covered at `intention-types/tests/error_contracts.rs:90`) and `Path::join` semantics that hold for any base, so no workspace code is exercised | 13 | delete |
| 5 | `intention-hooks/src/lib.rs:709` and `:720` | the two direct-`apply_result` tests | 1 | `dispatch_with_observability` returns early for `Reject` (294-299) and handles `TransformInput` before the fall-through (319), so those two `apply_result` arms are unreachable in production and the tests exist only to cover them | 20 | delete only together with the two dead arms |
| 6 | `intention-config/tests/snapshot_fixtures.rs:62` | `malformed_config_snapshot_wire_shapes_are_rejected` | 3b | 5 of 7 wires omit the required nested `provider_execution` field (`src/lib.rs:335-342`), so they fail before reaching the version check, unknown kind, blank model/endpoint, or unknown field they target | 0 | repair: add the field to those five wires |
| 7 | `hook_contracts.rs:394` | `fail_open_is_observable_and_fail_closed_is_not_swallowed` | 3b | The hook returns `Ok(Reject)`, never an error, so no failure metadata is emitted and the policy is never consulted; the unique behavior is that a rejection is not swallowed | 0 | rename |
| 8 | `intention-hooks/src/lib.rs:392` | `orders_duplicates_rejects_and_chains_transforms` | 3b | Ordering is covered at `src/lib.rs:465` and `hook_contracts.rs:183`; the unique coverage is the duplicate-id rejection path | 0 | rename |
| 9 | `intention-test-support/tests/composition_contract.rs:146` | `queued_turn_removal_remains_durable` | 3b | Only asserts the command is accepted; durability is never observed | 0 | rename or add a reopen/read-back |
| 10 | `hook_contracts.rs:254` (line inside) | `assert_eq!(FailurePolicy::FailClosed, FailurePolicy::FailClosed);` | 1 | Unit-variant self-comparison; cannot fail | 1 | delete the line |
| 11 | `intention-config/tests/snapshot_fixtures.rs:41` and `:84-87` | `contains(FAKE_CREDENTIAL)` assertions | 1 | Neither test's input ever contains the credential (the fixture carries only `credential_configured`), so the assertions cannot fail | 5 | delete the asserts |
| 12 | `intention-config/tests/m4_contracts.rs:136` (line inside) | `!error.to_string().contains(FAKE_CREDENTIAL)` | 1 | The loop's inputs never contain the credential | 1 | delete the line |
| 13 | `intention-test-support/tests/composition_contract.rs:122` (line inside) | `let _ = queued_turn;` | 1 | Dead statement; the value was already consumed | 1 | delete the line |

### Keep-exemplars (do not touch)

- `intention-config/src/lib.rs:1041` exact credential round trip; `src/lib.rs:935` redacted projection; `tests/m4_contracts.rs:50` boundary table with credential-bearing input.
- `intention-hooks/tests/hook_contracts.rs:339` and `:366` four-phase rejection loops; `intention-hooks/src/lib.rs:731` Continue-vs-TransformInput distinction.
- `intention-workspace/tests/workspace_contract.rs:64` CWD-independent addressing (ADR 0047).
- `intention-tui/tests/tui_contract.rs:35` fixture daemon over a real endpoint; `intention-test-support/tests/fixture_host.rs:54` real transport round trip.

## Tools: `intention-tools`

No outdated behavior pins in this crate: every asserted cap (`64 KiB`, `128 KiB`, `1 MiB`) and error code still exists in source. The three coverage-style files with unique `#[cfg(unix)]`/`Workspace`-scope behavior were kept.

### Findings

| # | Location | Test / defect | Cat | Evidence | LOC | Action |
|---|---|---|---|---|---|---|
| 1 | `tests/tool_contracts.rs:1204` | `execute_inherits_the_invoking_environment` | 1 | `assert!(result.is_ok() \|\| previous.is_some())` cannot fail: the marker is never set, and a non-zero exit is a normalized `Ok` (`src/lib.rs:1731-1761`). Sole attempted coverage of `command.envs(std::env::vars_os())` (`src/lib.rs:1719`) | 37 | repair, do not delete: set the marker and assert `exit_code:0` |
| 2 | `tests/tool_coverage_remaining.rs:49-70` | tail of `logical_paths_cover_all_inputs_and_projections` | 1 | Discarded `value.projection()` plus a `value.tool_id() == value.tool_id()` self-comparison; the `values` array exists only to feed the tail. Keep lines 17-48 (`logical_path()` mapping coverage) | 22 | trim the tail |
| 3 | `tests/tool_contracts.rs:1434` (lines 1469, 1472; comment 1470-1471) | `execute_formats_success_and_truncates_both_streams` | 1 (partial) + 4 | `... \|\| cfg!(windows)` assertions are constant-true on Windows; the comment calls the marker "retained for compatibility", a claim the no-backward-compatibility policy rejects | 2 (comment) | repair: assert the Windows expectation |
| 4 | `tests/tool_contracts.rs:603` | `file_tools_report_policy_and_execution_failures` | 3a | Name claims policy reporting; the body never reads policy or observability. All three `is_err()` probes are weaker duplicates with exact codes elsewhere: `tool_read_failed` via `:652`/`:834`; `edit_target_missing` and `tool_execute_spawn_failed` via `:1476` (asserts 1497, 1522) | 48 | delete |
| 5 | `tests/tool_coverage_contracts.rs:18` | `all_result_projections_and_metadata_fallbacks_are_typed` | 3a | "Fallbacks" were already deleted; the remaining asserts compare projections against the same constants the constructor injects. Covered by `:1866` (schema/tool/cwd/per-tool content) and `:2072` | 28 (+7) | delete; remove the orphaned `path`/`text` helpers |
| 6 | `tests/tool_contracts.rs:187` | `tool_service_covers_write_and_edit_failures` | 3a | Name claims failures; the second half asserts success. `tool_write_failed` with exact code via `:702` (assert 734); empty-`old` edit success via `:1241` (assert 1273) | 40 | delete |
| 7 | `tests/tool_contracts.rs:66` | `tool_service_covers_read_and_edit_success_values` | 3a | No success value is asserted, only variants. Covered by `:1241` (read text, edit variant) and `:1866` (read text, edit bytes); all-variant dispatch by `:1788` | 34 | delete |
| 8 | `tests/tool_contracts.rs:384` | `glob_and_grep_cover_empty_and_invalid_search_paths` | 3b | Name promises empty glob results and invalid paths; the glob half matches the variant only and the grep path is valid. Unique residue: the zero-match grep; empty glob is already asserted at `:928` | 35 | repair/rename: keep the zero-match assertion |
| 9 | `tests/tool_coverage_extra.rs:5` | `invocation_validates_schema_and_call_identity` | 3b | Two bare `is_err()` probes duplicate `tool_call_id_mismatch` (`:1053-1058`) and `tool_schema_mismatch` (`tool_coverage_invocation.rs:52-53`). Unique: the only `TOOL_SCHEMA_VERSION` value pin in the workspace | 21 | repair: keep the version pin, drop the probes (or assert codes) |
| 10 | `tests/tool_contracts.rs:1352` | `read_and_grep_bound_large_content` | 3b | Read half subsumed by `:522`. The grep half asserts `len <= 65_536 + 12`, but grep's retained bound is `MAX_GREP_AGGREGATE_BYTES` = 128 KiB (`src/lib.rs:151`, `:2091-2110`), so it pins a bound the product does not have | 48 | repair (drop the synthetic bound) or delete |
| 11 | `tests/tool_contracts.rs:652` | `file_tools_execute_against_the_declared_workspace` | 2 | Write, Edit, and read-missing assertions are all covered stronger (`:101` on-disk content; `:139` on-disk content; `:603`/`:834`). Unique residual: the write-to-edit sequence is the only cross-tool same-root proof | 49 | delete if that sequence is expendable |
| 12 | `tests/tool_contracts.rs:1782` | `bounded_text_accepts_text_and_rejects_nul` | 2 | The NUL branch is covered through the JSON boundary at `bounded_contracts.rs:40` (the deserializer calls `BoundedText::new`, `src/lib.rs:1247`) | 5 | delete |

### Keep-exemplars (do not touch)

- `tests/bounded_contracts.rs:117` grep aggregate cap with a zero-match window; `:291` conflict leaves the file unmutated.
- `tests/tool_contracts.rs:1476` exact typed error codes; `:523` boundary table; `:1866` independent expected literals per tool; `:1678` schema-vs-serialized cross-check; `:467` sorted, deduplicated glob property.
- `src/lib.rs:2143` `bounded_lossy` boundary unit test.

## Domain and types: `intention-types`, `intention-domain`

### Findings

| # | Location | Test / defect | Cat | Evidence | LOC | Action |
|---|---|---|---|---|---|---|
| 1 | `intention-domain/tests/m3_contracts.rs:88-107` | `m3_runs_require_a_config_revision_in_projections_and_start_events` | 1 | Body is two `let _ =` constructor calls; the in-code comment concedes the contract is compile-time. Both constructors are asserted at `m3_contracts.rs:142-147`, `m3_contracts.rs:191-198`, `m4_durable_facts.rs:22-28` | 20 | delete |
| 2 | `m3_contracts.rs:29-43` | `m3_sessions_require_stable_workspace_identity_in_commands_events_and_projections` | 3a | After slimming the body is one Ok-path construction with an empty queue and no value assertion; subsumed by `m3_contracts.rs:59-71` and `m3_contracts.rs:205-216` | 15 | delete |
| 3 | `tests/event_fixtures.rs:112-114` | self-comparisons in `tool_lifecycle_event_round_trips_and_rejects_unsafe_detail` | 1 | `assert_eq!(event.session_id(), event.session_id())` and the same for `run_id`, `call_id`; cannot fail | 3 | delete the 3 lines |
| 4 | `tests/m5_tool_results.rs:107-119, 153-165, 216-217, 222-223` | cap-pinning assertions in the metadata-uniqueness test | 4 | Pins former 4 KiB content, 16-entry, 128-byte key, and 1 KiB value caps that no longer exist (`src/lib.rs:1002`, `src/lib.rs:1097`, `src/lib.rs:1103-1106`). Keep the just-beyond guards at `:121-133`, `:166-184`, `:220-226` as the ADR 0048 anti-regression guard | 30 | delete the cap asserts, keep the guards |
| 5 | `m5_tool_results.rs:185-199, 227-228, 229-231` | oversized duplicates in the same test | 2 | The 64-entry, 8 KiB, and 16 KiB cases duplicate the 17-entry, 129-byte, and 1025-byte cases; metadata validation is duplicate-key uniqueness and size-independent | 20 | delete |
| 6 | `m5_tool_results.rs:47-71` | envelope block of `tool_result_record_round_trips_with_typed_identity_and_accessors` | 3b | Duplicated by `:299-330` (same payload, payload equality at `:329`); the name still claims "accessors". Keep the bare round trip at `:44-47` | 25 | trim + rename |
| 7 | `intention-domain/src/lib.rs:1767-1773` | inner transition re-check in `tool_result_statuses_map_only_to_terminal_lifecycle_statuses` | 2 | Re-checks four pairs already proven by the exhaustive 49-pair matrix at `src/lib.rs:1723-1746`. Keep the `status.lifecycle_status() == lifecycle` assertion | 7 | delete inner check |
| 8 | `tests/m4_durable_facts.rs:162-165` | `ToolResultRecorded.as_str()` duplicate | 2 | Byte-identical assertion at `tests/m4_durable_fact_wires.rs:55-58` | 4 | delete one copy |
| 9 | `intention-types/tests/contracts.rs:83` | `assert!(!encoded.contains("backtrace"))` | 1 | `ErrorDto` has exactly six named fields and no free-form payload, so it cannot fail. The real leak coverage is `error_contracts.rs:22-25` | 1 | delete the line |
| 10 | `m4_durable_facts.rs:108` | `matches!(event, DomainEventDto::ProviderAttemptStarted(_))` | 1 | The event was built as that exact variant two lines above | 1 | delete the line |
| 11 | `m3_contracts.rs:162-186` | `m3_projection_deserialization_rejects_invalid_nested_turns` | 3b | Name says deserialization; the body calls `SessionProjectionDto::new(...).is_err()`. The foreign-session rejection is real and unique | 0 | rename (or route through `serde_json::from_value`) |
| 12 | `tests/contracts.rs:86-106` | `run_status_matrix_and_terminal_classification_are_complete` | 3b | The matrix half was deleted in the first pass; the body only checks `is_terminal()` for 10 statuses (sole in-package use of `RunStatusDto::is_terminal`) | 0 | rename |
| 13 | `m4_durable_facts.rs:139-174` | `model_fact_validators_cover_success_boundaries_and_all_projection_shapes` | 3b | Body asserts only `kind()` for 5 variants; no boundaries or shapes are checked. Keep: it is the sole coverage of four `ModelRunFactInputDto::kind` arms | 0 | rename |
| 14 | `m3_contracts.rs:272-297` | `plan_transitions_cover_all_allowed_and_rejected_edges` | 3b | All 11 allowed edges asserted, but only 3 of ~46 rejected pairs | 0 | rename or extend |

### Keep-exemplars (do not touch)

- `intention-types/tests/error_contracts.rs:17` and `:82` — leak test with real secret/root inputs, and malformed wire rejection.
- `intention-domain/tests/m3_contracts.rs:227` — exhaustive 10x10 status matrix.
- `intention-domain/src/lib.rs:1702` — 49-pair lifecycle transition matrix.
- `intention-domain/tests/m5_tool_results.rs:235` — exact JSON wire pin with additive tolerance and rejections.
- `intention-domain/tests/m4_durable_fact_wires.rs:118` — identity/cursor mismatch, contiguity, overflow, bound checks.
- `intention-domain/tests/contracts.rs:39` — native absolute-root construction plus serde rejections.

## Application and runtime: `intention-application`, `intention-runtime`

Shorthand: `[A]` = `intention-application/tests/m3_application.rs`; `[AS]` = `intention-application/src/lib.rs`; `[R3]` = `intention-runtime/tests/m3_runtime.rs`; `[R5]` = `intention-runtime/tests/m5_tool_loop.rs`; `[D3]` = `intention-domain/tests/m3_contracts.rs`.

### Findings

| # | Location | Test | Cat | Evidence | LOC | Action |
|---|---|---|---|---|---|---|
| 1 | `[A]:662` | `post_effect_transform_is_applied_sequentially_and_invalid_outcome_fails_terminally` | 3a | Registers one hook; no sequential application is exercised. Identical behavior asserted at `[A]:903` (block 940-973) | 42 | delete; `PostEffectHook` becomes dead |
| 2 | `[A]:1357` | `local_tool_covers_post_execution_hook_errors_and_rejections` | 2 | Reject case covered by `[A]:1145` (assert 1206); DispatchError cases by `[A]:852` (assert 899) and `[A]:1212` (phases 1219-1227) | 37 | delete |
| 3 | `[A]:1794` | `publication_boundary_receives_exact_committed_identity_and_payload` | 2 | Covered by `[A]:2204` block 1 (2231-2249), which adds the stronger `evidence_at_publish == [3]` probe | 36 | delete |
| 4 | `[A]:1110` | `local_tool_hook_transform_result_before_execution_is_rejected` | 2 | Covered by `[A]:767` phase loop (819-848), which also pins the exact error message | 34 | delete |
| 5 | `[A]:586` | `local_tool_invalid_result_outcome_is_rejected_before_execution` | 2 | Same `[A]:767` phase loop | 34 | delete |
| 6 | `[A]:621` | `local_tool_cancellation_records_one_external_effect_terminal_event` | 3a | Name claims the external-effect-unknown status, but the signal is pre-cancelled so the status is `Cancelled`; covered by `[A]:2204` block 3 and `[A]:1497` | 40 | delete |
| 7 | `[A]:1989` | `cancelled_results_never_reach_the_publication_boundary` | 2 | Covered by `[A]:2204` block 3 (2302-2319): single last correlated `Cancelled` terminal, empty publications, empty `evidence_at_publish` | 43 | delete |
| 8 | `[A]:1456` | `local_tool_propagates_append_persistence_error` | 2 | Covered by `[A]:2033` scenario `("started-commit", 2, None, 1)` (list at 2071), which asserts both the propagated code and the durable event count | 10 | delete; `tool_error_after` becomes dead |
| 9 | `[AS]:1650` | `failure_documents_carry_the_terminal_discriminator_and_code` | 2 | Covered by `[A]:2385` (2460-2464, 2496-2500, 2561-2565) through the public path | 22 | delete |
| 10 | `[AS]:1501` | `maps_terminal_error_statuses` | 2 | Covered by `[A]:2204` (2272-2278, 2307-2313, 2371-2377) on persisted events | 18 | delete |
| 11 | `[R3]:283` | `status_graph_allows_declared_edges_and_rejects_forbidden_edges` | 2 + dead surface | The 9x9 matrix is covered by `[D3]:227`; `RuntimeService::can_transition` (`intention-runtime/src/lib.rs:88-91`) has no production consumer | 19 | delete the test and the dead wrapper |
| 12 | `[R5]:1407` | `port_infrastructure_error_terminalizes_without_leaking_text` | 3b | Injects `"sensitive provider detail"` but never asserts its absence; all assertions pin codes/retry/status. Sole coverage of the port-`Err` path | 60 | repair: assert the absence in the recorded appends |
| 13 | `[A]:466` | `lifecycle_details_redact_secret_content_and_absolute_workspace_root` | 3b | `assert!(!rendered.contains("FAKE_SECRET_9f3a"))` can never fail (the literal appears only in another test's hook); the absolute-root assertion is real | 26 | repair: drop the vacuous line or wire a real secret |
| 14 | `[A]:1212` | `local_tool_covers_dispatch_errors_and_post_effect_result_transforms` | 3b | The post-effect half registers `TransformResult` but asserts only `matches!(result, ToolResult::Read(_))`; a dropped transform passes | 100 | repair: assert the transformed content; the dispatch-error half stays |
| 15 | `[A]:1062` | `local_tool_workspace_and_execution_hooks_cover_transform_and_rejections` | 3b + partial redundancy | Name promises rejections but no `Reject` hook is registered; the transform result is unverified; its `BeforeWorkspaceResolution` half duplicates `[A]:767` | 47 | repair: pin the transformed result and rename; drop the redundant half |
| 16 | `[AS]:1673` | `durable_evidence_binds_exact_identity_kind_and_time` | 2 | Identity/kind/content/time all duplicated by `[A]:2385`; the only unique assertion (the `"unknown"` tool-id rejection) is unreachable from public entries | 56 | delete (or trim to that one assertion if the guard stays) |
| 17 | `[A]:1058` (in test 1020) | `let _ = GetSessionSnapshotQueryDto::new(session_id);` | 1 | Constructs and discards a DTO; no assertion, no effect | 1 | delete the statement |

### Keep-exemplars (do not touch)

- `[A]:2033` append-failure atomicity matrix; `[A]:2204` terminal ordering + publication probe; `[A]:2385` typed evidence matrix.
- `[A]:1593` the correct redaction pattern (real secret, exact observations); `[A]:1395` spawn-observed cancellation.
- `[R5]:1917` gated sequential tool ordering; `[R5]:2393` retry delay never armed; `[R5]:1153` textless channel; `[R5]:2187` gate-then-append ordering.
- `intention-runtime/tests/m4_runtime_failure_helpers.rs:181` — sole coverage of `fail_starting_run`.

## Daemon: `intention-daemon`

### Findings

| # | Location | Test | Cat | Evidence | LOC | Action |
|---|---|---|---|---|---|---|
| 1 | `src/lib.rs:2021` | `host_stop_without_an_admitted_task_terminalizes_and_cleans_the_registry` | 2 | Covered by `tests/m4_streaming_foundation.rs:971`, which uses deterministic cleanup instead of 20-yield polling | 37 | delete (covering test is `test-support`-gated) |
| 2 | `src/lib.rs:2059` | `host_stop_signals_the_registered_task_and_executor_owns_cancelled_terminal_state` | 2 | Covered by `tests/m4_streaming_foundation.rs:606`; the residual `tasks.is_empty()` duplicates test 5 below for the same code line | 57 | delete; keep exactly one of tests 2/5 |
| 3 | `src/lib.rs:2264` | `listener_accepts_and_dispatches_one_typed_health_query` | 2 | Sync dispatch is covered through the real consumer at `intention-tui/tests/tui_contract.rs:37`; the only unique target is the `#[cfg(test)]` wrapper `serve_next_connection` (`src/lib.rs:1479-1500`, sole call site `src/lib.rs:2272`); the trailing 1 ms sleep asserts nothing | 30 + 23 helper | delete the test and the wrapper (`#[cfg(test)] use std::thread` becomes unused) |
| 4 | `src/lib.rs:1728` | `async_host_keeps_m3_requests_and_run_streams_on_one_listener` | 3a | The subscription check at 1832-1835 matches `RunSubscription(_)`, which also matches `Error`/`Resync`, so a failed registration cannot fail the test; turn-over-host plus replay is covered by `m4_streaming_foundation.rs:501` and `facade_e2e.rs:639` | 110 | delete (covering m4 test is `test-support`-gated) |
| 5 | `src/lib.rs:1920` | `host_executes_starting_run_once_and_publishes_durable_live_batches` | 2 | Covered by `m4_streaming_foundation.rs:501` (`executions() == 1`, live facts, reconnect replay `Completed`) | 63 | delete; keep one of tests 2/5 |
| 6 | `tests/m4_streaming_foundation.rs:1177` | `daemon_stop_seam_persists_cancelling_without_direct_terminalization` | 2 | Covered by `intention/src/lib.rs:1365` (stop at 1412, `Cancelling` at 1419-1430) and `intention/src/lib.rs:1667` (stop at 1709; 1719) | 19 | delete |
| 7 | `src/lib.rs:1984` (tail 2012-2018) | `duplicate_or_unknown_admission_never_creates_an_extra_task` | 1 | The trailing block ends with `host.schedule_if_starting(session_id, run_id);` and asserts nothing; the test passes even if extra state is recorded. The first half is the meaningful dedupe pin | 7 | repair: assert the registry stays empty and the run stays `Cancelling`, or drop the tail |
| 8 | `src/lib.rs:1839` | `one_connection_serves_requests_and_run_frames_together` | 3b | Matches any `RunSubscription(_)`, so the "subscription registered" premise is unverified (a failed registration leaves the connection serving) | 53 | repair: match `RunSubscriptionResponseDto::Replay(_)` |

### Keep-exemplars (do not touch)

- `src/lib.rs:2117` subscriber admission scope/eviction; `:2218` tool-decoder coverage (non-vacuous both ways); `:1202` paused-clock 10 s deadline; `:1893` stale-hello version mismatch; `:2252` endpoint-in-use.
- `tests/m5_tool_loop_wiring.rs:251` and `:366` — the ungated real tool-loop proof and its typed failure.
- `tests/m4_streaming_foundation.rs:1026` (terminalizer retry) and `:1092` (first-append-gate race).
- `tests/facade_e2e.rs:639` — strongest end-to-end proof.

## Storage, model, providers, facade

### Findings

| # | Location | Test / defect | Cat | Evidence | LOC | Action |
|---|---|---|---|---|---|---|
| 1 | `intention/src/lib.rs:1942` | `provider_driver_branches_and_empty_test_driver_stream_are_exercised` | 1 | `let _ = openrouter.driver();` plus a stream that is pinned and dropped without a poll; no assertion. Only line coverage of the `OpenRouter` driver arm would drop | 40 | delete |
| 2 | `intention-model/tests/model_contracts.rs:375` | `model_message_legacy_wire_still_decodes` | 4 (+3a) | The "legacy" wire is the current wire (`tool_calls`/`tool_call_id` are `Option` with `skip_serializing_if`); single-version policy means no legacy format exists. Covered by `:108` | 21 | delete |
| 3 | `intention/src/lib.rs:1830` | `command_rejects_malformed_turn_identifier` | 4 (3b) | The "malformed" id is a valid `TurnId::new()` UUID; the OR-assert accepts `daemon_command_unavailable` | 11 | repair: assert the single real code and rename, or delete |
| 4 | `intention/src/lib.rs:1923` | `subscription_accepts_exact_checkpoint_and_rejects_unknown_position` | 3a | No rejection half exists; acceptance is covered at `:1565`, rejection at `:1592` | 18 | delete |
| 5 | `intention/src/lib.rs:1807` | `selected_provider_rejects_configuration_kind_mismatch` | 1 | `Ok(_) => return` passes if the facade accepts the mismatch it targets | 22 | repair: `expect_err` |
| 6 | `intention-storage-sqlite/tests/sqlite_contracts.rs:224` | `append_tool_lifecycle_event_preserves_sequence_and_rolls_back_on_fault` | 3a | Never calls `arm_fault`; assertions restate `sqlite_contracts.rs:149`. Rollback is covered by `src/lib.rs:2544` and `src/lib.rs:2638` | 26 | delete (or fold the tail-length check into `:149`) |
| 7 | `intention/src/lib.rs:1511` | `config_loading_redacts_raw_toml_and_creates_a_fresh_safe_snapshot` | 2 | Redaction already asserted for both provider kinds at `:1432` (1461-1476); the only unique surface is the `#[cfg(test)]` wrapper | 28 | delete (wrapper keeps a consumer at `:1560`) |
| 8 | `intention-provider-openrouter/tests/openrouter_contracts.rs:174` (portion 197-220) | finish/error fixture loops in the wrong-kind test | 2 (partial) | The finish loop exercises only the fixture-only `map_fixture_finish`; the production path is covered at `src/lib.rs:753`. The redaction loop is vacuous (`map_fixture_error` discards the message) | 24 of 48 | trim; keep the wrong-kind rejection |
| 9 | `sqlite_contracts.rs:948-949` | stale doc comment on `completed_result_evidence_is_durable_across_reopen_with_redacted_payload` | 4 | Claims a source-directory scan the test does not perform; orphaned from a deleted seed-guard test | 2 | delete the comment lines |
| 10 | `intention-storage/tests/contracts.rs:88-89, :303` | `accepts_repository` never called plus `let _ =` pins | 1 | Compile-time existence pins, no assertions | 3 | trim or justify as an object-safety pin |
| 11 | `intention/src/lib.rs:1842` | `daemon_execution_bridge_runs_selected_test_driver_and_tool_bridge_reports_safe_error` | 3b | Never drives a scheduled run; the bridge itself is exercised in daemon tests | 0 | rename or extend |

### Keep-exemplars (do not touch)

- `intention-storage/tests/opaque_json_guard.rs:433` boundary guard (and its parser-coverage siblings).
- `sqlite` `m4_durable_facts.rs:101` replay cursors; `m4_model_context.rs:102` 32-iteration race with an explicit oracle; `m4_run_safe_config.rs:190` unavailable-vs-not-found.
- `sqlite src/lib.rs:2638` fault injection at every stage.
- `intention/src/lib.rs:2104` stop-fencing and `:2027` in-flight cancellation.
- `intention-model/tests/m4_execution_contracts.rs:114` ordered events with drop accounting.

## Controller verification

The following headline findings were independently re-read and confirmed (not just trusted from the auditors):

- `RuntimeService::can_transition` has no production consumer (only the definition at `intention-runtime/src/lib.rs:89` and the test's four calls).
- `serve_next_connection` is defined at `intention-daemon/src/lib.rs:1479` and called only from the test at `:2272`.
- `RunSubscriptionResponseDto` has `Replay`/`Resync`/`Error` variants, so the daemon findings 4 and 8 are valid.
- The daemon test 7 tail ends with an unasserted `schedule_if_starting` call.
- `execute_inherits_the_invoking_environment` (tools) cannot fail: the marker is never set by the test, and a non-zero exit is a normalized `Ok` result (`intention-tools/src/lib.rs:261-270`).
- `file_tools_report_policy_and_execution_failures` (tools) asserts only three bare `is_err()`.
- `m3_runs_require_a_config_revision_in_projections_and_start_events` contains only `let _ =` calls.
- `[A]:767` genuinely covers all four phases including the exact `invalid_hook_outcome` message (application findings 4, 5).
- `[A]:2204` block 3 asserts a single last correlated `Cancelled` terminal and empty publications (application findings 6, 7).
- `[A]:2033` scenario `started-commit` asserts error code and durable count (application finding 8).
- `[A]:2385` asserts typed terminal documents with exact content through the public path (application finding 9).
- `[A]:662` registers a single hook; no sequential application exists (application finding 1).
- `intention/src/lib.rs:1807` uses `Ok(_) => return` (finding 5); `:1830` uses a valid UUID with an OR-assert (finding 3); `:1923` has no rejection half (finding 4); `:1942` discards the driver and never polls the stream (finding 1).
- `sqlite_contracts.rs:224` never calls `arm_fault`; `:948-949` is a stale comment (findings 6, 9).
- `model_message_legacy_wire_still_decodes` decodes exactly the current wire (finding 2).
- Workspace plumbing: `dispatch_with_observability` intercepts `Reject` and `TransformInput` before the fall-through that calls `apply_result`, so the two direct-call tests cover dead arms (finding 5); the five malformed snapshot wires omit the required nested `provider_execution` and fail on the missing field (finding 6); `rejection_is_typed_and_stops_before_execution` asserts only the outcome while `rejection_skips_later_hook_effects` asserts the stop (finding 1).
- Wire stack: the method-table test at `contracts.rs:95-169` round-trips all eight request payloads with payload equality (finding 1); `EventEnvelopeDto` flattens `EventMetadataDto`, so the `bad_tail` fixture fails on missing fields before the contiguity rule (finding 2); the async listener test asserts the socket remains for reclaim while its name says "removes" (finding 5).
- Tools: `file_tools_report_policy_and_execution_failures` asserts only three bare `is_err()` and is confirmed redundant against `:1476` (exact codes); `execute_inherits_the_invoking_environment` is the sole attempted coverage of the environment-inheritance line, so it is kept as a repair candidate rather than a deletion.

## Dead code that falls out of the deletions

- `PostEffectHook` (`[A]:64-70`, impl `[A]:137-150`) after deleting application finding 1.
- `FakeRepository::tool_error_after` (`[A]:234, 252, 264-269`) after deleting application finding 8.
- `RuntimeService::can_transition` (`intention-runtime/src/lib.rs:88-91`) after deleting application finding 11.
- `serve_next_connection` (`intention-daemon/src/lib.rs:1479-1500`) and `#[cfg(test)] use std::thread` after deleting daemon finding 3.
- `path`/`text` helpers (`tests/tool_coverage_contracts.rs:10-15`) after deleting tools finding 5.

## Execution status

Executed with seven zone workers (one per zone), each restricted to its own focused test targets; the controller ran the full gates afterwards and applied three follow-ups.

- All 82 rows were executed: deletions, trims, repairs, and renames as listed above.
- Controller follow-ups: `tool_coverage_contracts.rs` (left with zero tests) was deleted and removed from `quality/architecture.toml` and the `quality/self_test.py` pin; three residual name/body mismatches were renamed (`logical_paths_cover_all_inputs`, `tool_schema_version_is_current`, `model_projection_and_snapshot_keep_safe_terminal_shapes`).
- Deviations: tools finding 1 probes the guaranteed-inherited `PATH` instead of setting a fresh marker (edition 2024 makes `set_var` unsafe and the workspace denies `unsafe`); storage finding 5 diverges with `unreachable!` instead of `expect_err` (`DaemonApplicationFacade` has no `Debug`, and `clippy::panic` is denied workspace-wide); daemon rows 2/5 kept row 2 because both bodies assert the registry cleanup.
- Gates after execution: `make quick` green (484 passed, 2 skipped, formatting and clippy clean); coverage from clean artifacts green (`intention-daemon` 80.33% in the isolated run, workspace aggregate 92.22%, base 80%); `make deps` green after the orphaned `serde_json` dev-dependency was removed from `intention-tui`; `make quality-self-test` green.
