# PR #36 Independent Review Findings (pr24-review-1)

Complete, uncompressed register of every finding produced by the independent review
pass over pull request #36, with location, defect mechanism, observed evidence,
consequence, remediation, and verification method for each entry.

## Review metadata

| Field | Value |
| --- | --- |
| Repository | `/home/data/intention-relay` (remote `https://github.com/y0tsy/intention-relay.git`) |
| Pull request | #36, `feat(m5+): deliver the M5+ retrospective stack (ADR 0037-0041)` |
| Branch under review | `impl/m5plus-reasoning-roundtrip` |
| Head commit at review time | `1180f69` (`docs(m5+): record the final live run and gate state`) |
| Base | `origin/main` at `b5fa71e` |
| Diff size | 58 commits, 192 files, +57,208 / -6,660 lines |
| Review date | 2026-09-24 |
| Reviewer setup | Seven independent read-only worker subagents (medium complexity tier), one per area, each with the same rule set; plus direct re-verification of the two highest-impact claims by the consolidator (this document's author) |
| Delivery scope of the reviewed PR | M5+ Slice 2 control plane (ADR 0037), no-backward-compatibility removal waves 1-9 (ADR 0038), request-side tool advertisement (ADR 0039), opt-in live-provider e2e (ADR 0040), same-run reasoning round trip (ADR 0041), provider SDK graph refresh (`openrouter-rs` 0.16.0, `async-openai` 0.42.0, `rustls` 0.23.45) |
| Gate state before the review | `make verify` exit 0 at `722a8f4`; `make quick` 1040 passed at `1180f69`; `make e2e-real-api` 2 passed in 18.32s at `722a8f4`; CI on PR #36 green (9/9 jobs); merge blocked by `REVIEW_REQUIRED` |

### Review method

Each reviewer received a self-contained brief containing:

1. the PR context (branch, head, base, diff size, ADR scope);
2. its path scope and the exact `git diff origin/main...HEAD -- <paths>` command;
3. the repository rules to judge against, quoted from `AGENTS.md` and the architecture
   documents: DTO-first boundaries (`architecture/02-dto-and-contract-policy.md`),
   the single-live-version and no-backward-compatibility policy (ADR 0038), minimum
   change and no speculative surface, generalizability (no ad-hoc code serving exactly
   one caller, fixture, model name, or tool name), correctness (panics on reachable
   paths, ordering, casting, TOCTOU, lock scope, blocking in async, transaction
   atomicity, typed error mapping, fail-closed behavior), security (no credentials in
   code, fixtures, DTOs, logs, errors, snapshots; redaction; `WorkspaceRoot` boundary;
   no hard-coded POSIX paths in production code), test adequacy and boundary coverage,
   coverage-tier declarations, and documentation-to-code consistency;
4. the discipline rules: verify before flagging, trace a concrete trigger path, check
   for intentional sibling patterns, no stylistic nits, no defensive "what-if"
   findings, prefer few high-confidence findings;
5. a fixed JSON output schema (`area`, `reviewed`, `assessments`, `findings`,
   `verified_sound`, `uncertainties`) with a 12-finding cap and the priority ladder
   P0 (blocking: certain crash, exploit, or data loss), P1 (high-confidence
   correctness, security, or contract violation), P2 (plausible defect with a
   described trigger path that the reviewer could not fully execute), P3 (minor but
   real defect).

Reviewers were forbidden to modify, stage, or commit anything, to run builds or
tests, and to read `target/`, `.env`, or the gitignored live-run reports. All
reviewer conclusions therefore come from reading the code, its tests, and the
governing documents; no dynamic evidence is claimed except where this document says
the consolidator re-verified something directly.

### Finding counts

| Priority | Count | Meaning used in this document |
| --- | --- | --- |
| P0 | 0 | No finding with a certain, unconditional crash, exploit, or data-loss path was established |
| P1 | 3 | High-confidence defect that breaks delivered functionality or a declared contract |
| P2 | 17 | Defect with a described trigger path; verification of the trigger was partly analytical |
| P3 | 38 | Real but bounded defect: dead or speculative surface, error-mapping nuance, test gap, or stale documentation |
| **Total** | **58** | 57 findings returned by the seven area reviewers plus finding `P3-38`, derived and verified directly by the consolidator |

Correction note: the consolidated chat summary delivered before this document stated
"P3 = 38" on top of the reviewer reports. The accurate reviewer count is 37 P3
findings; finding `P3-38` (stale in-repo NOTE claiming the session provider profile
transaction rolls back, plus the missing end-to-end read-back assertion) was added by
the consolidator after direct verification. The totals in this document are the
corrected ones.

### Index of all findings

| ID | Priority | Area | Location | Title |
| --- | --- | --- | --- | --- |
| P1-01 | P1 | Domain layer | `crates/intention-domain/src/provider_selection.rs:647` | Canonical selection and profile digests are dead; production hashes four ad-hoc shapes |
| P1-02 | P1 | Protocol and client | `crates/intention-client/src/lib.rs:52` | Client never advertises `provider_profiles_v1`, so the real daemon rejects all control-plane calls |
| P1-03 | P1 | Composition and daemon | `crates/intention/src/lib.rs:1450` | Restart with an edited catalog field leaves catalog and configuration snapshot divergent |
| P2-01 | P2 | Domain layer | `crates/intention-domain/src/reasoning_history.rs:39` | Reasoning DTO and validator surface has no consumer and duplicates a bound rule |
| P2-02 | P2 | Domain layer | `crates/intention-domain/src/provider_selection.rs:421` | `selection_canonicalization_version` is not validated as a closed value |
| P2-03 | P2 | Protocol and client | `crates/intention-protocol/src/contract_families.rs:1646` | Control-plane DTOs carry an unchecked `schema_version: String` |
| P2-04 | P2 | Protocol and client | `crates/intention-protocol/src/contract_families.rs:2392` | `ReconcileUnavailableQueueCommandDto.page_cursor` is never read by any producer |
| P2-05 | P2 | Protocol and client | `crates/intention-protocol/src/negotiation.rs:129` | Capability gate helpers have no production caller; the daemon re-implements the gate per variant |
| P2-06 | P2 | Protocol and client | `crates/intention-protocol/src/contract_families.rs:3910` | Protocol duplicates the intention-model reasoning, header, and parser families with no consumer |
| P2-07 | P2 | Protocol and client | `crates/intention-protocol/src/contract_families.rs:2644` | Nine control-plane event DTOs have no producer and no consumer |
| P2-08 | P2 | Protocol and client | `crates/intention-client/src/lib.rs:769` | Run-stream client accepts the caller's schema version instead of the current one |
| P2-09 | P2 | State and configuration | `crates/intention-storage-sqlite/src/lib.rs:669` | `UNIQUE(workspace_root)` violation maps to `storage_unavailable`, not a typed conflict |
| P2-10 | P2 | State and configuration | `crates/intention-storage/src/lib.rs:1198` | Public storage DTOs carry opaque untyped JSON strings |
| P2-11 | P2 | State and configuration | `crates/intention-storage-sqlite/src/control_plane.rs:3091` | Removal-evidence decoder slices JSON strings without unescaping |
| P2-12 | P2 | Application services | `crates/intention-application/src/session_selection.rs:690` | `by_profile` usage totals are labelled with the last row's revision and model |
| P2-13 | P2 | Application services | `crates/intention-application/src/provider_catalog.rs:714` | Catalog acceptance commits durably before the all-or-nothing registry build |
| P2-14 | P2 | Composition and daemon | `crates/intention-transport/src/lib.rs:883` | Stale-socket probe can misjudge a live listener and `Drop` unlinks another host's socket |
| P2-15 | P2 | Model and providers | `crates/intention-model/src/lib.rs:1427` | Six public model types have no production consumer |
| P2-16 | P2 | Model and providers | `crates/intention-provider-generic-chat/src/lib.rs:684` | SDK API errors are classified by an optional type field, not by HTTP status |
| P2-17 | P2 | Tooling, live proof, quality policy | `crates/intention-daemon/tests/real_api_e2e.rs:1060` | Live run asserts model wording (`READY`) outside the bounded attempt budget |
| P3-01 | P3 | Domain layer | `crates/intention-domain/src/reasoning_history.rs:17` | Provider-native dialect paths are hard-coded in the domain with no consumer |
| P3-02 | P3 | Domain layer | `crates/intention-domain/src/provider_catalog.rs:98` | Id bound counts bytes while documented as characters and disagrees with the wire DTO |
| P3-03 | P3 | Domain layer | `crates/intention-domain/src/provider_catalog.rs:146` | `validate_endpoint` accepts an endpoint with an empty authority |
| P3-04 | P3 | Domain layer | `crates/intention-domain/src/lib.rs:598` | Command DTO documents and implements legacy omitted-field tolerance |
| P3-05 | P3 | Domain layer | `crates/intention-domain/src/provider_catalog.rs:685` | Tombstone documentation still claims a permanent identity record |
| P3-06 | P3 | Domain layer | `crates/intention-domain/src/provider_catalog.rs:822` | Profile and kind tombstone codecs are byte-for-byte duplicates |
| P3-07 | P3 | Protocol and client | `crates/intention-protocol/src/contract_families.rs:1787` | Control-plane DTOs bypass declared invariants at the wire decode boundary |
| P3-08 | P3 | Protocol and client | `crates/intention-protocol/src/contract_families.rs:3589` | `ConfigurationProjectionDto.reload_status` is an untyped status string |
| P3-09 | P3 | State and configuration | `crates/intention-storage-sqlite/src/control_plane.rs:3276` | Removal accept, reject, and expire have no fault-injection coverage |
| P3-10 | P3 | State and configuration | `crates/intention-storage-sqlite/src/control_plane.rs:3536` | `ensure_catalog_state_seed` duplicates the schema DDL seed row |
| P3-11 | P3 | State and configuration | `crates/intention-storage-sqlite/src/control_plane.rs:2639` | `persist_resolved_run_provider_selection` has no production caller |
| P3-12 | P3 | State and configuration | `crates/intention-config/src/control_plane.rs:741` | `restore_credential_document` returns the raw document when re-serialization fails |
| P3-13 | P3 | State and configuration | `crates/intention-storage-sqlite/src/lib.rs:1141` | `load_run_config_snapshot` maps a decode failure to `run_configuration_unavailable` |
| P3-14 | P3 | State and configuration | `crates/intention-storage/src/lib.rs:990` | Storage crate header claims no raw strings cross the boundary, contradicting the `*_json` fields |
| P3-15 | P3 | State and configuration | `crates/intention-storage-sqlite/tests/m5_control_plane_repos.rs:1321` | Removal-evidence escaping is never round-tripped through the reader |
| P3-16 | P3 | Application services | `crates/intention-application/src/provider_catalog.rs:1000` | Tombstoned admission branch and in-memory tombstone set are unreachable |
| P3-17 | P3 | Application services | `crates/intention-application/src/session_selection.rs:925` | Removal command's `source_recheck` flag is ignored and replaced by a constant |
| P3-18 | P3 | Application services | `crates/intention-application/src/session_selection.rs:425` | Session default change emits no `SessionProviderProfileChanged` event |
| P3-19 | P3 | Application services | `docs/intention-relay/architecture/12-quality-gates-and-makefile.md:276` | New `m5_session_selection` test target is missing from the normative enumerations |
| P3-20 | P3 | Application services | `crates/intention-application/src/provider_control_plane.rs:499` | Health evidence fabricates a `health-profile-<hex>` revision identity |
| P3-21 | P3 | Application services | `crates/intention-application/src/provider_catalog.rs:1586` | Removal evidence is hand-rolled JSON crossing the storage boundary |
| P3-22 | P3 | Application services | `crates/intention-application/src/provider_catalog.rs:1463` | Third selection-digest implementation with no production consumer |
| P3-23 | P3 | Application services | `crates/intention-application/tests/m5_control_plane_runtime.rs:473` | Non-authority tests assert `Debug` substrings, not behaviour |
| P3-24 | P3 | Application services | `crates/intention-application/src/provider_catalog.rs:271` | Startup documentation claims all failures degrade, but storage errors propagate |
| P3-25 | P3 | Composition and daemon | `crates/intention-daemon/src/lib.rs:166` | Held-run lookup error is treated as "not held" and auto-schedules |
| P3-26 | P3 | Composition and daemon | `crates/intention-daemon/src/lib.rs:895` | Tool decoder re-lists wire names instead of using the typed `ToolId` or registry |
| P3-27 | P3 | Composition and daemon | `crates/intention/src/lib.rs:307` | Option seam selects the adapter builder by string kind in two places |
| P3-28 | P3 | Composition and daemon | `crates/intention/src/lib.rs:1185` | Typed-edit TOML rendering lives in the composition and interpolates values |
| P3-29 | P3 | Composition and daemon | `crates/intention/src/lib.rs:1443` | Public activation method has one in-crate caller and takes raw credential text |
| P3-30 | P3 | Model and providers | `crates/intention-provider-generic-chat/src/lib.rs:551` | Reasoning failure branch is unreachable and its test is vacuous |
| P3-31 | P3 | Model and providers | `crates/intention-provider-generic-chat/src/wire.rs:75` | Private wire reuses the closed `FinishReason`; unknown reasons abort the response |
| P3-32 | P3 | Model and providers | `crates/intention-runtime/src/lib.rs:1346` | Attachment validation can abort a run that the durable path accepted |
| P3-33 | P3 | Tooling, live proof, quality policy | `crates/intention-daemon/tests/real_api_e2e.rs:88` | Harness attempt budget exceeds the workflow step and job timeouts |
| P3-34 | P3 | Tooling, live proof, quality policy | `crates/intention-daemon/tests/real_api_e2e.rs:1277` | State-bytes credential scan is vacuous on macOS |
| P3-35 | P3 | Tooling, live proof, quality policy | `crates/intention-daemon/tests/real_api_e2e.rs:1351` | "No raw provider text" assertion is a tautology |
| P3-36 | P3 | Tooling, live proof, quality policy | `quality/run_coverage.py:133` | New coverage dedup rule is undocumented in policy and untested |
| P3-37 | P3 | Tooling, live proof, quality policy | `crates/intention-daemon/tests/real_api_e2e.rs:1199` | Two live-harness waits are not deadline-bounded |
| P3-38 | P3 | State and configuration / tests | `crates/intention/src/lib.rs:5522` | In-repo NOTE claims the session provider profile transaction rolls back; the code commits and the read-back assertion is missing (consolidator-verified) |

## Scope assessments

Each reviewer also returned a five-field qualitative assessment of its area. They are
reproduced here in full because they are the context in which the findings should be
read.

### Domain layer (crates/intention-domain, crates/intention-types)

- **Expediency:** The codecs, goldens and rejection suites are tight and
  version-current, but `reasoning_history.rs` ships a whole DTO/validator surface
  (`ReasoningDeltaDto`, `ReasoningSummaryDeltaDto`, `validate_reasoning_dialect` and
  `REASONING_DIALECT_VALUES`, `validate_reasoning_history_available`,
  `validate_reasoning_history_compatibility`, `validate_reasoning_output_bound`) that
  no production or test target outside its own unit module consumes.
- **DTO compliance:** Canonical records, tags, credential-shape roles and endpoint
  policy are single-sourced in `intention-domain` and reused by protocol and config,
  but the canonical digest functions are not: no production path calls
  `provider_selection_digest` / `provider_profile_revision_digest`, and three other
  ad-hoc digest shapes exist for the same selection meaning.
- **Generalizability:** The record codecs are descriptor and field-table driven and
  provider-neutral, with two exceptions: the closed dialect vocabulary hard-codes
  provider-native field paths inside the domain crate, and the profile and kind
  tombstone codecs are literal copies of each other differing only in the id validator
  and the error code.
- **Single live version:** Goldens are pinned to the current version only
  (`record_version = 1`, the v3 codec and golden are gone, `workspace_id` and
  `category` are mandatory) and repository-wide greps for `ensure_compatible_with`,
  the `0x020C` bridge, `SnapshotBindingSource` and the legacy error fixture return zero
  code references.
- **Test adequacy:** The rejection suites genuinely exercise fail-closed framing per
  family (missing, duplicate, descending, unknown fields, wrong wire type, truncation,
  trailing bytes, wrong version, invalid UTF-8, closed enums, over-limit counts,
  non-canonical u64, digest mismatch) with exact `CanonicalError` assertions, though a
  few "no marker in canonical bytes" tests only search fixed fixture bytes.

### Protocol and client surface (crates/intention-protocol, crates/intention-client)

- **Expediency:** The Slice 2 wire surface is mostly ADR 0037-backed and the removals
  are clean, but nine control-plane event DTOs and a reasoning, header, and parser DTO
  cluster have no producer or consumer, and one request field (reconcile
  `page_cursor`) is never read.
- **DTO compliance:** New DTOs are typed with explicit `validate()` and typed error
  codes and are validated at daemon admission, but decode does not enforce declared
  invariants (derived `Deserialize`) and several fields use bare strings
  (`schema_version`, `reload_status`) where the contract policy requires typed values
  and exact-version checks.
- **Generalizability:** Capability forcing is not data on the protocol surface: the
  new protocol gate helpers have no production caller and the daemon re-implements the
  gate as a hand-maintained per-variant allowlist plus a duplicated error string, so
  the next control-plane variant silently escapes the gate; the tool-advertisement
  area has no hard-coded tool names.
- **Single live version:** Same-major compatibility is genuinely gone
  (`ensure_compatible_with` deleted, 1.0 fixtures and legacy-shape decoders removed,
  transport requires exact 1.1 equality), but the new control-plane families carry an
  unchecked `schema_version` `String`, so a peer can send a non-current schema version
  and be served.
- **Test adequacy:** Protocol and client fixtures are strong on round-trips, bounds,
  closed-enum rejection and credential redaction, but the real client to real daemon
  control-plane path is untested (the live e2e only calls `health()`), which is exactly
  why the client capability-advertisement break is invisible.

### State and configuration (intention-storage, intention-storage-sqlite, intention-config)

- **Expediency:** The storage and config surface is large but each module maps to an
  ADR 0037 Slice 2 direction, and the migration and legacy machinery was genuinely
  removed rather than shadowed. Two leftovers remain: a DDL seed re-inserted after the
  schema batch, and a repository method with no production caller.
- **DTO compliance:** Typed DTOs with validated constructors cover the read paths, but
  several public storage DTOs carry opaque JSON strings (`safe_projection_json`,
  `selection_json`, `candidate_json`, `usage_json`), and the two JSON input fields are
  caller-supplied text persisted verbatim without admission validation.
- **Generalizability:** Catalog, queue, usage, removal and held-run repositories are
  generic over their DTOs with no per-fixture special case; the one-off items found
  are a redundant seed helper and an unused persist method, not special-cased callers.
- **Single live version:** Exactly one current schema is created in a single
  `execute_batch` on open (no `MIGRATIONS`, no `user_version` gate, no
  `TEST_SCHEMA_3_SQL`), matching ADR 0038 wave 4, and the config crate has one TOML
  shape with unversioned documents failing closed.
- **Test adequacy:** Fault-injection tests reopen the database and assert no partial
  rows for turn acceptance, terminal promotion, model facts, tool results, catalog
  acceptance (four stages), reload (two stages), provider selection, queue, held run,
  and usage. The removal lifecycle and the JSON-escape round trip have no equivalent
  coverage.

### Application services (crates/intention-application)

- **Expediency:** Most additions map to declared ADR 0037 surfaces, but a few pieces
  exist only to satisfy tests or future wiring: an inert in-memory tombstone set with
  an unreachable error branch, a third hand-rolled selection digest with no production
  consumer, and a client-supplied removal field that is silently replaced by a
  constant.
- **DTO compliance:** Boundaries are DTO-only and credential-free in the expected
  places, with one exception: the application hand-builds a JSON removal-evidence
  document written into a raw `String` storage field and hand-parsed by storage,
  rather than using a typed DTO or canonical codec.
- **Generalizability:** Catalog and selection code is provider-neutral and driven by
  declarations, but two spots bake in placeholders: health evidence invents a
  `health-profile-<hex>` revision identity instead of reading the catalog, and
  `by_profile` usage attribution depends on storage row order.
- **Single live version:** Verified: exactly one execution path per operation remains
  (the legacy M4 selection bridge and the selection-less send-user-turn path are gone;
  no feature flag or fallback branch), and the session-default read path fails closed
  when no profile applies.
- **Test adequacy:** Happy paths and several error paths are well covered, but the
  non-authority properties are asserted only by `Debug`-substring checks, and the
  post-acceptance registry-build failure (partial durable activation) has no test at
  all.

### Composition, daemon and adapters

(crates/intention, crates/intention-daemon excluding `tests/real_api_e2e.rs`,
crates/intention-transport, crates/intention-workspace, crates/intention-tui,
crates/intention-test-support)

- **Expediency:** Most of the added code is genuine port wiring with documented seams,
  but a few surfaces exceed wiring: the composition renders configuration TOML itself,
  keeps a public single-caller activation method that takes credential-bearing raw
  text, and repeats the provider-kind mapping three times. The startup activation is
  also invoked twice per process start (`open_with_selected_provider` and `run`).
- **DTO compliance:** Every boundary traced passes typed DTOs; the one raw-JSON
  boundary (provider tool arguments) is decoded into the typed `ToolInput` and rejected
  with a typed error, and the private credential never reaches a DTO, error, digest,
  `Debug` or durable row. The credential state has no `Debug`/`Display`/serde and
  `RawConfigInputDto` has no `Debug` or content accessor, so the redaction law holds on
  the paths read.
- **Generalizability:** Two seams are one-branch-per-case rather than registry or enum
  driven: the option seam selects the adapter builder by comparing the kind to the
  literal `"openrouter"`, and the daemon re-lists the six tool wire names instead of
  using `intention_tools::ToolId` (or `model_visible_descriptors()`). Adding a third
  provider or a newly model-visible tool therefore needs edits in several places, and
  the else branches silently route unknown kinds to the generic-chat builder.
- **Single live version:** No compatibility branch remains in the reviewed code: hello
  negotiation requires the exact current version, `ProtocolAcceptedDto::result()` is no
  longer optional, and the post-commit publisher, M4 selection bridge,
  `resolve_path_for_tool` and legacy dispatch stubs are removed with no fallback. The
  only version literals are the pre-existing `"1.1"` text pattern used workspace-wide
  plus the composition's `CONFIG_SCHEMA_VERSION = (1, 0)`, which mirrors a private
  `intention-config` constant.
- **Test adequacy:** Strong evidence for credential redaction and rotation, reload
  atomicity, held-run admission, queue promotion, pending-removal restart, and
  stale-socket reclaim. Gaps: nothing exercises the startup-catalog versus
  startup-config divergence, nothing exercises the daemon's degraded gate for
  `SendUserTurn` (only `SetSessionProviderProfile`), and the file-backed credential
  source is only reachable through hand-replicated fixtures rather than `open_platform`.

### Model and provider adapters

(crates/intention-model, crates/intention-provider-generic-chat,
crates/intention-provider-openrouter, crates/intention-runtime)

- **Expediency:** Mostly minimum-change and ADR-scoped: waves 6 and 7 removed the
  legacy tool-fragment paths and the no-port denial fallback, and the reasoning round
  trip adds one DTO plus one adapter translation. The exceptions are roughly 350 lines
  of new public model surface with no production consumer and one unreachable failure
  branch in the generic adapter.
- **DTO compliance:** Both adapters stay behind typed DTOs: no SDK type appears in a
  public signature (the generic BYOT structs live in the private `mod wire;`,
  `parse_parameters<T: FromStr>` names no native type, and `quality/architecture.toml`
  per-package dependency lists enforce the boundary). Raw JSON crosses only as the
  ADR 0039-sanctioned validated `parameters_json` text, and reasoning travels as a
  validated `AssistantReasoningDto`.
- **Generalizability:** Tool advertisement is registry-driven (application to
  `intention_tools::model_visible_descriptors()`, Active plus schema, registry order;
  `with_tools` forces the tool-call capability; both adapters translate whatever
  arrives and omit `tool_choice`), and no model-name routing, alias table, or fallback
  exists in these crates. The one gap is the new private wire type reusing the SDK's
  closed `FinishReason` enum, so an unlisted gateway reason aborts the response instead
  of mapping to `Unknown`.
- **Single live version:** No compatibility branch, version switch, or dual-shape
  handling remains in scope: `finish_legacy` / `merge_legacy` and the optional
  tool-executor denial path are gone, `ModelRunExecutionService` requires the tool
  executor, and the reasoning attachment has exactly one live path.
- **Test adequacy:** Adapter tests assert request shape (tools, reasoning echo
  including the empty presence, absence of `tool_choice`), stream normalization order,
  error retryability, and credential non-leakage; runtime tests assert per-round
  attachment order and the textless-presence case; `m6_reasoning_surface` is declared
  in `quality/architecture.toml`. Gaps: the generic reasoning-failure branch is only
  reached by a fabricated `state.fail(...)` call, and no test drives an unknown
  provider finish reason through the chunk decoder.

### Tooling, live proof and quality policy

(crates/intention-tools, crates/intention-daemon/tests, quality/, Makefile, deny.toml,
Cargo.lock, .github, THIRD_PARTY_NOTICES.md, .gitignore, tests/)

- **Expediency:** The tools work is surgical and ADR-driven (PR24-011/022/023 bounds,
  ADR 0038 wave 8, ADR 0039 schemas), and the live harness is one opt-in file plus one
  Makefile target and one manual workflow, matching ADR 0040 exactly. Two harness
  choices go slightly beyond minimum evidence: a model-wording assertion and an
  attempt budget that cannot fit the CI step it is wired into.
- **DTO compliance:** Strong: `BoundedText` and `ExecuteInput` now validate on
  `Deserialize` so JSON at the daemon tool boundary cannot bypass constructor bounds,
  in-process `ExecuteInput` construction revalidates explicitly, and the model-facing
  schemas travel as validated JSON-object text in `ModelToolDefinitionDto` per
  ADR 0039 rather than raw `serde_json::Value`.
- **Generalizability:** The registry-driven advertisement
  (`model_visible_descriptors`, registry order) and the call-id-matched fact helpers
  generalize across providers, tool orders and phrasings, and the invalid-credential
  test accepts a closed set of normalized codes for both adapters. The one exception is
  the `READY` reply assertion, which hard-codes model wording outside the retry budget.
- **Single live version:** ADR 0038 wave 8 is fully landed: `dispatch`, `invoke` and
  `invoke_with_context` are gone with every call site migrated to
  `dispatch_with_cancellation` / `invoke_enveloped*`, the `-1` exit-code sentinel is
  replaced by a typed status render, `resolve_path_for_tool` has no remaining code
  reference, and the orphan `tests/quality` tree is deleted with no dangling reference
  (only ADR 0038's own removal note names it).
- **Test adequacy:** The new `bounded_contracts` target plus the
  model-visible-descriptor tests give real behaviour coverage for the bounds and the
  advertisement, and the three new self-tests pin the opt-in channel
  (`workflow_dispatch` only, never a prerequisite of quick, check, verify, or ci).
  Gaps are harness-side: two waits are not deadline-bounded, the state-bytes credential
  scan is vacuous on macOS, and the new coverage dedup rule has no self-test or policy
  text.

## P1 findings

### P1-01. Canonical selection and profile digests are dead; production hashes four ad-hoc shapes

**Accepted decision:** D-01, option A (see Appendix G.1): one canonical identity digest owned
by the record, called by application and storage, with the ad-hoc digests deleted.

- **Priority:** P1
- **Area:** Domain layer
- **Category:** contract (single-source-of-meaning violation)
- **Location:** `crates/intention-domain/src/provider_selection.rs:647` (also `:675`, the shared `record()` helper)
- **Confidence:** high
- **Defect:** The domain crate owns the canonical identity digest
  (`provider_selection_digest`, `provider_profile_revision_digest`, built from the
  canonical field table), but no production path calls it. The durable selection
  identity is instead computed three other ways, so one logical selection has four
  different byte shapes and the canonical codec is not the single source of meaning
  that `architecture/02-dto-and-contract-policy.md` requires. Any future divergence
  between these shapes silently changes or collides admission identity without failing
  a test, and the digest input field table is additionally re-derived by hand at every
  call site instead of being read from the record.
- **Observed evidence:** A repository-wide grep for
  `provider_selection_digest` / `provider_profile_revision_digest` shows callers only in
  `crates/intention-domain/tests/m5_control_plane_canonical.rs:1224` and `:1285`.
  Production computes instead:
  (a) `crates/intention-application/src/provider_catalog.rs:1463-1484` builds
  `"ir-selection-v1|profile=...|source=..."` and SHA-256s it into
  `ProviderAdmissionDto.selection_digest`, which no caller reads: the two consumers of
  `registry_lookup` (`crates/intention/src/lib.rs:361` and `:405`) discard the returned
  DTO;
  (b) `crates/intention-storage-sqlite/src/control_plane.rs:1116-1117` hashes the
  selection JSON text into the persisted `selection_digest` column;
  (c) `crates/intention/src/lib.rs:1298-1306` emits `"ir-record:" + hex(selection.encode())`
  as `selection_json`.
  The canonical field table is re-assembled by hand in the tests at
  `m5_control_plane_canonical.rs:1127-1197` rather than from the record.
- **Consequence:** Admission identity is not derivable from one canonical function, so
  a change in any of the three ad-hoc computations (field order, separator, inclusion
  of provenance fields such as `selection_source`, which the domain documents as
  non-identity-bearing) changes persisted identity or creates collisions without any
  test failing. The canonical digest APIs remain dead public surface that can drift
  from the code paths that actually run.
- **Remediation:** Give the records their own identity digest (for example
  `ProviderSelectionV1::identity_digest()` built from the same field table as `encode`)
  and make the application, storage, and composition call it; delete the
  `"ir-selection-v1|..."` / `"ir-profile-v1|..."` string digests and the JSON-text
  hash. If the application digest is a deliberate second identity, record that decision
  in the ADR and in the reconciliation ledger and delete the unused domain digest APIs
  instead, so exactly one of the two exists.
- **Verification:** Add a test that builds one logical selection through the
  application path and persists it through storage, then asserts the persisted
  `selection_digest` equals the domain canonical digest and that the admission DTO
  digest (if kept) equals it too. Add a grep-style guard test (or a lint assertion in
  the test suite) that no `format!("ir-selection-v1...")`-shaped digest computation
  exists outside the domain. Before acting, read the PR24 ledger rows referenced by the
  domain reviewer to confirm whether a second admission digest was deliberately
  declared. Run `cargo nextest run -p intention-domain -p intention-application -p intention-storage-sqlite --tests`
  and `make quick`.

### P1-02. Client never advertises `provider_profiles_v1`, so the real daemon rejects all control-plane calls

- **Priority:** P1
- **Area:** Protocol and client surface
- **Category:** correctness (delivered feature is unusable end to end)
- **Location:** `crates/intention-client/src/lib.rs:52` (`REQUIRED_CAPABILITIES`), with the enforcing side at
  `crates/intention-daemon/src/lib.rs:1117-1120` and `:1140-1143`
- **Confidence:** high (re-verified directly by the consolidator on 2026-09-24)
- **Defect:** `IntentionClient` builds its protocol hello from `REQUIRED_CAPABILITIES`,
  which contains only `SessionSubscriptions`, `CorrelatedRequests`, and `DaemonHealth`.
  The daemon gates every Slice 2 command and query on the peer's hello capabilities, so
  with the real client every control-plane method added by this PR
  (`reload_configuration`, `submit_raw_toml_edit`, `apply_configuration_edit`,
  `rotate_credential`, `set_session_provider_profile`, `reconcile_unavailable_queue`,
  `admit_recovered_run`, catalog removal accept and reject, and every control-plane
  query) is rejected with `provider_profiles_capability_required` before any effect.
  The typed client surface delivered by this PR is therefore unreachable against a real
  daemon.
- **Observed evidence:** `crates/intention-client/src/lib.rs:52-56` declares the
  three-capability set; `:649-651` negotiates with exactly that hello. The daemon stores
  the peer's set (`crates/intention-daemon/src/lib.rs:1032`,
  `remote_capabilities = Arc::from(remote.capabilities().to_vec())`) and rejects any
  gated command or query whose peer lacks `ProviderProfilesV1`
  (`crates/intention-daemon/src/lib.rs:1117-1120`, `:1140-1143`, error constructor at
  `:1148-1153`). The client's own test suite acknowledges the baseline:
  `crates/intention-client/tests/control_plane_client.rs:568-570` asserts only that the
  protocol helper `require_provider_profiles` fails closed, and the fixture daemons in
  that suite never inspect the client's capability set. The only test that runs against
  a real daemon (`crates/intention-daemon/tests/real_api_e2e.rs`) calls `client.health()`
  only, which is never gated.
- **Consolidator re-verification:** Direct reads on 2026-09-24 confirmed (a)
  `REQUIRED_CAPABILITIES` in `crates/intention-client/src/lib.rs:52-56` holds exactly
  `SessionSubscriptions`, `CorrelatedRequests`, `DaemonHealth`; (b) the daemon's
  capability gate for both commands and queries is exactly as reported; (c)
  `crates/intention-protocol/src/negotiation.rs:129-131` defines
  `require_provider_profiles`, whose only references are its own module tests and one
  client test. The defect stands.
- **Consequence:** Every consumer of the new control-plane client API (TUI, adapters,
  M6 bridge work) receives a typed rejection wall instead of functionality; the
  capability gate silently turns the delivered client surface into dead API. No
  hermetic test catches it because the fixture daemons do not enforce the gate.
- **Remediation:** Advertise the post-M5 capabilities the client understands in its
  hello (at minimum `ProviderProfilesV1`, and `RunStreamSubscriptions` already exists as
  a separate set) and require it explicitly in `connect()` before returning a client
  whose control-plane methods are usable, for example by extending a shared capability
  set used both for hello construction and for a per-method `require_capability` check.
  Then add the missing real-daemon test.
- **Verification:** Add an integration test that spawns the real daemon (via the
  existing test support) and, through `IntentionClient`, runs one gated query (for
  example `provider_catalog_status`) and one gated command (for example
  `set_session_provider_profile`), asserting `Accepted` results, plus a negative case
  that a client hello without `ProviderProfilesV1` is rejected with
  `provider_profiles_capability_required`. Run
  `cargo nextest run -p intention-client -p intention-daemon --tests` and `make quick`.

### P1-03. Restart with an edited catalog field leaves the catalog and the configuration snapshot divergent

**Accepted decision:** D-02, option A (see Appendix G.2): at startup the catalog is
re-derived from the startup document through the normal prepare and accept path.

- **Priority:** P1
- **Area:** Composition and daemon
- **Category:** correctness (durable authorities disagree)
- **Location:** `crates/intention/src/lib.rs:1450` (`activate_startup_catalog` early return)
- **Confidence:** high
- **Defect:** `activate_startup_catalog` returns `Ok(())` immediately whenever a catalog
  is already active, so a kind, model, or endpoint change made in the startup TOML is
  never re-derived into the catalog. `open_platform` nevertheless builds the executing
  driver and the active configuration snapshot from that same file. A run is then
  admitted against the stale catalog profile (and its immutable selection records the
  old kind and model) while it is executed by the new file's driver and model, so the
  two durable authorities disagree silently. The error code
  `catalog_change_requires_restart` advertises a recovery (restart) that does not
  actually apply the change.
- **Observed evidence:** `crates/intention/src/lib.rs:1449-1451`:
  `if state.active_catalog_revision_id.is_some() { return Ok(()); }`.
  `crates/intention/src/lib.rs:1413-1425` builds `selected_provider` from
  `load_provider_configuration(source)` and then calls `activate_startup_catalog`.
  `crates/intention/src/lib.rs:1824-1831` compares only the selected provider kind
  against `config_snapshot`, never against the durable catalog.
  `send_user_turn` persists `self.active_config_snapshot()`
  (`crates/intention/src/lib.rs:2613`) while the admission port resolves the selection
  from the durable catalog (`crates/intention/src/lib.rs:341-378`), and
  `schedule_from_context` takes the request model from that safe config
  (`crates/intention-application/src/lib.rs:1476-1478`).
- **Consequence:** After a legitimate restart with an edited catalog field, the
  model that executes a run differs from the model recorded in the run's immutable
  provider selection; the persisted catalog revision and the persisted config snapshot
  describe different providers. Operators following the advertised recovery path
  (`catalog_change_requires_restart`) cannot fix the state by restarting, and no test
  covers the divergence (the composition test suite exercises startup activation only
  for a fresh store).
- **Remediation:** At open time, compare the startup-derived declaration (kind, model,
  endpoint) with the active catalog's active profile declaration. On a difference,
  either prepare a catalog candidate so the catalog is re-derived (and let the normal
  acceptance path record the new revision), or fail the open path closed with a typed
  catalog error. Never construct a `SelectedProvider` whose kind differs from the active
  catalog's kind, and assert that invariant in a test.
- **Verification:** Add a composition or facade test that (1) opens a store and
  activates a catalog, (2) edits the model or kind in the startup document, (3) re-opens,
  and (4) asserts either a typed catalog rejection or a re-derived active catalog whose
  profile declaration matches the file. Add an invariant assertion that the executing
  driver kind equals the active catalog kind for every admitted run. Run `make quick` and
  the daemon facade end-to-end target.

## P2 findings

### P2-01. Reasoning DTO and validator surface has no consumer and duplicates a bound rule

- **Priority:** P2
- **Area:** Domain layer
- **Category:** dead-code (unconsumed public surface) plus duplicated rule
- **Location:** `crates/intention-domain/src/reasoning_history.rs:39` (`ReasoningDeltaDto`), with the duplicate rule at
  `crates/intention-domain/src/reasoning_history.rs:308` versus `crates/intention-domain/src/model_facts.rs:90`
- **Confidence:** high
- **Defect:** `ReasoningDeltaDto`, `ReasoningSummaryDeltaDto`, `validate_reasoning_dialect`,
  `validate_reasoning_history_available`, `validate_reasoning_history_compatibility`, and
  `validate_reasoning_output_bound` exist only in this module's unit tests and in the
  `lib.rs` re-export list; no production or test target outside the module consumes them.
  They are therefore untested contract surface that can drift from the code paths that
  actually run. In addition, `validate_reasoning_output_bound` is a second implementation
  of the rule enforced by `model_facts`, with a different error type, and only the
  `model_facts` copy is used by the durable append authority.
- **Observed evidence:** `ReasoningDeltaDto` at `reasoning_history.rs:39`,
  `ReasoningSummaryDeltaDto` at `:59`, `validate_reasoning_dialect` at `:257`,
  `validate_reasoning_history_available` at `:271`,
  `validate_reasoning_history_compatibility` at `:290` appear nowhere outside
  `reasoning_history.rs` and the re-export list. The storage append path
  (`crates/intention-storage-sqlite/src/lib.rs:1039`) and the runtime use the
  `model_facts` alias `validate_reasoning_fact_output_bound`, whose body
  (`model_facts.rs:90`) is identical to `reasoning_history.rs:308` except for returning
  a `DtoResult` error instead of a `CanonicalError`.
- **Consequence:** Two implementations of one bound can diverge (one accepts what the
  other rejects) while only one is executed; the unused DTO family looks like a
  supported contract to future consumers and must be maintained or removed later at
  higher cost. This is exactly the class of surface the no-backward-compatibility and
  minimum-change policies ask to delete.
- **Remediation:** Delete the unconsumed DTOs and validators, or move the reasoning DTO
  family to `intention-model`, which `architecture/22` and ADR 0037 assign the
  provider-neutral reasoning surface to. Keep exactly one reasoning-output-bound
  function, shared by the durable append authority, and re-point any remaining caller
  to it.
- **Verification:** After the change, a repository-wide grep for each removed identifier
  must return no hits outside history or ADR text; add a unit test that the single
  remaining bound function is the one the storage append path calls (or a compile-time
  single-owner arrangement). Run `cargo nextest run -p intention-domain -p intention-storage-sqlite -p intention-runtime --tests`
  and `make quick`.

### P2-02. `selection_canonicalization_version` is not validated as a closed value

- **Priority:** P2
- **Area:** Domain layer
- **Category:** contract (fail-open validation)
- **Location:** `crates/intention-domain/src/provider_selection.rs:421` (validate), constant at `:24`
- **Confidence:** high
- **Defect:** `PROVIDER_SELECTION_CANONICALIZATION_VERSION` documents that there is a
  single canonicalization version so that identical selections share bytes and digests,
  yet `validate` only checks that the field is a non-empty string of at most 256
  characters. A producer that writes `"2"` (or any other string) validates successfully
  and silently emits different canonical bytes and a different identity digest for the
  same logical selection, defeating the invariant the constant documents. The validation
  cannot fail closed for a non-current version, unlike every other versioned family.
- **Observed evidence:** `provider_selection.rs:24` declares
  `PROVIDER_SELECTION_CANONICALIZATION_VERSION = "1"`; `validate` at `:421-450` calls
  `validate_provider_string(&self.selection_canonicalization_version, 256)` with no
  equality check. The sibling closed value in the same area is enforced:
  `ProviderProfileRevisionV1::validate` compares `capability_taxonomy_revision` against
  `MODEL_CAPABILITY_TAXONOMY_V1` and returns `ProviderProfileRevisionInvalid`.
- **Consequence:** A non-current canonicalization version is accepted into the durable
  record and digest, so meaning and identity can diverge without any error; the digest
  becomes dependent on an unvalidated string.
- **Remediation:** Compare the field against
  `PROVIDER_SELECTION_CANONICALIZATION_VERSION` inside `validate` and return the typed
  `CanonicalError::ProviderProfileRevisionInvalid` (or a dedicated error) for any other
  value, mirroring the capability-taxonomy check. If a second canonicalization version is
  ever required, it needs its own record version and ADR entry, not a free-form string.
- **Verification:** Add a rejection fixture that sets `"2"` and asserts the typed error,
  and a positive fixture that `"1"` validates and produces the golden digest. Run
  `cargo nextest run -p intention-domain --tests` and `make quick`.

### P2-03. Control-plane DTOs carry an unchecked `schema_version: String`

- **Priority:** P2
- **Area:** Protocol and client surface
- **Category:** contract (same-version rule unenforced on the new families)
- **Location:** `crates/intention-protocol/src/contract_families.rs:1646` and eleven sibling fields
  (`:1788`, `:1857`, `:1955`, `:2095`, `:2167`, `:2531`, `:3327`, `:3355`, `:3389`, `:3553`, `:3583`)
- **Confidence:** high
- **Defect:** Eleven new control-plane command, query, and projection DTOs declare
  `schema_version: String` and validate it only as bounded text. Nothing compares it to
  `CURRENT_DTO_SCHEMA_VERSION`, so a peer can send `"schema_version": "9.9"` or
  `"banana"` in a control-plane command or query and the daemon serves it. The
  repository's stated rule that a schema version other than the current one always fails
  safely is therefore unenforced for the new families, unlike every other protocol DTO,
  which carries `SchemaVersionDto` and is compared exactly.
- **Observed evidence:** Field declarations at the lines listed above; validation at
  `:1660` (`valid_text(&self.schema_version, 64, ...)` and credential checks); no
  comparison with `CURRENT_DTO_SCHEMA_VERSION` anywhere in `contract_families.rs`. The
  daemon admission path only calls `command.validate()` / `query.validate()`
  (`crates/intention/src/lib.rs:2048-2166` for queries, `:2604-2667` for commands). The
  client-side counterpart defect for the run-stream path is `P2-08`.
- **Consequence:** A non-current or malformed schema version is accepted for the
  control-plane surface, so mixed-version peers are served instead of rejected, and the
  exact-version negotiation guarantee is silently weaker on the newest surface. Any
  future field-shape change becomes a silent misinterpretation instead of a typed
  rejection.
- **Remediation:** Replace the string with the typed `SchemaVersionDto` for these fields,
  or keep the string but add an exact-equality check against
  `CURRENT_DTO_SCHEMA_VERSION` in each `validate()`, returning a typed error such as
  `incompatible_protocol_version` / `schema_version_mismatch`. Update the ADR 0037 field
  table so the declared shape matches the implementation.
- **Verification:** Add protocol rejection fixtures for `"9.9"` and `"banana"` on both a
  command and a query, asserting the typed error; add a positive fixture for the current
  version; and add a daemon-admission test that a non-current schema version is rejected
  before any effect. Run `cargo nextest run -p intention-protocol -p intention-daemon --tests`
  and `make quick`.

### P2-04. `ReconcileUnavailableQueueCommandDto.page_cursor` is never read by any producer

**Accepted decision:** D-11, option (b) (see Appendix G.11.1): the request field and its
credential-shaped validation are deleted, and the durable reconciliation marker stays the single
paging authority.

**Remediation home:** Package C → D-11 (b). Implemented together with `P3-17`, which touches the
same removal-command surface.

- **Priority:** P2
- **Area:** Protocol and client surface
- **Category:** correctness / write-only wire surface
- **Location:** `crates/intention-protocol/src/contract_families.rs:2392` (validation at `:2406` and `:2411`)
- **Confidence:** high
- **Defect:** The command accepts and validates an opaque page cursor, and the acceptance
  returns a page cursor, implying cursor-based paging. No production code reads the
  request cursor: the application reconciles with the durable marker and passes only
  `now`, `operation_id`, and `max` to storage. A caller that feeds the returned cursor
  back (the documented paging loop) silently gets marker-driven behaviour with no error
  and no way to detect that its cursor was ignored.
- **Observed evidence:** Field at `contract_families.rs:2392`, validation only at
  `:2406`/`:2411`. `crates/intention-application/src/session_selection.rs:586-605`
  constructs `ReconcileUnavailableQueueInputDto { now, operation_id, max }` from the
  command's `operation_id` and `session_id` and never touches `command.page_cursor`; the
  response cursor is derived from
  `load_queue_reconciliation_marker(...).next_page_cursor`. A repository-wide grep shows
  no reader of the request field. All tests pass `page_cursor: None`
  (`crates/intention-client/tests/session_selection_client.rs:465`,
  `crates/intention-application/tests/m5_session_selection.rs:2517`).
- **Consequence:** The paging contract is a lie on the wire: callers implementing the
  documented loop cannot advance a cursor deterministically, and a stale or foreign
  cursor is accepted silently. It is also an extra field that every implementation must
  keep validating.
- **Remediation:** Either honour the cursor (pass it into
  `ReconcileUnavailableQueueInputDto` and reject a stale or unknown cursor with a typed
  error) or remove the field and its cursor-shaped validation so the command has exactly
  one execution path. Add a test that feeds a returned cursor back in.
- **Verification:** Add an application or facade test that reconciles twice using the
  cursor returned by the first call and asserts either correct continuation or a typed
  rejection; after removal, assert the wire shape no longer carries the field. Run
  `cargo nextest run -p intention-application -p intention-protocol -p intention-client --tests`
  and `make quick`.

### P2-05. Capability gate helpers have no production caller; the daemon re-implements the gate per variant

**Accepted decision:** D-03, option A (see Appendix G.3): the classification becomes data on
the protocol surface with an exhaustive match, and the daemon calls it.

- **Priority:** P2
- **Area:** Protocol and client surface
- **Category:** generalizability (fail-open classification maintained by hand)
- **Location:** `crates/intention-protocol/src/negotiation.rs:129` (`require_provider_profiles`), with the hand-maintained classification at
  `crates/intention-daemon/src/lib.rs:1173-1204`
- **Confidence:** high
- **Defect:** `require_provider_profiles` (new in this PR, documented as gating every
  control-plane command and query), together with `require_capability`,
  `require_gateway_tool_loop`, and `intersect_capabilities`, is referenced only from its
  own module tests. The real gate is a hand-maintained `matches!` allowlist over
  `ProtocolCommandDto` and `ProtocolQueryDto` variants in the daemon, with no
  exhaustiveness requirement and a duplicated `provider_profiles_capability_required`
  string literal. Adding the next control-plane variant therefore silently bypasses the
  capability gate (fail-open) instead of forcing a decision or failing to compile.
- **Observed evidence:** A repository-wide grep shows
  `require_provider_profiles` / `require_capability` / `require_gateway_tool_loop` /
  `intersect_capabilities` only in `crates/intention-protocol/src/negotiation.rs`
  (definition plus tests) and in `crates/intention-client/tests/control_plane_client.rs:565-575`.
  `crates/intention-daemon/src/lib.rs:1173-1204` classifies variants by hand in
  `command_requires_provider_profiles` / `query_requires_provider_profiles`;
  `:1151-1158` re-declares the error code string; capabilities are enumerated
  separately in `ProtocolCapabilityDto`, `POST_M5_CAPABILITIES`, and the daemon lists
  with no compile-time link between them.
- **Consequence:** The security-relevant gate on the control-plane surface is only as
  strong as a reviewer's memory: a new variant added without updating the hand list is
  served to peers that never negotiated the capability. It also duplicates the error
  code in two crates.
- **Remediation:** Make the classification data on the protocol surface (a
  `requires_provider_profiles()` / `is_control_plane()` method or a table declared next
  to the variants) and have the daemon call
  `negotiation::require_provider_profiles` or the classifier, so a new variant forces a
  decision and the error code has one owner. A compile-time exhaustive match (no wildcard
  arm) over the command and query enums is the natural mechanism.
- **Verification:** Add a test that enumerates every `ProtocolCommandDto` and
  `ProtocolQueryDto` variant and asserts each is classified by the protocol classifier;
  with an exhaustive `match` the test fails to compile when a variant is added without a
  decision. Keep the existing daemon gate tests (`crates/intention-daemon/src/lib.rs:2462-2495`)
  as behavioural coverage and add the real-client case from `P1-02`. Run
  `cargo nextest run -p intention-protocol -p intention-daemon --tests` and `make quick`.

### P2-06. Protocol duplicates the intention-model reasoning, header, and parser families with no consumer

**Accepted decision:** D-16, option (b) (see Appendix G.11.6): audit the duplicated protocol
families against the roadmap's M6 to M9 plan, declare the group a named slice will consume with an
evidence anchor, and delete the rest.

**Remediation home:** Package B → D-16 (b). The audit is shared with `P2-07` and `P2-15`, and the
`P2-07` pass must follow D-03.

- **Priority:** P2
- **Area:** Protocol and client surface
- **Category:** expediency (duplicated owner, speculative wire surface)
- **Location:** `crates/intention-protocol/src/contract_families.rs:3910` (the duplicate cluster spans `:3780-3960`)
- **Confidence:** high
- **Defect:** The same closed contract surface is defined twice in this PR.
  `intention-model` owns the provider-neutral reasoning surface
  (`ReasoningEffortLevel`, `ResponsesReasoningMode`, `CredentialTransportMode`,
  `AuthenticationHeaderPolicyV1`, `ProviderNativePreservationControlsV1`,
  `ServerSideParserConfigV1`) and the provider adapters consume it, while
  `intention-protocol` adds parallel copies (`ReasoningEffortLevel`,
  `ResponsesReasoningMode`, `ArbitraryHeaderPolicyDto`, `ProviderPreservationControlsDto`,
  `ServerSideParserConfigDto`, `ProviderReasoningCatalogProjectionDto`). The protocol
  copies are unreachable: no command or query variant, no client method, and no other
  crate references them, so they are speculative surface that can drift from the
  model-owned enums.
- **Observed evidence:** `contract_families.rs:3780-3960` defines the new types, which
  are referenced only by their own unit tests. A repository-wide grep shows no consumer
  of `ArbitraryHeaderPolicyDto` / `ProviderPreservationControlsDto` /
  `ServerSideParserConfigDto` / `ProviderReasoningCatalogProjectionDto` outside
  `intention-protocol`. The model equivalents at `crates/intention-model/src/lib.rs:941`,
  `:961`, `:983`, `:1087`, `:1195` are used by
  `crates/intention-provider-generic-chat/src/lib.rs:29-30` and tested by
  `crates/intention-model/tests/m6_reasoning_surface.rs`. The ADR 0037 ownership table
  assigns the reasoning surface to `intention-model` and anchors its evidence at
  `m6_reasoning_surface.rs`.
- **Consequence:** Two definitions of one meaning can diverge silently (a value accepted
  by the model enum but rejected or renumbered by the protocol copy), and the ownership
  table in ADR 0037 no longer matches the code. Reviewers and future consumers cannot tell
  which type is authoritative.
- **Remediation:** Delete the duplicate protocol types (or replace them with re-exports
  or typed projections of the `intention-model` types) and, if a reasoning-catalog wire
  surface is genuinely required, add the command or query variant plus the client method
  that consumes it in the same change, with an evidence anchor.
- **Verification:** After the change, the protocol crate must have exactly one definition
  per concept (grep guard for the removed names) and the ADR 0037 ownership table must
  list the surviving owner. Add a boundary test asserting the protocol surface exposes
  the model types rather than copies. Run
  `cargo nextest run -p intention-protocol -p intention-model --tests` and `make quick`.

### P2-07. Nine control-plane event DTOs have no producer and no consumer

**Accepted decision:** D-16, option (b) (see Appendix G.11.6): each event DTO is audited against
the roadmap's M6 to M9 plan; the ones a named slice will emit are declared with an evidence anchor,
the rest are deleted until the durable append path exists.

**Remediation home:** Package B → D-16 (b), audited after D-03.

- **Priority:** P2
- **Area:** Protocol and client surface
- **Category:** dead-code (unproduced wire surface) plus inaccurate module documentation
- **Location:** `crates/intention-protocol/src/contract_families.rs:2644` (the family spans the catalog and configuration event DTOs)
- **Confidence:** high
- **Defect:** The catalog and configuration event DTO family
  (`ProviderCatalogCandidatePrepared`, `ProviderCatalogCandidateRemovalPending`,
  `ProviderCatalogCandidateRejected`, `ProviderCatalogCandidateExpired`,
  `ProviderCatalogActivationRecoveryRequired`, `ProviderCatalogRecoveryCompleted`,
  `SessionProviderProfileChanged`, `ConfigurationReloaded`,
  `ConfigurationReloadRejected`) exists only in `contract_families.rs` plus its unit
  tests. No service constructs them, no domain event carries them, and no storage or
  stream path emits them, so they are public wire surface with no execution path. The
  application module documentation additionally claims that services construct the
  protocol event DTOs, which no service does.
- **Observed evidence:** A repository-wide grep for those nine identifiers matches only
  `crates/intention-protocol/src/contract_families.rs`.
  `crates/intention-application/src/session_selection.rs:1-14` states that "the services
  construct the protocol event DTOs where applicable and the durable append is a later
  storage zone", while no service references any of them; `intention-domain` has no
  matching event variants.
- **Consequence:** Subscribers and resync consumers have no event to observe, so a client
  built against these DTOs would silently never receive anything; the surface must be
  maintained (validation, docs, coverage) for behaviour that does not exist; and the
  module documentation asserts a contract the code does not honour. `P3-18` records the
  concrete instance for `SessionProviderProfileChanged`.
- **Remediation:** Remove the event DTO family until the durable append path exists (and
  add it back together with the storage and stream producer in one change), or implement
  the append and emit the events. Correct the application module documentation either
  way, so it describes what the services actually do.
- **Verification:** If the family is removed, a grep guard must return no hits outside
  ADR and register text, and the protocol test suite must not reference the names. If the
  events are implemented, add an end-to-end test that performs the described operation and
  asserts the event is both emitted to a subscriber and appended durably (facade plus
  storage read). Run `cargo nextest run -p intention-protocol -p intention-application --tests`
  and `make quick`.

### P2-08. Run-stream client accepts the caller's schema version instead of the current one

- **Priority:** P2
- **Area:** Protocol and client surface
- **Category:** contract (exact-version rule not applied on the stream path)
- **Location:** `crates/intention-client/src/lib.rs:769` (`RunStreamClient::subscribe`), with `:786` and `:839-845`
- **Confidence:** medium
- **Defect:** `RunStreamClient::subscribe` and `request_replay` compare the response DTO
  schema version against `subscription.schema_version()` or `self.schema_version`, both
  caller-supplied and re-used for later replays, instead of against
  `CURRENT_DTO_SCHEMA_VERSION`. Because neither the transport nor the daemon validates the
  incoming request schema version, an adapter can drive the whole run-stream path at a
  non-current schema version, while the synchronous path was hardened in this same PR to
  require exact equality with `SCHEMA_VERSION`. The comment claiming both paths apply the
  same equality is therefore inaccurate.
- **Observed evidence:** `crates/intention-client/src/lib.rs:766-772`
  (`response.message().schema_version() == subscription.schema_version()`), `:786` (the
  reducer stores the caller's version), `:839-845`. Contrast `:680-687`, which requires
  `!= SCHEMA_VERSION` for the synchronous path. No `schema_version` check exists in
  `crates/intention-transport/src/lib.rs` or `crates/intention-daemon/src/lib.rs`, and
  `crates/intention-client/tests/run_stream_contract.rs` contains no schema-version case.
- **Consequence:** The stream path silently accepts a self-consistent but non-current
  schema version, so a caller that passes an older or fabricated version sees data that
  the protocol rule says must be rejected. The mixed-version guarantee is enforced on one
  path and not the other.
- **Remediation:** Reject any subscription whose `schema_version` differs from
  `CURRENT_DTO_SCHEMA_VERSION` before sending, and compare responses against
  `SCHEMA_VERSION` on both stream paths; correct the misleading comment. Consider
  validating the incoming request schema version in the daemon as well, so the guarantee
  does not depend on client behaviour (this also covers `P2-03`).
- **Verification:** Add a fixture in `crates/intention-client/tests/run_stream_contract.rs`
  that subscribes with a mismatched schema version and asserts the typed rejection, plus
  a positive case at the current version. If daemon-side validation is added, add the
  matching rejection test there. Run
  `cargo nextest run -p intention-client -p intention-daemon --tests` and `make quick`.

### P2-09. `UNIQUE(workspace_root)` violation maps to `storage_unavailable`, not a typed conflict

- **Priority:** P2
- **Area:** State and configuration
- **Category:** contract (constraint errors collapse into an internal, retryable-looking code)
- **Location:** `crates/intention-storage-sqlite/src/lib.rs:669` (insert), DDL at `:44`, conflict mapping at `:678`; second instance on the turn path at `:805` and `:865`
- **Confidence:** high
- **Defect:** `workspace_roots` has `PRIMARY KEY(workspace_id)` plus
  `UNIQUE(workspace_root)`, and the insert suppresses only the `workspace_id` conflict
  (`ON CONFLICT(workspace_id) DO NOTHING`). A second `WorkspaceId` bound to an
  already-used root therefore raises a raw SQLite constraint error, which `storage_error`
  collapses into `storage_unavailable` (`Unavailable` / `Manual`). The caller sees an
  internal, retryable-looking failure instead of the typed conflict that the sibling
  reverse direction already returns (`workspace_root_conflict`), contradicting
  `architecture/02` ("persistence validates storage constraints and maps failures to
  `ErrorDto`"). The same class is reachable on the turn path: `turns.proposed_run_id` and
  `runs.run_id` are globally `UNIQUE` / primary keys while the idempotency pre-check is
  scoped to `(session_id, turn_id)`, so a caller-supplied `TurnId` reused in another
  session yields `storage_unavailable` from the insert at `:805` / `:865`.
- **Observed evidence:** `lib.rs:44` (`workspace_root TEXT NOT NULL UNIQUE`), `lib.rs:669`
  (insert with the single-conflict clause), `lib.rs:678` (read-back that only detects the
  same-id, different-root case and returns the typed `workspace_root_conflict`);
  `storage_error` ignores the SQLite error kind and returns `unavailable()`. The passing
  test `workspace_identity_cannot_bind_conflicting_roots_and_unknown_tails_fail_typed`
  (`crates/intention-storage-sqlite/tests/sqlite_contracts.rs:716-747`) exercises only the
  same-`WorkspaceId`, different-root path, so the different-`WorkspaceId`, same-root path
  is untested.
- **Historical context:** This is the same defect class that produced the
  `storage_unavailable` failure during the live-provider harness work in this session
  (session creation over an already-bound workspace root), which was worked around in the
  test harness by sharing the project and workspace identity. The product-level mapping
  defect described here remains.
- **Consequence:** A legitimate caller mistake or a second session over the same root
  produces an internal error code that looks transient, so callers may retry forever, and
  operators cannot distinguish a constraint violation from real unavailable storage. It
  also hides genuine identity conflicts from the reconciliation surface.
- **Remediation:** Pre-check the root binding inside the transaction and return the typed
  `workspace_root_conflict` for both directions, or map `SQLITE_CONSTRAINT_UNIQUE` on
  `workspace_roots` to that typed conflict in `storage_error`. Apply the same treatment to
  the `turns` / `runs` identity inserts, returning a typed conflict such as
  `turn_identity_conflict` instead of `storage_unavailable`.
- **Verification:** Add a storage contract fixture that creates two sessions with distinct
  `WorkspaceId` values over one root and asserts the typed conflict (and the absence of
  `storage_unavailable`); add a second fixture that reuses a `TurnId` across sessions and
  asserts the typed turn conflict. Run
  `cargo nextest run -p intention-storage-sqlite --tests` and `make quick`.

### P2-10. Public storage DTOs carry opaque untyped JSON strings

**Accepted decision:** D-07, option A, full replacement (see Appendix G.7): every opaque JSON
string field is replaced by the typed record it encodes; nothing of the string surface
remains. Shared with `P3-21`.

- **Priority:** P2
- **Area:** State and configuration
- **Category:** dto (boundary shape violates the DTO-first policy)
- **Location:** `crates/intention-storage/src/lib.rs:1198` (`selection_json`), with `:1211`, `:1277`, `:1325`, `:1338`, and `:1032`
- **Confidence:** medium
- **Defect:** Several public storage DTOs expose free-form `String` fields that carry
  JSON crossing the boundary: `UnavailableRunQueueEntryDto.selection_json` (`:1198`),
  `EnqueueUnavailableRunInputDto.selection_json` (`:1211`),
  `ProviderUsageEventInputDto.usage_json` (`:1277`),
  `ProviderCatalogRemovalCandidateDto.candidate_json` (`:1325`),
  `CreateProviderCatalogRemovalCandidateInputDto.candidate_json` (`:1338`), and
  `ProviderCatalogProfileEntryDto.safe_projection_json` (`:1032`, documented as "the
  opaque safe projection JSON produced by the backend"). `architecture/02` requires typed
  DTOs at every boundary, and the project concept text states that category collections
  cannot be maps, raw JSON, or opaque vendor blobs and that unproduced categories must be
  an empty typed collection, never an opaque placeholder or untyped map. The backend does
  build these strings from typed records, but the contract exposes them as text, so any
  consumer must re-parse untyped JSON to read a selection, and the writer path accepts
  arbitrary caller text for the two input fields.
- **Observed evidence:** The field declarations and doc comments at the lines listed
  above. The storage crate header still claims that no raw JSON, maps, or strings cross
  the boundary, which `P3-14` records as a documentation contradiction.
- **Consequence:** Two crates plus tests must agree by convention on the JSON shape with
  no type safety; a hand-rolled producer (see `P3-21`) or a missing escape (see `P2-11`)
  turns into a runtime decode mismatch rather than a compile error; and the boundary
  policy is unenforced exactly where the newest record families were added.
- **Remediation:** Replace these fields with the typed records they encode
  (`ProviderSelectionV1` for `selection_json`, a typed removal-evidence DTO for
  `candidate_json`, a typed usage record for `usage_json`, the typed safe-projection
  record for `safe_projection_json`). If a string is genuinely opaque transport for one
  specific adapter, document that single consumer and validate the content at admission
  instead of persisting caller text verbatim.
- **Verification:** Add a round-trip test that builds each record through the typed writer
  and reads it back through the typed reader (no string slicing), and a rejection test for
  malformed content at admission. Run
  `cargo nextest run -p intention-storage -p intention-storage-sqlite --tests` and
  `make quick`.

### P2-11. Removal-evidence decoder slices JSON strings without unescaping

- **Priority:** P2
- **Area:** State and configuration
- **Category:** correctness (asymmetric writer and reader)
- **Location:** `crates/intention-storage-sqlite/src/control_plane.rs:3091` (`decode_removed_identity_list`)
- **Confidence:** high
- **Defect:** `decode_removed_identity_list` scans a JSON array of strings and pushes the
  raw bytes between the quotes (`item[1..item.len() - 1]`) instead of decoding the JSON
  string. Any identity containing an escape sequence (`\"`, `\\`, `\uXXXX`) decodes to the
  escaped text rather than the real value, so after a restart the rebuilt prepared
  candidate holds `removed_profile_ids` / `removed_kind_ids` that do not match the durable
  identities. The removal path is the one place where these identities drive catalog
  mutation, so a mismatch silently changes which profiles are tombstoned during
  roll-forward.
- **Observed evidence:** `control_plane.rs:3089-3092`:
  `let (item, after) = json_string_span(bytes, index)?; identities.push(item[1..item.len() - 1].to_owned());`.
  The writer (`crates/intention-application/src/provider_catalog.rs:1586-1602`,
  `removal_candidate_json` with `json_string`) escapes `"` and `\`, so an escaped identity
  round-trips incorrectly. Note that sibling scanners in the same file do use proper
  decoding (`decode_json_string`, `decode_json_string_list` parse spans with
  `serde_json::from_str`).
- **Consequence:** Durable removal evidence and the in-memory prepared candidate can
  disagree about which identities were removed, with no error raised; a roll-forward then
  tombstones the wrong set. The defect is invisible today because identifiers are
  generated from a closed vocabulary and every test uses plain identities (`P3-15`).
- **Remediation:** Decode each element with `serde_json::from_str::<String>(item)` (as
  `decode_json_string_list` already does) instead of raw slicing, and remove the
  hand-rolled span arithmetic where a shared decoder exists.
- **Verification:** Add a storage fixture whose removed identity contains a quote and a
  backslash, write it through the real writer (`removal_candidate_json`), reload the
  candidate, and assert the loader returns the original identities. Add a malformed
  evidence case that must fail typed. Run
  `cargo nextest run -p intention-storage-sqlite --tests` and `make quick`.

### P2-12. `by_profile` usage totals are labelled with the last row's revision and model

**Accepted decision:** D-04, option A (see Appendix G.4): aggregation returns one result per
identity instead of one total labelled with the last row.

- **Priority:** P2
- **Area:** Application services
- **Category:** correctness (order-dependent misattribution)
- **Location:** `crates/intention-application/src/session_selection.rs:690` (`aggregate_usage`, `by_profile` projection)
- **Confidence:** high
- **Defect:** `aggregate_usage` sums request and unit counts across every in-period
  aggregate row of a profile but assigns `provider_profile_revision_id` and `model_id`
  from the last iterated row. A profile with usage under two revisions or two models
  therefore reports one identity with the combined totals, so the reported revision and
  model are storage-order dependent and misattribute usage to a revision that did not
  produce those units.
- **Observed evidence:** The loop body assigns
  `revision = aggregate.provider_profile_revision_id; model = aggregate.model_id;` after
  `request_count.saturating_add(...)`. Storage
  (`load_provider_usage_by_profile`) returns all rows for the profile ordered by
  `provider_profile_revision_id, model_id, usage_period_start`. The only covering test
  (`by_profile_aggregates_in_period_aggregates_only`,
  `crates/intention-application/tests/m5_session_selection.rs:2603`) seeds a single
  revision and model, so the cross-revision case is untested.
- **Consequence:** Usage reporting and any pricing or quota consumer reading this
  projection see combined totals stamped with an arbitrary identity, which can understate
  or overstate a specific revision and breaks attribution for the control plane's usage
  surface.
- **Remediation:** Either filter and group by the requested identity (return one
  aggregation per revision and model, or a bounded `Vec` of them), or drop the identity
  fields from the by-profile projection and require `by_revision_and_model` for attributed
  totals. State the chosen semantics in the ADR or architecture 29 text.
- **Verification:** Add a multi-revision, multi-model fixture in
  `crates/intention-application/tests/m5_session_selection.rs` that asserts the reported
  identity (or the returned set) and the totals per identity. Run
  `cargo nextest run -p intention-application --tests` and `make quick`.

### P2-13. Catalog acceptance commits durably before the all-or-nothing registry build

**Accepted decision:** D-05, option A (see Appendix G.5): the replacement registry is fully
built and its limits pre-validated before the durable catalog acceptance, so activation
cannot fail afterwards.

- **Priority:** P2
- **Area:** Application services
- **Category:** correctness (partial activation on a post-commit failure)
- **Location:** `crates/intention-application/src/provider_catalog.rs:714` (durable accept) versus `:724` (registry build) and `:1199` (`activate_registry`)
- **Confidence:** medium
- **Defect:** In the auto-accept path of `prepare_candidate`, the durable
  `accept_provider_catalog` call precedes `build_registry_from_candidate` and
  `activate_registry`. If the registry build or the activation fails, the durable active
  catalog has already advanced while the in-memory registry, the admissions map, and the
  gate still describe the previous revision, and the caller receives an error (so it
  believes nothing changed). After a restart the new revision is loaded and the provider
  control plane degrades to `Blocked`, with no way back except a corrected configuration.
  This contradicts the module's own all-or-nothing activation contract.
- **Observed evidence:** `self.catalog.accept_provider_catalog(...)?` at
  `provider_catalog.rs:714`, then
  `let (built, admissions) = self.build_registry_from_candidate(&kind_descriptors, &profiles, next_revision)?;`
  at `:724` and `self.activate_registry(built, admissions)?` at `:1199`.
  `PrivateRegistry::build_all` propagates `factory.build(material)?`
  (`crates/intention-application/src/provider_registry.rs:139`) and `activate` rejects maps
  larger than `MAX_ACTIVE_PRIVATE_ENTRIES`. No test injects a failing factory build
  (`CountingFactory` always succeeds). The removal path has a documented roll-forward
  (PR24-004); the auto-accept path has none.
- **Consequence:** A prepared catalog can be durable but not executable, with the
  in-memory gate disagreeing with storage until the next restart, and the caller's error
  does not describe the durable advance. Operators see a `Blocked` readiness state that no
  prior action explains, and the only recovery is editing configuration.
- **Remediation:** Build the replacement registry and admissions before the durable
  acceptance, and commit acceptance plus gate and registry activation together only after
  the build succeeds; or, on a post-acceptance build failure, repair the durable state
  (roll the acceptance back or mark it as recovery-required with a typed reason) so
  durable and in-memory state agree and the degradation is self-describing.
- **Verification:** Add an injectable failing factory (a `CountingFactory` variant that
  fails for a declared kind or declaration) and a test that asserts either no durable
  advance at all, or a typed recovery-required state whose durable revision and in-memory
  gate agree after a reopen. Add a restart assertion that the readiness reason matches the
  recorded failure. Run `cargo nextest run -p intention-application --tests` and
  `make quick`.

### P2-14. Stale-socket probe can misjudge a live listener and `Drop` unlinks another host's socket

**Accepted decision:** D-06, options A **and** B (see Appendix G.6): identity-verified unlink
by device and inode, plus no unconditional path removal in `Drop`, with reclaim at the next
bind as the only removal path.

- **Priority:** P2
- **Area:** Composition and daemon (transport)
- **Category:** correctness (endpoint ownership)
- **Location:** `crates/intention-transport/src/lib.rs:883` (probe and unlink), with `:244-247` (retry bind), `:273-279` and `:335-341` (`Drop`)
- **Confidence:** medium
- **Defect:** Liveness is decided by one bounded connect with a 50 ms timeout. A live
  listener whose accept backlog is momentarily full fails the probe, is classified stale,
  and its socket file is unlinked, after which the retry bind succeeds and a second
  listener owns the same endpoint path. Both listeners' `Drop` implementations then
  unconditionally remove that path, so whichever host exits first unlinks the other's live
  socket and leaves a running daemon unreachable to new clients.
- **Observed evidence:** `crates/intention-transport/src/lib.rs:880-888`
  (`connect_sync().is_ok()` with `ConnectWaitMode::Timeout(Duration::from_millis(50))`
  then `fs::remove_file(&endpoint.path)`); `:244-247` retries the bind after the reclaim
  returns true; `:273-279` and `:335-341` run
  `let _ = fs::remove_file(&self.endpoint.path);` with no identity check. The accompanying
  comment states the probe's intent, and no test exercises a live but slow listener.
- **Consequence:** A transient backlog saturation can hand the endpoint to a second
  process, and the first process to exit then destroys the survivor's socket path, so
  clients get connection errors while a healthy daemon keeps running. The failure is
  timing-dependent and therefore hard to diagnose from logs.
- **Remediation:** Make the unlink identity-verified: stat the socket before and after the
  probe and remove it only when the device and inode are unchanged, and have `Drop`
  remove the path only when it still resolves to the socket this listener created
  (compare the bound socket's identity). Alternatively adopt the platform's native reclaim
  semantics for stale socket files instead of a connect probe.
- **Verification:** Add transport tests that (a) hold a live listener whose backlog is
  saturated and assert the reclaim refuses to unlink, and (b) assert that dropping one
  listener does not remove a socket owned by another (identity check). Keep the existing
  non-Unix fail-closed stub test. Run
  `cargo nextest run -p intention-transport --tests` on Unix and `make quick`.

### P2-15. Six public model types have no production consumer

**Accepted decision:** D-16, option (b) (see Appendix G.11.6): the six types are audited against
the roadmap's M6 to M9 plan; the ones a named slice will consume are declared with an evidence
anchor and the rest are deleted.

**Remediation home:** Package B → D-16 (b), audited with `P2-06` and `P2-07`.

- **Priority:** P2
- **Area:** Model and provider adapters
- **Category:** expediency (speculative public surface)
- **Location:** `crates/intention-model/src/lib.rs:1427` (`ReasoningUsageDto`), with `:1117` (`ParserLimitsV1`),
  `:1195` (`ServerSideParserConfigV1`), `:1087` (`ProviderNativePreservationControlsV1`),
  `:1302` (`ModelCapabilityEnvelopeV1`), `:961` (`ResponsesReasoningMode`)
- **Confidence:** high
- **Defect:** Six public model types (roughly 350 lines with their helpers) exist with no
  production consumer, so they are speculative API that must be maintained and can
  silently diverge from the protocol and domain families that actually carry the same
  concepts. `ReasoningUsageDto` additionally documents a consumer that does not exist.
- **Observed evidence:** A grep over `crates/` finds only
  `crates/intention-model/tests/m6_reasoning_surface.rs` referencing `ReasoningUsageDto`
  (`:1427`), `ParserLimitsV1` (`:1117`), `ServerSideParserConfigV1` (`:1195`),
  `ProviderNativePreservationControlsV1` (`:1087`), `ModelCapabilityEnvelopeV1` (`:1302`),
  and `ResponsesReasoningMode` (`:961`). The only consumer of the preservation controls
  (`with_preservation_controls`) was deleted in wave 6 (`8c75fd7`).
  `crates/intention-types/src/model.rs:90` (`UsageDto::Reported`) carries no reasoning
  field, so the documented consumer of `ReasoningUsageDto` does not exist. The real wire
  and canonical families live in
  `crates/intention-protocol/src/contract_families.rs:2624`, `:3830`, `:3843`, `:3903` and
  `crates/intention-domain/src/provider_selection.rs:168`.
- **Consequence:** Two parallel vocabularies for the same concepts invite divergence
  (a value accepted by the model surface but not by the protocol or domain surface), and
  the unused surface inflates the coverage denominator while providing no behaviour.
  This is the class ADR 0038's simplicity and single-version rules ask to remove.
- **Remediation:** Either wire each type into its declared Slice 2 consumer (config and
  application flow for the capability envelope, adapters for preservation and parser
  controls) or remove the type together with its `m6_reasoning_surface` assertions.
  Correct the `ReasoningUsageDto` doc comment if it stays.
- **Verification:** After the change, a grep for each removed or newly wired identifier
  must show the intended consumer (and no test-only usage). If a type is kept, add the
  consumer path plus an evidence anchor in ADR 0037 and the reconciliation register. Run
  `cargo nextest run -p intention-model -p intention-provider-generic-chat -p intention-provider-openrouter --tests`
  and `make quick`.

### P2-16. SDK API errors are classified by an optional type field, not by HTTP status

**Accepted decision:** D-12, option (a) (see Appendix G.11.2): retryability comes from the HTTP
status (429 and 5xx retryable, other 4xx not), the `type` string is a secondary signal, and the
pre-existing defect is fixed in this change.

**Remediation home:** Package C → D-12 (a). The defect exists on `main`; fixing it here is the
accepted scope, so it is not carried as a separate open item.

- **Priority:** P2
- **Area:** Model and provider adapters
- **Category:** correctness (retryability misclassification)
- **Location:** `crates/intention-provider-generic-chat/src/lib.rs:684` (`map_openai_error`)
- **Confidence:** medium
- **Defect:** `map_openai_error` derives retryability from `api_error.r#type` and treats a
  missing type as retryable, while the authoritative `status_code` in the same
  `ApiErrorResponse` is unused. A gateway that returns 400, 401, or 403 without a `type`
  field is therefore reported as `generic_chat_provider_unavailable` with `Delayed` retry,
  so the runtime retries a permanent rejection up to the attempt maximum and records the
  wrong failure class. Conversely, a 5xx carrying a non-matching type is labelled
  non-retryable.
- **Observed evidence:** `crates/intention-provider-generic-chat/src/lib.rs:684-687`:
  `OpenAIError::ApiError(error) => matches!(error.api_error.r#type.as_deref(), None | Some("rate_limit_exceeded" | "server_error"))`.
  The `async-openai` 0.42 `ApiErrorResponse` exposes `status_code: reqwest::StatusCode`,
  which the mapping never reads. The identical expression exists at `origin/main`, so the
  defect is pre-existing and was not introduced by this PR; it is reported because the PR
  refreshes this SDK and its fixtures and the review brief explicitly covers SDK error
  mapping.
- **Remediation:** Classify by `error.status_code.as_u16()` (429 and 5xx retryable, other
  4xx non-retryable) and use the `type` string only as a secondary signal. Add fixtures
  for an untyped 400 and an untyped 500 so the classification is pinned.
- **Verification:** Unit tests constructing `ApiErrorResponse` values with an untyped 400,
  an untyped 500, and a typed `rate_limit_exceeded` 429, asserting the resulting
  `ProviderErrorDto` retryability. Run
  `cargo nextest run -p intention-provider-generic-chat --tests` and `make quick`, then
  re-run `make e2e-real-api` to confirm the live channel still records the same normalized
  failure classes for an invalid credential.

### P2-17. Live run asserts model wording (`READY`) outside the bounded attempt budget

- **Priority:** P2
- **Area:** Tooling, live proof and quality policy
- **Category:** generalizability (proof depends on model phrasing)
- **Location:** `crates/intention-daemon/tests/real_api_e2e.rs:1060` (read-turn assertion after `drive_tool_turn` returns)
- **Confidence:** high
- **Defect:** `drive_tool_turn` returns as soon as a durable succeeded call exists, but the
  caller then asserts that the assistant text contains `READY`. That is the only
  reply-word assertion in the file (the five prompts asking for exactly `DONE` are never
  checked), so a live model that completes the read call but answers without the literal
  token fails the whole opt-in run even though every durable fact the ADR requires was
  recorded. The failure is not absorbed by `TOOL_TURN_ATTEMPTS` because the assertion
  sits after the retry loop, which makes the proof depend on provider phrasing rather
  than on durable facts.
- **Observed evidence:** Lines 1046-1064 call `drive_tool_turn(...)` and then
  `assert!(read_run.assistant_content().to_ascii_uppercase().contains("READY"), ...)`.
  `drive_tool_turn_with` returns on
  `observed.has_tool_call(tool) && !observed.succeeded_contents(tool).is_empty()`
  (lines 941-944). The glob, grep, write, edit and execute prompts ask for "exactly DONE"
  and never assert it.
- **Consequence:** The opt-in live run can fail for a reason unrelated to the behaviour
  under test, producing a false negative that costs a manual rerun and erodes trust in the
  recorded evidence. It also contradicts the ADR 0040 principle that the run's value is
  durable-fact evidence.
- **Remediation:** Delete the `READY` assertion (the durable read content plus the
  tool-call and result facts are the evidence), or move it inside the attempt loop and
  count a missing token as one bounded attempt. Remove the "reply with exactly
  READY/DONE" wording from the prompts so no assertion depends on model phrasing.
- **Verification:** Re-run `make e2e-real-api` twice and confirm the harness still passes
  while asserting only durable facts; add a grep-style guard that no assistant-text
  assertion exists outside the durable-fact helpers. Record the new run in EVD-063 per
  ADR 0040.

## P3 findings

### P3-01. Provider-native dialect paths are hard-coded in the domain with no consumer

- **Priority:** P3
- **Area:** Domain layer
- **Category:** generalizability
- **Location:** `crates/intention-domain/src/reasoning_history.rs:17` (`REASONING_DIALECT_VALUES`)
- **Confidence:** medium
- **Defect:** `REASONING_DIALECT_VALUES` bakes provider-native field paths (for example
  `reasoning_details[].message.thinking` and `thinking_token_budget`) into the domain
  crate, but ADR 0037 assigns dialect decoding to the provider adapters, and ADR 0038
  wave 6 removed the only provider-side decoder. Nothing validates a declared dialect
  today, so the table is dead weight in the wrong layer, and adding a new provider
  dialect would require editing domain source rather than declaring a descriptor.
- **Observed evidence:** `reasoning_history.rs:17-26` lists the eight provider field
  paths. A grep for `REASONING_DIALECT_VALUES` / `validate_reasoning_dialect` outside that
  file returns only the `lib.rs` re-export and the module's own unit test.
  `crates/intention-provider-generic-chat` contains no dialect reference at all after the
  wave 6 removal.
- **Consequence:** The domain crate carries provider-specific vocabulary with no
  authority, so the next dialect either duplicates this table in the adapter or mutates
  the domain for a provider concern, violating adapter isolation and the descriptor-driven
  pattern used elsewhere in the same PR.
- **Remediation:** Move the dialect vocabulary to the provider descriptor or adapter layer
  (or a descriptor-declared closed table validated at build time) and drop the unused
  domain constant. If it is deliberately pre-staged for the M6 normalized-reasoning
  driver, declare it as a Slice 3 or M6 contract with an evidence anchor and a test that
  consumes it.
- **Verification:** After the change, no provider-native field path may appear in
  `intention-domain`; a grep guard plus the descriptor test (if moved) proves the owner.
  If the constant is declared for M6, add the consuming test and the ADR row. Run
  `cargo nextest run -p intention-domain --tests` and `make quick`.

### P3-02. Id bound counts bytes while documented as characters and disagrees with the wire DTO

**Accepted decision:** D-13, option (a) (see Appendix G.11.3): both layers count characters and
ADR 0037 Appendix A states one number per identifier field.

**Remediation home:** Package C → D-13 (a), executed with D-01 (Appendix G.1, item 6), because the
ADR field table this finding appeals to is already moved by that decision.

- **Priority:** P3
- **Area:** Domain layer
- **Category:** contract (bound unit and value divergence between layers)
- **Location:** `crates/intention-domain/src/provider_catalog.rs:98` (`validate_provider_string`, used from `:105`)
- **Confidence:** medium
- **Defect:** `validate_provider_string` compares `value.len()` (bytes) but is documented
  and named as a character bound, so a 30-scalar multi-byte identifier inside the
  documented bound is rejected. The same logical identifier is additionally accepted up
  to 256 characters by the public wire DTO and rejected above 63 bytes by the canonical
  record, so one value can pass one boundary and fail the other.
- **Observed evidence:** `provider_catalog.rs:98-105` uses `value.len() > max_chars`.
  The wire contract validates the same fields with `valid_text(field, 256, ...)` using
  `value.chars().count()` (`crates/intention-protocol/src/contract_families.rs:158-172`).
  ADR 0037 Appendix A states "up to 256 scalar values" for `profile_id`, `revision_id`,
  `provider_kind_id`, and `model_id`, while the domain uses
  `MAX_PROVIDER_ID_CHARS = 63` (the test `provider_ids_are_limited_to_63_characters` pins
  63).
- **Consequence:** Validation is inconsistent across layers for the same value,
  producing rejections that contradict the documented contract and a canonical bound that
  silently differs from the wire bound; multi-byte identifiers are treated more strictly
  than the documentation promises.
- **Remediation:** Use `chars().count()` for the documented bound and align the canonical
  id bound with the ADR and wire value (or record the deliberately stricter canonical
  bound in the ADR field table so both layers are intentional). Update the affected tests.
- **Verification:** Add fixtures with a multi-byte identifier inside and outside the
  documented bound at both layers and assert consistent acceptance, plus a test that the
  canonical and wire bounds are equal (or explicitly documented as different). Run
  `cargo nextest run -p intention-domain -p intention-protocol --tests` and `make quick`.

### P3-03. `validate_endpoint` accepts an endpoint with an empty authority

**Remediation home:** Package E, item E1 (Appendix D). Small fail-closed correction at the shared validator that `intention-config` delegates to; no decision required.

- **Priority:** P3
- **Area:** Domain layer
- **Category:** correctness (fail-open admission of an unusable endpoint)
- **Location:** `crates/intention-domain/src/provider_catalog.rs:146` (the HTTPS branch of `validate_endpoint`)
- **Confidence:** medium
- **Defect:** The documentation requires an absolute HTTPS endpoint, but the `https`
  branch only checks that the remainder after `https://` is non-empty, so `https:///v1`
  and `https://:8080/v1` validate. `intention-config` delegates to this same function, so
  a hostless endpoint can enter a profile revision, the canonical selection, and the
  identity digest, and only fail later inside the provider driver instead of failing
  closed at admission.
- **Observed evidence:** `provider_catalog.rs:147-165`: for the `https` prefix only
  `rest.is_empty()` is rejected; there is no host or authority check (the loopback
  authority parsing exists only for the `http` branch). `crates/intention-config/src/lib.rs:289-305`
  calls this same `validate_endpoint` for configured endpoints.
- **Consequence:** A configuration or catalog that cannot ever connect is admitted and
  digested as valid, so the failure surfaces as a runtime provider error with a different
  code, and the durable selection records an endpoint that cannot be parsed into an
  authority.
- **Remediation:** Reject an empty authority: require a non-empty host after the scheme
  and before any `/`, `:` or `[`. Add a rejection fixture for `https:///v1` and
  `https://:8080/v1` at both the domain and the config layer.
- **Verification:** Add the rejection fixtures to the domain catalog tests and the config
  control-plane tests; add a positive case for a valid host with a port. Run
  `cargo nextest run -p intention-domain -p intention-config --tests` and `make quick`.

### P3-04. Command DTO documents and implements legacy omitted-field tolerance

**Remediation home:** Package E, item E4 (Appendix D). Removing the legacy tolerance is mechanical once the owner confirms the tolerance was not a deliberate ADR 0038 binding decision; the alternative recorded in the remediation is to pin it with a test instead.

- **Priority:** P3
- **Area:** Domain layer
- **Category:** contract (compatibility decode class kept alive)
- **Location:** `crates/intention-domain/src/lib.rs:598` (documentation), attributes at `:620-623`
- **Confidence:** medium
- **Defect:** The doc comment justifies the `#[serde(default)]` on the override fields as
  accepting "a legacy command that omits them", which is exactly the compatibility decode
  class ADR 0038 removes ("additive fields become required on the wire", no legacy-shape
  deserializer). The tolerance branch is real code with no current-shape requirement
  behind it, so an outdated producer is silently accepted.
- **Observed evidence:** `intention-domain/src/lib.rs:596-600` (doc) and `:620-623`
  (`#[serde(default)]` on `profile_override` and `expected_profile_revision` in
  `RawSendUserTurnCommandDto`). `crates/intention-domain/tests/m5_session_selection_overrides.rs`
  exercises the fields-present case and the expected-revision-without-override case but
  never asserts that absence is rejected.
- **Consequence:** A producer emitting the older shape is served without an error, so the
  single-current-version guarantee is weaker than documented and the tolerance cannot be
  removed later without breaking a silent consumer.
- **Remediation:** Drop the `#[serde(default)]` attributes so absence becomes a decode
  error while `null` remains a valid current value, and reword the doc to describe the
  current optional-state fields. If the tolerance is deliberate, record it as an explicit
  ADR 0038 binding decision with a test that pins the accepted shape.
- **Verification:** Add a rejection fixture that omits both fields and asserts the decode
  error, plus the existing present and null cases. Run
  `cargo nextest run -p intention-domain --tests` and `make quick`.

### P3-05. Tombstone documentation still claims a permanent identity record

- **Priority:** P3
- **Area:** Domain layer
- **Category:** docs (stale contract description)
- **Location:** `crates/intention-domain/src/provider_catalog.rs:685` (profile tombstone), duplicated at `:820-827` (kind tombstone)
- **Confidence:** high
- **Defect:** The first doc line states "A permanent safe identity record for one removed
  provider profile", which the next paragraph and the implementation (PR24-017) explicitly
  contradict: tombstones are append-only removal-history events keyed by the removed
  catalog revision and are not an admission veto. The stale line misdescribes the durable
  contract for both tombstone types.
- **Observed evidence:** `provider_catalog.rs:685-692` (profile) and `:820-827` (kind)
  keep the "A permanent safe identity record" line immediately above "Removal history is
  durable evidence, not admission authority ... an identifier removed by one catalog may
  be reintroduced by a later accepted catalog (PR24-017)". The tests assert that
  re-removal produces a fresh event, which matches the second description only.
- **Consequence:** A reader or reviewer can implement an admission veto over tombstones
  (or a uniqueness assumption) based on the stale sentence, producing behaviour that
  contradicts the accepted design and the tests.
- **Remediation:** Delete the "permanent safe identity record" line and keep the removal
  history wording for both tombstone types.
- **Verification:** Documentation-only change; `make docs-check` must pass, and a search
  for "permanent safe identity record" must return no code or documentation hit that
  contradicts the removal-history description.

### P3-06. Profile and kind tombstone codecs are byte-for-byte duplicates

- **Priority:** P3
- **Area:** Domain layer
- **Category:** dead-code / duplication (framing has two owners)
- **Location:** `crates/intention-domain/src/provider_catalog.rs:822` (`ProviderKindTombstoneDto`), helper at `:1034-1054`
- **Confidence:** medium
- **Defect:** `ProviderKindTombstoneDto` is a copy of `ProviderProfileTombstoneDto`: the
  same five fields, the same encode and decode bodies, the same identity recomputation,
  differing only in the id validator and the error code. In addition, `tombstone_identity`
  hand-rolls the IRCR frame instead of using the shared `CanonicalRecordBuilder`, and the
  private `record()` helper is copy-pasted across four modules. Every framing fix must be
  applied twice, and the hand-rolled writer can diverge from the reader's rules.
- **Observed evidence:** `provider_catalog.rs:693-818` and `:822-955` contain structurally
  identical `new`, `encode`, `decode`, and `validate` bodies. `tombstone_identity`
  (`:1034-1054`) re-implements the frame header and field encoding that
  `CanonicalRecordBuilder::finish` (`crates/intention-domain/src/canonical.rs`) already
  performs. The shared private `record()` appears at `provider_catalog.rs:1056`,
  `provider_selection.rs:675`, `context_projection.rs:490`, and `reasoning_history.rs:401`.
- **Consequence:** Two codecs for one framing rule can drift (one accepting what the other
  rejects), and a framing change (tag, length encoding, digest input) has to be made in
  several places; the duplication also inflates the rejection-suite surface without adding
  coverage of distinct behaviour.
- **Remediation:** Parameterize one tombstone codec over the id validator and the error
  code (or keep one generic helper), and build the identity bytes with
  `CanonicalRecordBuilder` so framing has a single owner. Consider hoisting the repeated
  `record()` helper into the canonical module.
- **Verification:** The existing tombstone goldens and rejection tests must pass unchanged
  after the merge; add one test that asserts both tombstone types produce the same framing
  bytes for equivalent inputs (so the shared codec is exercised for both). Run
  `cargo nextest run -p intention-domain --tests` and `make quick`.

### P3-07. Control-plane DTOs bypass declared invariants at the wire decode boundary

**Accepted decision:** D-08, option 1 plus deserialize (see Appendix G.8): every
invariant-bearing control-plane DTO decodes through a private raw shape whose `Deserialize`
validates, matching the M3 and M4 pattern; the exact-version check of `P2-03` is implemented
in the same change.

- **Priority:** P3
- **Area:** Protocol and client surface
- **Category:** dto (validation not enforced at decode)
- **Location:** `crates/intention-protocol/src/contract_families.rs:1787` (`ProviderCatalogPageDto`), representative of the new family
- **Confidence:** medium
- **Defect:** The new contract-family DTOs derive `Deserialize` directly, so blank or
  over-long text, unsorted or duplicated catalog entries, `has_more` versus next-token
  inconsistency, and schema-version violations all decode successfully. Enforcement
  depends entirely on a caller remembering to invoke `validate()`. The same crate's M3 and
  M4 DTOs (`SessionSnapshotDto`, `RunLiveBatchDto`, `SessionEventTailBatchDto`) decode
  through a private raw shape plus constructor validation for exactly this reason, and the
  client returns decoded projections (`list_provider_profiles`,
  `provider_catalog_status`, `session_provider_profile`, `provider_usage`, and so on)
  without validating them.
- **Observed evidence:** `contract_families.rs:1787` derives `Serialize, Deserialize`
  with invariants only in `validate()` at `:1805-1845`. Compare
  `crates/intention-protocol/src/lib.rs:527-560` (`RunLiveBatchDto` raw plus `TryFrom`)
  and `:712-760` (`SessionSnapshotDto`). `crates/intention-client/src/lib.rs:455-470`
  returns `ProviderCatalogPageDto` unvalidated.
  `crates/intention-client/tests/session_selection_client.rs:640-645` claims the DTO
  "validates ... at decode time" while only calling `validate()` on locally built
  fixtures.
- **Consequence:** A malformed or hostile frame can enter the process as a structurally
  valid but semantically invalid DTO, and the boundary guarantee moves from the type
  system to reviewer discipline; the client's own test comment documents a guarantee the
  code does not provide.
- **Remediation:** Decode each invariant-bearing family through a private raw shape whose
  `Deserialize` calls `validate()` (or a validated newtype with private fields), matching
  the M3 and M4 pattern, and validate decoded projections on the client before returning
  them.
- **Verification:** Add decode-time rejection fixtures for each new family (blank text,
  duplicate entries, inconsistent paging, wrong schema version) that construct the wire
  JSON directly rather than the typed struct, and a client test asserting an invalid
  projection is rejected. Run
  `cargo nextest run -p intention-protocol -p intention-client --tests` and `make quick`.

### P3-08. `ConfigurationProjectionDto.reload_status` is an untyped status string

**Accepted decision:** D-14, option (a) (see Appendix G.11.4): the field becomes a closed enum
validated by serde, implemented inside D-08's family conversion.

**Remediation home:** Package C → D-14 (a), executed inside D-08 (Appendix G.8, item 5). Deletion
was verified available (no production reader) and was not taken.

- **Priority:** P3
- **Area:** Protocol and client surface
- **Category:** dto (closed vocabulary not represented)
- **Location:** `crates/intention-protocol/src/contract_families.rs:3589`
- **Confidence:** medium
- **Defect:** `reload_status` is a bare `String` whose only production value is the
  hard-coded literal `"active"`, and `validate()` accepts any bounded text. The closed
  reload vocabulary that sibling families model with enums
  (`ProviderCatalogActivationState`, `ProviderReadinessDto`,
  `ConfigurationCommitOutcomeDto`) is therefore neither representable nor checkable:
  consumers cannot exhaustively match it, and no peer can detect an unknown status.
- **Observed evidence:** `contract_families.rs:3589` declares the field with only
  `valid_text` and credential checks at `:3591-3615`. The sole producer is
  `crates/intention/src/lib.rs:1163-1164`
  (`reload_status: "active".to_owned()`); no other value exists in the workspace, and no
  enum is used.
- **Consequence:** The status field cannot be matched exhaustively, so a future status
  arrives as an unchecked string, and the projection cannot express the closed states the
  configuration control plane already uses elsewhere.
- **Remediation:** Replace the `String` with a closed enum (for example
  `ConfigurationReloadStatusDto { Active, ... }`) validated by serde, and update the
  producer and the ADR wording.
- **Verification:** Add a round-trip test for every enum value and a decode rejection test
  for an unknown string. Run `cargo nextest run -p intention-protocol -p intention --tests`
  and `make quick`.

### P3-09. Removal accept, reject, and expire have no fault-injection coverage

**Remediation home:** Package F, item F1 (Appendix D). Test-and-evidence hardening; no production change.

- **Priority:** P3
- **Area:** State and configuration
- **Category:** test-gap (transactional surface without rollback evidence)
- **Location:** `crates/intention-storage-sqlite/src/control_plane.rs:3276` (`accept_provider_catalog_removal`), with the `FaultPoint` enum at `crates/intention-storage-sqlite/src/lib.rs:1553-1570`
- **Confidence:** high
- **Defect:** `FaultPoint` has no variant for the removal lifecycle, so
  `accept_provider_catalog_removal`, `reject_provider_catalog_removal`, and
  `expire_provider_catalog_removal_candidate` commit with no test that a mid-transaction
  failure leaves the pending candidate and the catalog status unchanged. Every other
  multi-write transaction in this PR (turn acceptance, promotion, model facts, tool
  results, catalog acceptance, reload, provider selection, queue, held run, usage) has an
  arm-fault loop that reopens the database and asserts no partial rows; the removal
  lifecycle is the one transactional surface with none, and it is the surface that mutates
  `provider_catalog_state.status`.
- **Observed evidence:** The `FaultPoint` enum at `lib.rs:1553-1570` lists `Events`,
  `ModelFacts`, `Projection`, `Snapshot`, `ToolResult`, `CatalogRevision`,
  `CatalogTombstone`, `CatalogProjection`, `CatalogAudit`, `ReloadSnapshot`,
  `ReloadAudit`, `ProviderSelection`, `UnavailableQueue`, `UsageAggregate`, and `HeldRun`,
  with no removal variant. `accept_provider_catalog_removal` (`control_plane.rs:3276`)
  performs the candidate update, the audit insert, and `tx.commit` with no `self.fault`
  call. Existing fault tests live at `lib.rs:2539`, `:2587`, `:2638`, `:2750`, `:2844`,
  `:2994`, `:3109`, `:3152`, `:3203`, `:3295`, `:3352`.
- **Consequence:** The removal path can partially commit (candidate marked accepted while
  the audit row or the catalog status update is missing, or the reverse) and no test or
  gate would notice; the durable removal evidence is exactly the material a restart uses
  to rebuild a prepared candidate.
- **Remediation:** Add a `FaultPoint::RemovalLifecycle` (or per-stage points) armed
  between the candidate update and the audit insert in each removal operation, plus a test
  that reopens the store and asserts the candidate status, `operation_id`, `completed_at`,
  and `provider_catalog_state.status` are all unchanged.
- **Verification:** The new fault test must fail before the fix if the transaction is not
  atomic and pass after it; run `cargo nextest run -p intention-storage-sqlite --tests`
  and `make quick`.

### P3-10. `ensure_catalog_state_seed` duplicates the schema DDL seed row

- **Priority:** P3
- **Area:** State and configuration
- **Category:** expediency (second create path for one row)
- **Location:** `crates/intention-storage-sqlite/src/control_plane.rs:3536`, DDL at `:129-131`, call site at `crates/intention-storage-sqlite/src/lib.rs:158-161`
- **Confidence:** high
- **Defect:** The M5 DDL already ends with
  `INSERT OR IGNORE INTO provider_catalog_state(...) VALUES (1, NULL, NULL, 'preparing', ...)`,
  and `open()` runs that batch immediately before calling `ensure_catalog_state_seed`,
  which executes the identical statement. The helper is documented as defensive ("the
  schema DDL normally creates it"), so it is dead in the only production call path, and it
  adds a second place that must change whenever the seed columns change. ADR 0038 requires
  one execution path per operation.
- **Observed evidence:** `control_plane.rs:129-131` (DDL seed) and `control_plane.rs:3536-3545`
  (helper executing the same statement). `lib.rs:158-161` calls
  `execute_batch(SCHEMA_SQL + SCHEMA_M5_SQL)` and then `ensure_catalog_state_seed`; the
  helper has no other caller.
- **Consequence:** Two seeds for one row can drift (for example the DDL seed gains a
  column default while the helper keeps the old column list), and the duplicate makes the
  creation path harder to reason about than the single-batch design ADR 0038 mandates.
- **Remediation:** Delete `ensure_catalog_state_seed` and its call site, keeping the DDL
  seed as the single create path.
- **Verification:** The schema-creation test already asserts the seed row exists
  (`current_storage_schema_is_created_completely_and_remains_authoritative`), so removing
  the helper must keep that test green; run
  `cargo nextest run -p intention-storage-sqlite --tests` and `make quick`.

### P3-11. `persist_resolved_run_provider_selection` has no production caller

- **Priority:** P3
- **Area:** State and configuration
- **Category:** dead-code (second write path for one row)
- **Location:** `crates/intention-storage-sqlite/src/control_plane.rs:2639`
- **Confidence:** medium
- **Defect:** The provider-selection repository method is implemented and tested only from
  storage tests. Production commits the fresh-run selection through
  `AcceptUserTurnInputDto::with_provider_selection` inside `accept_user_turn` and transfers
  queued selections inside `promote_oldest_queued_turn`, so the standalone persist method
  is a second, unreachable write path for the same row. That is exactly the redundant
  surface ADR 0038's one-execution-path rule asks to remove; if it is intended as the
  recovery or repair seam, it needs a caller and a fault test on that path.
- **Observed evidence:** A repository-wide grep for
  `persist_resolved_run_provider_selection` outside the trait and impl returns only tests:
  `crates/intention-application/tests/m5_session_selection.rs:1546` (the fake), and
  `crates/intention-storage-sqlite/tests/sqlite_contracts.rs:1713`, `:2012`, `:2021`,
  `:2049`, `:2071`, `:2113`, `:2915`, `:3302`. The production paths are
  `crates/intention-storage-sqlite/src/lib.rs:809-816` (`with_provider_selection` inside
  `accept_user_turn`) and `lib.rs:494-498` (`insert_selection` during promotion).
- **Consequence:** Two implementations of one durable write can diverge (validation,
  fault points, digest computation), and the unused one still counts toward coverage while
  providing no executed behaviour.
- **Remediation:** Either delete `PersistResolvedRunProviderSelectionInputDto`, its impl,
  and the test surface, or wire the intended recovery producer and add a fault-injection
  test on it.
- **Verification:** After removal, a grep guard must return no hits outside the trait's
  consumer list. If wired, add a fault test that reopens the store and asserts no partial
  selection row. Run `cargo nextest run -p intention-storage-sqlite --tests` and
  `make quick`.

### P3-12. `restore_credential_document` returns the raw document when re-serialization fails

**Remediation home:** Package E, item E8 (Appendix D), executed with D-10 (Appendix G.10, item 6). Same helper and same file as the typed-edit rendering change, so one change owns it.

- **Priority:** P3
- **Area:** State and configuration
- **Category:** security-adjacent correctness (fail-closed for the credential, but the real cause is hidden)
- **Location:** `crates/intention-config/src/control_plane.rs:741`
- **Confidence:** medium
- **Defect:** The helper parses the credential-free document, inserts
  `provider.credential`, and re-serializes with `toml::to_string`, falling back to
  `text.to_owned()` (the original, credential-free text) on serialization failure, and it
  also returns the input unchanged when the document is not a table or has no `provider`
  table. In every failure branch the caller receives a document that silently lacks the
  credential rather than a typed error, so the downstream parse fails with
  `missing_provider_credential` and the operator sees a credential error for what is
  actually a document-shape problem.
- **Observed evidence:** `control_plane.rs:741-757`:
  `let Ok(document) = toml::from_str(...) else { return text.to_owned(); }; ... toml::to_string(&toml::Value::Table(document)).unwrap_or_else(|_| text.to_owned())`.
  The tests `restore_credential_document_escapes_values_and_passes_through_unsuitable_documents`
  (`crates/intention-config/tests/m5_control_plane_config.rs:849`) assert the pass-through
  behaviour, so it is intentional, but no error is surfaced.
- **Consequence:** Error diagnosis is misleading (a document-shape failure is reported as
  a missing credential), and a caller cannot distinguish "the operator did not configure a
  credential" from "the document cannot be edited by this path"; the silent pass-through
  also means a future caller could persist a document it believes carries the credential.
- **Remediation:** Return `DtoResult<String>` and fail with a typed validation error on a
  non-table document or a serialization failure, so the caller can distinguish the cases.
  The redaction guarantee itself is intact and must be preserved.
- **Verification:** Add tests for a non-table document and for a value that cannot be
  serialized, asserting the typed error; keep the escaping test. Run
  `cargo nextest run -p intention-config --tests` and `make quick`.

### P3-13. `load_run_config_snapshot` maps a decode failure to `run_configuration_unavailable`

**Remediation home:** Package E, item E5 (Appendix D). Error-mapping correction in the same storage loader family that D-05 reorders.

- **Priority:** P3
- **Area:** State and configuration
- **Category:** correctness (error mapping collapses corruption into unavailability)
- **Location:** `crates/intention-storage-sqlite/src/lib.rs:1141`
- **Confidence:** medium
- **Defect:** The final decode is
  `serde_json::from_str(&snapshot).map_err(|_| run_configuration_unavailable())`. A malformed
  persisted `snapshot_json` (corrupted bytes) is therefore reported as `Unavailable` /
  `Manual`, the same code as a genuinely unreadable row, so the caller cannot tell
  corruption from transient unavailability and will retry a permanently broken record.
  Every sibling loader in the crate uses `storage_decode_failed` (via `codec_error`) for
  malformed persisted records.
- **Observed evidence:** `lib.rs:1141-1145`; compare `load_config_snapshot` at
  `lib.rs:1629-1645` (`run_model_context_unavailable` for the same shape, also debatable)
  and `load_model_run_snapshot` at `lib.rs:1860` (`run_history_unavailable` for a decode
  failure). `codec_error()` (`storage_decode_failed`, `Internal`, never retryable) is used
  for every other persisted-record decode (`parse_mode`, `parse_status`, `run_projection`,
  `load_tail`).
- **Consequence:** Corrupted durable data looks transient, so retry loops and operator
  runbooks cannot converge; the error taxonomy for snapshots is inconsistent with the rest
  of the crate.
- **Remediation:** Map the serde decode failure to `storage_decode_failed` (`codec_error`)
  and keep `run_configuration_unavailable` for the missing-row case, or document why
  corruption and unavailability are deliberately merged for this specific boundary and
  update the trait doc accordingly.
- **Verification:** Add a unit test that writes a corrupted `snapshot_json` and asserts
  the returned code, plus the existing missing-row test asserting
  `run_configuration_unavailable`. Run
  `cargo nextest run -p intention-storage-sqlite --tests` and `make quick`.

### P3-14. Storage crate header claims no raw strings cross the boundary, contradicting the `*_json` fields

- **Priority:** P3
- **Area:** State and configuration
- **Category:** docs (boundary contract stated inconsistently)
- **Location:** `crates/intention-storage/src/lib.rs:990` (control-plane section header), with the crate header at `:1-5` and the field at `:1032`
- **Confidence:** low
- **Defect:** The crate-level documentation says the crate "exposes no connection,
  filesystem, SQL, path, or closure-based API across its boundary", and the control-plane
  section header says "Record JSON columns are opaque safe strings produced and consumed by
  the backend", while the same file declares public DTO fields named `*_json` and a
  `ProviderCatalogProfileEntryDto` whose documented purpose is to carry backend JSON. The
  two statements are in tension, so a reviewer or consumer cannot tell whether opaque JSON
  is a sanctioned boundary shape or a violation. The evidence register EVD-060 records that
  `serde_json::Value` violations were removed from this crate, which reinforces the
  impression that raw JSON is disallowed.
- **Observed evidence:** `intention-storage/src/lib.rs:1-5` (crate header), `:975-982`
  (control-plane section header), `:1032`
  (`pub safe_projection_json: String`, documented as "The opaque safe projection JSON
  produced by the backend"), and `reconciliation/evidence-register.md:75` (EVD-060
  recording the removed `serde_json::Value` violations).
- **Consequence:** The boundary rule is unreadable: one paragraph forbids raw JSON, the
  next sanctions it, and the fields exist. Contributors cannot decide the correct shape
  without an ADR reading, and the DTO-first policy cannot be enforced by review.
- **Remediation:** State the sanctioned exception explicitly in the boundary documentation
  (which JSON-typed fields are opaque by design and which consumer owns decoding them), or
  remove the fields per `P2-10`. Keep the crate header accurate either way.
- **Verification:** After the change, `make docs-check` passes and a reader can map every
  `*_json` field to a documented owner or find none; the boundary statement and the field
  list must agree in the same file.

### P3-15. Removal-evidence escaping is never round-tripped through the reader

- **Priority:** P3
- **Area:** State and configuration
- **Category:** test-gap (the fixture cannot detect the decoder defect)
- **Location:** `crates/intention-storage-sqlite/tests/m5_control_plane_repos.rs:1321`
- **Confidence:** medium
- **Defect:** The prepared-candidate and restart test writes removal evidence with plain
  identities (`"profile-a"`), and `corrupted_catalog_records_fail_typed_decode`
  (`:2150`) covers malformed shapes only. No test writes an identity containing a JSON
  escape and reads it back through `load_pending_removal_candidate`, which is exactly why
  the raw-slice decode defect in `P2-11` is invisible: a test using the real writer
  (`removal_candidate_json`) with an escaped identity would fail today.
- **Observed evidence:** `tests/m5_control_plane_repos.rs:1366` builds `candidate_json` by
  hand with `"removed_profiles":["profile-a"]` and no escape characters; the assertions at
  `:1400-1405` expect `vec!["profile-a"]`. The writer that does escape
  (`crates/intention-application/src/provider_catalog.rs:1586`, `json_string`) is not used
  by any storage test.
- **Consequence:** The durable removal evidence path has no coverage for the escaping
  contract, so a mismatch between writer and reader (or a future change to either) passes
  the suite and only appears after a restart in production.
- **Remediation:** Add a fixture that stores evidence produced by the escaping writer for
  an identity containing a quote and a backslash and asserts the loader returns the
  original identity. This fixture is the regression test for `P2-11`.
- **Verification:** The fixture must fail before the `P2-11` fix and pass after it; run
  `cargo nextest run -p intention-storage-sqlite --tests` and `make quick`.

### P3-16. Tombstoned admission branch and in-memory tombstone set are unreachable

- **Priority:** P3
- **Area:** Application services
- **Category:** dead-code (unreachable guard, fail-open lock handling)
- **Location:** `crates/intention-application/src/provider_catalog.rs:1000` (`registry_lookup`), with `record_tombstones` at `:734` and `:846`
- **Confidence:** high
- **Defect:** `registry_lookup` checks `is_tombstoned` only for keys that are present in
  the admissions map, but every activation replaces the admissions map wholesale with the
  accepted catalog's profiles, and `record_tombstones` clears the tombstone for every
  identifier present in that same accepted material. An identifier is therefore never
  both admitted and tombstoned, so the `provider_profile_tombstoned` error can never be
  returned and the `TombstoneSets` state is inert (it is also never rebuilt at startup).
  The lock is additionally ignored in a fail-open way (`if let Ok(mut tombstones)`,
  `is_ok_and`), so a poisoned lock silently disables the check rather than failing closed.
- **Observed evidence:** `record_tombstones` is called only at `:734` and `:846`,
  immediately after `activate_registry` installed a map containing exactly the active
  profile ids, and it removes those ids from the sets. `registry_lookup` performs
  `admissions.get(key)` before the tombstone check (`:1000`). The PR24-017 reintroduction
  test (`crates/intention-application/tests/m5_catalog_runtime.rs:1441`) asserts success
  but never exercises the tombstone branch.
- **Consequence:** The guard reads as an admission veto but cannot fire, so the code
  documents a policy it does not enforce; a poisoned lock would silently disable it if it
  ever became reachable; and the dead branch inflates the tested surface.
- **Remediation:** Delete the in-memory tombstone sets, `record_tombstones`, `is_tombstoned`,
  and the `provider_profile_tombstoned` arm (and the corresponding `registry_lookup` doc
  claims). If removal history must gate admission, make the durable tombstone the authority
  and add a test for the reachable rejection path, including the reintroduction case.
- **Verification:** After removal, a grep guard returns no hits and the catalog runtime
  tests pass unchanged. If the durable authority is used instead, add a test that a
  removed identifier is rejected on reintroduction and a test that a poisoned lock fails
  closed. Run `cargo nextest run -p intention-application --tests` and `make quick`.

### P3-17. Removal command's `source_recheck` flag is ignored and replaced by a constant

- **Priority:** P3
- **Area:** Application services
- **Category:** contract (write-only wire field)
- **Location:** `crates/intention-application/src/session_selection.rs:925` (`RemovalService::accept`), constant written at `crates/intention-application/src/provider_catalog.rs:679`
- **Confidence:** high
- **Defect:** `AcceptProviderCatalogRemovalCommandDto.source_recheck` is a validated wire
  field that the application service never reads, while the durable removal row is written
  with the hard-coded string `"health-recheck"`. A client can set the flag either way with
  no effect, and the durable evidence records a recheck mode that no caller chose.
- **Observed evidence:** `RemovalService::accept` (`session_selection.rs:925`) passes only
  `candidate_handle`, the expected revisions, `operation_id`, and `now` to
  `controller.accept_pending`; `provider_catalog.rs:679` writes
  `source_recheck: "health-recheck".to_owned()` into
  `CreateProviderCatalogRemovalCandidateInputDto`. `architecture/29` states that the
  command takes "a source recheck". All call sites (daemon, composition, client tests)
  pass `false`.
- **Consequence:** The wire surface promises control the implementation ignores, and the
  durable evidence asserts a recheck mode that did not come from the caller, so an auditor
  cannot reconstruct why removal was accepted.
- **Remediation:** Either consume the flag (record it as the durable recheck mode and, if
  appropriate, require it for acceptance) or remove it from the command DTO in the same
  change as the field it no longer feeds. Do not keep a wire field with no consumer.
- **Verification:** If consumed, add tests for both flag values asserting the recorded
  mode; if removed, a grep guard plus the protocol round-trip fixtures must show the field
  gone. Run `cargo nextest run -p intention-application -p intention-protocol --tests` and
  `make quick`.

### P3-18. Session default change emits no `SessionProviderProfileChanged` event

- **Priority:** P3
- **Area:** Application services
- **Category:** contract (declared event never emitted)
- **Location:** `crates/intention-application/src/session_selection.rs:425` (`SessionProfileService::set`)
- **Confidence:** high
- **Defect:** `architecture/29` requires `SetSessionProviderProfileCommandDto` to emit
  `SessionProviderProfileChanged`, but the application service only writes the durable
  default and returns an accepted DTO. No event DTO is constructed anywhere in the
  workspace, so subscribers and resync never observe the change. The module documentation
  additionally claims the services construct the protocol event DTOs, which no service
  does.
- **Observed evidence:** `SessionProfileService::set` (`session_selection.rs:425`) calls
  only `resolve_enabled_profile` and `set_session_provider_profile`. A grep for
  `SessionProviderProfileChangedEventDto` finds it only in `intention-protocol` and in the
  module doc at `session_selection.rs:12-14`. `architecture/29` states: "it changes only
  future intent and emits SessionProviderProfileChanged".
- **Consequence:** A client that relies on the event to refresh session state sees no
  notification, and the architecture text claims behaviour the code does not implement.
  this is the concrete instance behind `P2-07`.
- **Remediation:** Emit (and durably append) the typed event when `changed = true`, or
  amend `architecture/29` and the module doc to record that emission is deferred, and drop
  the inaccurate "services construct the protocol event DTOs" sentence.
- **Verification:** If emitted, add a facade test that performs the change and asserts both
  the emitted event and the durable append (storage read). If deferred, `make docs-check`
  plus a review of the amended text. Run
  `cargo nextest run -p intention-application --tests` and `make quick`.

### P3-19. New `m5_session_selection` test target is missing from the normative enumerations

- **Priority:** P3
- **Area:** Application services (documentation and policy consistency)
- **Category:** docs (policy, ADR, and register disagree)
- **Location:** `docs/intention-relay/architecture/12-quality-gates-and-makefile.md:276`
- **Confidence:** high
- **Defect:** This PR adds a 3,374-line integration target,
  `crates/intention-application/tests/m5_session_selection.rs`, and declares it in the
  machine-readable policy, but the quality-gate architecture section that enumerates the
  Slice 2 test targets still lists only `m5_catalog_runtime` and
  `m5_control_plane_runtime` for `intention-application`, and the ADR 0037 and evidence
  register rows for the session-selection direction anchor only the domain and client
  targets. The application-side session-selection evidence (defaults, resolution,
  promotion, reconciliation, held admission, usage) therefore has no declared anchor.
- **Observed evidence:** `quality/architecture.toml:189` lists
  `test_targets = ["m3_application", "m5_catalog_runtime", "m5_control_plane_runtime", "m5_session_selection"]`
  (added by this PR) versus `architecture/12` line 276, which names only
  `m5_catalog_runtime` and `m5_control_plane_runtime`. ADR 0037 line 159 and
  evidence-register EVD-056 cite `m5_session_selection_overrides.rs` (domain) and
  `session_selection_client.rs` (client) only.
- **Consequence:** The declared evidence chain does not point at the largest test target of
  the slice, so a reviewer cannot verify the session-selection acceptance outcomes from the
  normative documents, and the machine policy and prose disagree (the class AGENTS.md
  requires to be updated in the same change).
- **Remediation:** Add `m5_session_selection` to the `architecture/12` Slice 2 test-target
  list and to the ADR 0037 session-selection and control-plane evidence rows (or extend the
  existing EVD rows) so the machine policy, the ADRs, and the evidence register agree.
- **Verification:** `make docs-check` and a manual comparison of
  `quality/architecture.toml` test-target lists against the architecture enumerations; the
  known list of targets should match exactly in both directions.

### P3-20. Health evidence fabricates a `health-profile-<hex>` revision identity

**Accepted decision:** D-09, option 2 as the primary choice (see Appendix G.9): the fabricated
revision disappears and the field becomes optional and absent until the catalog is genuinely
wired into the health path.

- **Priority:** P3
- **Area:** Application services
- **Category:** generalizability (placeholder with the shape of a real identity)
- **Location:** `crates/intention-application/src/provider_control_plane.rs:499`, helper at `:721`
- **Confidence:** medium
- **Defect:** `ProviderHealthService` fills the evidence field
  `provider_profile_revision_id` with a value derived from the provider id
  (`health-profile-<16 hex>`), not with a real catalog profile revision. Any consumer that
  correlates health evidence with catalog or selection identities receives a value that can
  never match a profile revision, and the next case (a real profile) requires a new code
  path rather than reading the catalog binding.
- **Observed evidence:** `provider_control_plane.rs:499`
  (`provider_profile_revision_id: deterministic_profile_revision(&provider_id)`); the
  helper at `:721` hashes the provider id and prefixes `health-profile-`; the doc comment
  admits it is a placeholder "until the catalog is wired into the health path". The client
  fixture mirrors the fabricated value
  (`crates/intention-client/tests/control_plane_client.rs:250`).
- **Consequence:** Health evidence carries an identity-shaped value that is not an
  identity, so correlation, audit, and any future health-driven removal decision read a
  value they cannot join; a placeholder in a durable-facing projection is also exactly the
  kind of ad-hoc accommodation the review criteria reject.
- **Remediation:** Resolve the profile revision from the catalog binding (a
  `SafeBindingSource`-style read already exists in this module), or make the field optional
  or absent and document the placeholder in the protocol DTO. Do not emit a value with the
  shape of an identity it is not.
- **Verification:** Add a test asserting the health evidence revision equals the active
  catalog revision for the provider (or that the field is absent), and update the client
  fixture. Run `cargo nextest run -p intention-application -p intention-client --tests` and
  `make quick`.

### P3-21. Removal evidence is hand-rolled JSON crossing the storage boundary

**Accepted decision:** D-07, option A, full replacement (see Appendix G.7): the hand-rolled
writer and the span-slicing reader are deleted in favour of the typed removal-evidence record.
Shared with `P2-10`.

- **Priority:** P3
- **Area:** Application services
- **Category:** dto (second untyped codec between layers)
- **Location:** `crates/intention-application/src/provider_catalog.rs:1586` (`removal_candidate_json`), reader at `crates/intention-storage-sqlite/src/control_plane.rs:3091`
- **Confidence:** medium
- **Defect:** The application builds a JSON document with its own escaping
  (`removal_candidate_json` / `json_string`) and writes it into the raw
  `candidate_json: String` storage field, which storage then hand-parses with a span
  scanner. That is a second, untyped codec between two layers instead of a typed DTO or
  canonical codec, and the two halves already disagree: the writer escapes `"` and `\`,
  while the reader slices the quoted span without unescaping, so any escaped identifier
  decodes differently. The divergence is latent today only because provider kinds are a
  closed set and profile ids are generated.
- **Observed evidence:** `provider_catalog.rs:1586` (`removal_candidate_json(...)`), `:1607`
  (`json_string` escapes only quote and backslash), and
  `crates/intention-storage-sqlite/src/control_plane.rs:3091`
  (`identities.push(item[1..item.len() - 1].to_owned())`, no unescape) fed by
  `decode_removal_evidence`.
- **Consequence:** Two hand-written halves of one format must stay in sync without a type
  or a shared implementation; a future identifier containing a quote or backslash silently
  corrupts removal evidence, and neither half has compile-time protection. `P2-11` and
  `P3-15` record the decoder defect and the missing regression test.
- **Remediation:** Replace the raw JSON string with a typed removal-evidence DTO (a vector
  of removed ids plus the revision) shared by application and storage, or at minimum share
  one escaping implementation and unescape on read. The typed DTO also resolves `P2-10` for
  this field.
- **Verification:** A round-trip test with an escaped identity through the shared codec
  (the same fixture as `P3-15`) plus a boundary test asserting the storage DTO no longer
  exposes a free-form JSON string. Run
  `cargo nextest run -p intention-application -p intention-storage-sqlite --tests` and
  `make quick`.

### P3-22. Third selection-digest implementation with no production consumer

- **Priority:** P3
- **Area:** Application services
- **Category:** expediency (duplicate identity computation)
- **Location:** `crates/intention-application/src/provider_catalog.rs:1463` (`admission_dto`)
- **Confidence:** medium
- **Defect:** `admission_dto` populates `ProviderAdmissionDto.selection_digest` with a
  controller-local format-string digest (`ir-selection-v1|...` over fourteen fields) that
  is neither the domain canonical `provider_selection_digest` nor the storage
  `record_digest`. It additionally folds in `selection_source`, which the domain documents
  as provenance outside the execution digest. No production caller reads the field, so it
  is a divergent second source of meaning kept alive only by a length assertion.
- **Observed evidence:** `provider_catalog.rs:1458` and `:1463`; the consumers of
  `registry_lookup` (`crates/intention/src/lib.rs:361` and `:405`) discard the returned DTO.
  The only assertions on `selection_digest` are
  `crates/intention-application/tests/m5_catalog_runtime.rs:1960` (`len() == 64`) and the
  unit test at `provider_catalog.rs:1746`. `intention-domain` owns
  `provider_selection_digest` (`crates/intention-domain/src/provider_selection.rs:647`) and
  documents that the selection source is not identity-bearing.
- **Consequence:** One more place where identity can drift (see `P1-01`), and a digest that
  includes non-identity provenance can produce different values for logically identical
  selections, defeating the purpose of a canonical digest.
- **Remediation:** Remove the field and the local digest function, or compute it from the
  domain canonical digest and give it a real consumer. Resolve together with `P1-01` so one
  identity computation remains.
- **Verification:** After the change, a grep guard shows one canonical digest producer; if
  kept, a test asserts the admission digest equals the canonical domain digest for the same
  record. Run `cargo nextest run -p intention-application -p intention-domain --tests` and
  `make quick`.

### P3-23. Non-authority tests assert `Debug` substrings, not behaviour

**Remediation home:** Package F, item F2 (Appendix D), cross-referenced from D-09 (Appendix G.9). The strengthened non-authority assertions belong to the evidence-hardening package, and D-09 requires the EVD-051 wording to follow them.

- **Priority:** P3
- **Area:** Application services
- **Category:** test-gap (the roadmap non-authority property is not pinned)
- **Location:** `crates/intention-application/tests/m5_control_plane_runtime.rs:473` (health), `:503` (discovery), `:558` (pricing)
- **Confidence:** medium
- **Defect:** The health, discovery, and pricing non-authority fixtures assert that
  `format!("{projection:?}")` does not contain literals such as `"run_id"`, `"selection"`,
  or `"mandate"`. That passes for any field whose name is not in the literal list (for
  example `active_run`, `candidate`, `routing`) and asserts nothing about model-name
  routing or fallback behaviour, so the roadmap and ADR 0037 non-authority requirement is
  not actually pinned by these tests.
- **Observed evidence:** `tests/m5_control_plane_runtime.rs:483-490` (health), `:503-509`
  (discovery), `:558-572` (pricing) all loop over forbidden `Debug` substrings. No test
  asserts that the services hold no authority-bearing port, and no test asserts that a
  model name never influences routing, although evidence-register EVD-051 claims "no
  model-name routing, no fallback".
- **Consequence:** The strongest acceptance property of Slice 2 (health, discovery, and
  pricing create no authority) is guarded by a substring check that a future field rename
  or a new authority-bearing field would evade, while the evidence register records the
  property as verified.
- **Remediation:** Assert the property structurally (the services expose no storage,
  runtime, or catalog dependency; the projection types contain no authority field, for
  example via a compile-time exhaustive destructuring in the test) and add a routing and
  fallback fixture that would fail if health or discovery output ever influenced
  admission.
- **Verification:** The new structural assertions and the routing fixture must fail when a
  deliberate authority field or dependency is added (verify by temporary mutation during
  development). Re-run `cargo nextest run -p intention-application --tests` and
  `make quick`, and confirm EVD-051's wording matches the strengthened evidence.

### P3-24. Startup documentation claims all failures degrade, but storage errors propagate

- **Priority:** P3
- **Area:** Application services
- **Category:** docs and error-contract mismatch
- **Location:** `crates/intention-application/src/provider_catalog.rs:271` (and `:281`), documentation at `:258-259`
- **Confidence:** medium
- **Defect:** The startup contract says it returns an unavailable error only when the gate
  is poisoned and that all catalog failures degrade to a typed readiness state, yet several
  paths propagate raw storage errors (pending-removal row read, expiry, roll-forward
  acceptance, registry activation). A transient storage failure therefore aborts startup
  with an unrelated error instead of leaving a typed degraded readiness, contradicting both
  the doc and the degraded-mode design that callers rely on.
- **Observed evidence:** Doc at `provider_catalog.rs:258-259`;
  `self.removal.load_pending_removal_candidate()?` at `:271` and `self.expire_pending(now)?`
  at `:281`, plus `self.activate_registry(built, admissions)?` in the success paths. Only
  the catalog-status load maps errors through `blocked(...)`.
- **Consequence:** The documented recovery contract is wrong: callers cannot rely on
  startup always yielding a typed readiness value, so a transient storage failure surfaces
  as an arbitrary error code, and operators lose the degraded-mode signal.
- **Remediation:** Route these fallible calls through `blocked(error.code())` (or an
  equivalent typed degradation) as the documentation promises, or narrow the doc comment to
  the exact error contract and state which failures abort startup.
- **Verification:** Inject a storage failure at the pending-removal load and at expiry and
  assert either the typed degraded readiness or the documented abort. Update the doc in the
  same change. Run `cargo nextest run -p intention-application --tests` and `make quick`.

### P3-25. Held-run lookup error is treated as "not held" and auto-schedules

**Remediation home:** Package E, item E2 (Appendix D). Fail-closed correction; no decision required.

- **Priority:** P3
- **Area:** Composition and daemon
- **Category:** correctness (fail-open on a storage read error)
- **Location:** `crates/intention-daemon/src/lib.rs:166`
- **Confidence:** medium
- **Defect:** A failed held-recovery lookup is mapped to `false`, so a transient storage read
  error makes the host treat a held run as schedulable, contradicting the documented
  invariant that held recovered runs are never auto-scheduled. When the schedule step then
  also fails, `fail_unadmitted_starting_run` terminalizes the run as `Failed`, so the run is
  lost instead of staying held for explicit admission.
- **Observed evidence:** `crates/intention-daemon/src/lib.rs:163-170`
  (`is_recovered_run_held_for_daemon(session_id, run_id).unwrap_or(false)`); the failure
  path drops the registry lock and calls `fail_unadmitted_starting_run`
  (`:179-184`), which calls `fail_starting_run_for_daemon` with
  `model_scheduling_unavailable`.
- **Consequence:** A storage hiccup can permanently fail a run that was explicitly held for
  operator admission, and the held marker loses its fail-closed meaning. The failure is
  indistinguishable from an ordinary scheduling failure in the logs.
- **Remediation:** Treat a lookup error as held (return without scheduling) or propagate it,
  so the held marker is bypassed only on a positive `false`. Add a fixture that injects a
  held-lookup failure and asserts the run stays held.
- **Verification:** The new fixture must show the run remains held and not terminal after a
  lookup error; run `cargo nextest run -p intention-daemon --tests` and `make quick`.

### P3-26. Tool decoder re-lists wire names instead of using the typed `ToolId` or registry

**Remediation home:** Package E, item E3 (Appendix D). Removes the second authority for tool names; no decision required.

- **Priority:** P3
- **Area:** Composition and daemon
- **Category:** generalizability (duplicate name authority)
- **Location:** `crates/intention-daemon/src/lib.rs:895`
- **Confidence:** high
- **Defect:** The provider-facing decoder matches the six tool names as string literals,
  duplicating the name authority in `intention_tools::ToolId::as_str()` and the advertised
  set from `model_visible_descriptors()`. Any registry change (a renamed tool or a newly
  model-visible descriptor such as `fetch_url`) silently makes the loop advertise a tool
  the daemon rejects with `unknown_tool`, and the next tool needs a new branch in a second
  place.
- **Observed evidence:** `crates/intention-daemon/src/lib.rs:895-915`
  (`match tool_id { "read" => ..., "glob" => ..., "grep" => ..., _ => unknown_tool }`);
  `crates/intention-tools/src/lib.rs:656-690` defines the typed `ToolId` with `as_str()`;
  `:1094` (`model_visible_descriptors()`) drives what the loop advertises, asserted by
  `m5_tool_loop_wiring.rs`.
- **Consequence:** Advertisement and decoding can disagree, producing a live-tool-loop
  failure that only appears when the registry changes, and the fix requires edits in two
  places. This is the exact ad-hoc branch the review criteria call out.
- **Remediation:** Parse the provider tool name into `intention_tools::ToolId` (single name
  authority) and match on the typed value, or carry a per-descriptor decode function in the
  registry so the decodable set is derived from the advertised set.
- **Verification:** Add a test that enumerates `model_visible_descriptors()` and asserts each
  descriptor name is decodable by the daemon (a round-trip through the decoder), which fails
  whenever the two lists diverge. Run
  `cargo nextest run -p intention-daemon -p intention-tools --tests` and `make quick`.

### P3-27. Option seam selects the adapter builder by string kind in two places

**Remediation home:** Package E, item E7 (Appendix D), executed with D-02 (Appendix G.2, item 7). The typed-kind dispatch is what makes the driver-kind equals catalog-kind invariant checkable.

- **Priority:** P3
- **Area:** Composition and daemon
- **Category:** generalizability (kind-to-adapter mapping duplicated)
- **Location:** `crates/intention/src/lib.rs:307`, duplicated at `:1046-1051`, kind stored at `:126-130`
- **Confidence:** high
- **Defect:** The catalog factory and the credential-rebuild port both pick the adapter
  option builder by comparing the kind to the literal `"openrouter"` and default to the
  generic-chat builder otherwise, duplicating the kind-to-adapter mapping that
  `SelectedProvider::build_with_declared_options` already performs on the typed
  `ProviderKindDto`. A third provider must be added in three places, and forgetting one
  silently preflights a new kind through the wrong adapter instead of failing closed.
- **Observed evidence:** `crates/intention/src/lib.rs:307-312`
  (`let preflight = if self.kind == "openrouter" { declared.into_openrouter() } else { declared.into_generic_chat() }`);
  `:1046-1051` repeats the branch on `driver_kind == Some("openrouter")`; `:126-130` stores
  the kind as a `String` while the typed `ProviderKindDto` exists (`intention-config`) and is
  used at `:643-651`.
- **Consequence:** The else branch routes an unknown kind to the generic-chat adapter, so a
  misdeclared or newly added provider is silently handled by the wrong implementation and
  fails later with a misleading error.
- **Remediation:** Store the typed kind (or the adapter option-builder function) in
  `CompositionDriverFactory` and dispatch on `ProviderKindDto`, returning a typed
  unsupported-kind error instead of falling through to the generic-chat builder.
- **Verification:** Add a test with an unknown kind asserting the typed unsupported-kind
  error, and a test asserting the factory and the rebuild port produce the same adapter
  for the same declaration. Run `cargo nextest run -p intention --tests` and `make quick`.

### P3-28. Typed-edit TOML rendering lives in the composition and interpolates values

**Accepted decision:** D-10, options (1a) plus (2) plus (3) (see Appendix G.10): the document
is rendered from the safe snapshot as a TOML AST inside `intention-config`, the server-side
round trip is kept, and the composition no longer renders TOML.

- **Priority:** P3
- **Area:** Composition and daemon
- **Category:** expediency (configuration shape duplicated outside its owner)
- **Location:** `crates/intention/src/lib.rs:1185-1249`
- **Confidence:** medium
- **Defect:** The composition reconstructs the configuration document from typed edits with
  `format!`-interpolated TOML, duplicating the configuration shape (key-path list, schema
  literal, quoting) that the ownership map assigns to `intention-config`, whose sibling
  helper `restore_credential_document` uses the TOML serializer instead. A wire value
  carrying a quote or a trailing backslash produces an unparseable document, so the operator
  sees a generic rejected transaction (`invalid_config_toml` /
  `missing_provider_credential`) rather than a typed edit error, and any future
  configuration field is silently dropped from the rebuilt document.
- **Observed evidence:** `crates/intention/src/lib.rs:1185-1249` contains
  `kind = "{kind}"`, `model = "{model}"`, `endpoint = "{endpoint}"` and a hard-coded
  `schema_version = 1`. `crates/intention-config/src/control_plane.rs:741`
  (`restore_credential_document`) inserts values as `toml::Value` and serializes.
  `docs/intention-relay/reconciliation/ownership-and-dependency-map.md:54` assigns
  "Configuration reload and editing" to `intention-config`.
- **Consequence:** Two implementations of the configuration shape exist, one of them in the
  wrong crate; value escaping is unsafe; new fields are dropped silently; and the ownership
  map no longer matches the code.
- **Remediation:** Move the typed-edit document construction into `intention-config` and
  build the candidate through the TOML serializer (mirroring `restore_credential_document`),
  returning a typed edit error for values the configuration shape cannot represent.
- **Verification:** Add tests with values containing quotes and backslashes asserting either
  a successful edit with correct escaping or a typed edit error (no generic parse failure),
  and a test that a newly added configuration field survives a typed edit. Run
  `cargo nextest run -p intention-config -p intention --tests` and `make quick`.

### P3-29. Public activation method has one in-crate caller and takes raw credential text

**Remediation home:** Package E, item E6 (Appendix D), executed with D-02 (Appendix G.2, item 6). Narrowing the method is part of the startup-activation change.

- **Priority:** P3
- **Area:** Composition and daemon
- **Category:** expediency (public surface wider than needed, credential-bearing parameter)
- **Location:** `crates/intention/src/lib.rs:1443` (`pub fn activate_startup_catalog`)
- **Confidence:** high
- **Defect:** `activate_startup_catalog` is a public facade method with exactly one caller
  (`open_platform`) and no other consumer in the workspace, yet it widens the public surface
  with a parameter carrying raw configuration text that includes the credential. The daemon
  host does not call it (`run()` only calls `open_platform` and
  `provider_control_startup`), so the visibility is unnecessary and invites a second
  credential-bearing entry point.
- **Observed evidence:** `crates/intention/src/lib.rs:1443`
  (`pub fn activate_startup_catalog(&self, raw_toml: &str)`); a workspace-wide grep finds
  only `crates/intention/src/lib.rs:1425` calling it. `crates/intention-daemon/src/lib.rs:975-981`
  calls `open_platform` and `provider_control_startup` only.
- **Consequence:** The public API surface grows with a credential-bearing method whose
  contract no consumer needs, increasing the chance that a future caller passes raw text
  across a boundary the redaction design intended to keep inside the loader, and it
  contradicts the minimum-surface principle.
- **Remediation:** Make it a private helper of `open_platform`, or change the parameter to
  the already-parsed `StartupProviderMaterial` or declaration so raw credential text stays
  inside the private loading boundary.
- **Verification:** A compile-time check (the method is no longer public) plus a grep guard;
  the existing composition tests for startup activation must keep passing. Run
  `cargo nextest run -p intention --tests` and `make quick`.

### P3-30. Reasoning failure branch is unreachable and its test is vacuous

- **Priority:** P3
- **Area:** Model and provider adapters
- **Category:** dead-code (impossible failure path) plus misleading test
- **Location:** `crates/intention-provider-generic-chat/src/lib.rs:551` (the `Err(_) => self.fail(...)` arm), guard at `:533-556`
- **Confidence:** high
- **Defect:** `accept_reasoning` returns early for empty text, and
  `ModelEventDto::reasoning_delta_categorized` fails only on empty text, so the
  `Err(_) => self.fail("generic_chat_reasoning_stream_invalid")` branch can never run. The
  adapter therefore advertises a closed reasoning failure it cannot produce, and the test
  that claims to cover reasoning failures calls `state.fail("provider_reasoning_stream_invalid")`
  directly instead of exercising the decoder.
- **Observed evidence:** `generic-chat/src/lib.rs:533-556`
  (`if reasoning.is_empty() { ... return Ok(()) }` followed by the match) versus
  `crates/intention-model/src/lib.rs` `reasoning_delta_categorized`, whose only error is
  `invalid_model_reasoning_delta` for empty content. The test
  `normalized_reasoning_failures_never_carry_raw_provider_text` fabricates the error through
  `state.fail(...)`.
- **Consequence:** The adapter claims a failure surface that cannot occur, and the test
  asserts behaviour it never drives, which hides the fact that no code path actually
  produces that error code. A future change that makes the branch reachable would be
  untested.
- **Remediation:** Push the event directly and delete the impossible branch, or make the
  branch reachable by validating something real; align the code name with the closed
  `provider_reasoning_stream_invalid` if a failure is genuinely needed. Drop the
  fabricated-error test or drive it through the decoder.
- **Verification:** After the change, a coverage or mutation check shows no unreachable arm;
  if a real failure path is introduced, add a decoder-driven test that produces it. Run
  `cargo nextest run -p intention-provider-generic-chat --tests` and `make quick`.

### P3-31. Private wire reuses the closed `FinishReason`; unknown reasons abort the response

- **Priority:** P3
- **Area:** Model and provider adapters
- **Category:** generalizability (unknown provider value aborts instead of degrading)
- **Location:** `crates/intention-provider-generic-chat/src/wire.rs:75`
- **Confidence:** medium
- **Defect:** The new private BYOT chunk type keeps the SDK's closed `FinishReason` enum for
  `finish_reason`, so any gateway reason outside the five known values (for example
  `stop_sequence`, `max_tokens`, or a vendor-specific reason) fails chunk deserialization
  and aborts the entire response with a non-retryable error, even though
  `FinishReasonDto::Unknown` and the `map_finish_reason` `_ => Unknown` mapping exist for
  exactly that case.
- **Observed evidence:** `generic-chat/src/wire.rs:75`
  (`pub finish_reason: Option<FinishReason>`); `async-openai` 0.42 `FinishReason` has no
  `#[serde(other)]` fallback; `map_finish_reason` in `lib.rs` already maps unknown strings
  to `FinishReasonDto::Unknown` but is reachable only from the `map_fixture_finish` test
  helper.
- **Consequence:** A valid provider response with a new finish reason is dropped, producing
  a user-visible failure with a credential-free but misleading non-retryable code, and the
  next gateway needs a code change instead of degrading to `Unknown`.
- **Remediation:** Declare the wire field as `Option<String>` (or a private open enum) and
  map unknown values to `FinishReasonDto::Unknown`, keeping the `tool_calls` and
  `function_call` mapping policy intact.
- **Verification:** Add a chunk-decoder test with an unlisted finish reason (for example
  `stop_sequence`) asserting the response completes with `Unknown` rather than an error, and
  a test that the known reasons still map as before. Run
  `cargo nextest run -p intention-provider-generic-chat --tests` and `make quick`.

### P3-32. Attachment validation can abort a run that the durable path accepted

**Accepted decision:** D-15, option (a) (see Appendix G.11.5): the accumulated reasoning echo is
bounded per round and an unrepresentable attachment becomes a durable typed failed run, with the
per-round bound recorded in ADR 0041.

**Remediation home:** Package C → D-15 (a). Truncation and raising the attachment bound were
considered and rejected; see the brief for the recorded reasons.

- **Priority:** P3
- **Area:** Model and provider adapters (runtime)
- **Category:** correctness (unbounded accumulation, error type outside the durable taxonomy)
- **Location:** `crates/intention-runtime/src/lib.rs:1346` (`round_reasoning_attachment`), call sites at `:900` and `:997`
- **Confidence:** medium
- **Defect:** The round's reasoning is accumulated without a cap and validated only at round
  end by `AssistantReasoningDto::new` (512 KiB per attachment, no control characters other
  than `\n`, `\r`, `\t`). The same text was already accepted as durable reasoning facts
  (512 KiB per fact, 4 MiB per run) and by the domain's `BoundedText` (NUL only), so a long
  thinking round or one stray control character makes the continuation unrepresentable. The
  `?` then propagates out of `drive_provider_round` / `drive_attempt`, `execute` returns a
  validation error, and the run is only terminalized by the daemon's outer error path with a
  DTO validation code instead of a typed provider failure fact.
- **Observed evidence:** `crates/intention-runtime/src/lib.rs:1337-1347`
  (`round_reasoning_attachment` plus `?` at the two `RoundOutcome::ToolCalls` sites, lines
  900 and 997 in `drive_provider_round`); `crates/intention-model/src/lib.rs:238-243`
  rejects over 512 KiB or control characters; `crates/intention-tools/src/lib.rs`
  `BoundedText::new` rejects only over 1 MiB or NUL; the 4 MiB per-run bound is enforced
  later at the append authority, so 512 KiB to 4 MiB of reasoning is durable but
  un-attachable.
- **Consequence:** A run that the durable path accepted can fail with a validation error
  that is not in the typed provider failure taxonomy, producing a confusing terminal state
  and inconsistent evidence between durable reasoning facts and the run's failure record.
- **Remediation:** Bound the accumulated attachment text and convert an unrepresentable
  attachment into a durable typed failure (for example `RoundOutcome::Failed` with a
  dedicated code) instead of returning `Err` from `execute`. State the per-round echo bound
  explicitly in ADR 0041 so the two bounds are documented together.
- **Verification:** Add a test with a reasoning fragment that passes the durable bound but
  fails the attachment bound, asserting a typed failed run fact (not an `execute` error),
  plus a test for a control character. Run
  `cargo nextest run -p intention-runtime -p intention-model --tests` and `make quick`;
  update ADR 0041 in the same change.

### P3-33. Harness attempt budget exceeds the workflow step and job timeouts

**Remediation home:** Package F, item F3 (Appendix D). Harness arithmetic only; can ship immediately and must be followed by a live run and an EVD-063 update.

- **Priority:** P3
- **Area:** Tooling, live proof and quality policy
- **Category:** test-gap (the documented bound is unreachable inside its own CI wiring)
- **Location:** `crates/intention-daemon/tests/real_api_e2e.rs:88` (`TURN_DEADLINE`), with `:100` (`TOOL_TURN_ATTEMPTS`), `:922` and `:1310` (per-attempt deadlines), and
  `.github/workflows/real-api-e2e.yml:43` (job) and `:91` (step)
- **Confidence:** high
- **Defect:** Six positive turns plus the negative test, each up to `TOOL_TURN_ATTEMPTS`
  (3) attempts of `TURN_DEADLINE` (180 s), plus two `READINESS_DEADLINE` (30 s) daemon
  starts, is about 63 minutes of worst case, while the workflow gives the run step 30
  minutes and the job 45. A fully degraded live run therefore reports an opaque GitHub
  timeout instead of the harness's own diagnostic ("did not record a succeeded `<tool>`
  call within 3 turns"), which defeats the point of bounding attempts.
- **Observed evidence:** `real_api_e2e.rs:88` (`TURN_DEADLINE = 180s`), `:94`
  (`REPLAY_QUIET_WINDOW`), `:100` (`TOOL_TURN_ATTEMPTS = 3`), `:922` (per-attempt deadline),
  `:1310` (the same for the negative test). `.github/workflows/real-api-e2e.yml:43`
  (`timeout-minutes: 45`), `:91` (`timeout-minutes: 30`).
- **Consequence:** The failure diagnostic that the bounded budget exists to produce can
  never be seen in CI; instead the job dies with a timeout, the run report is truncated, and
  the operator cannot tell which turn and attempt were in flight.
- **Remediation:** Either lower `TURN_DEADLINE` and the attempt counts so the worst case fits
  the step budget with margin, or raise the step and job timeouts to cover seven turns times
  three attempts times `TURN_DEADLINE` plus readiness and replay windows. State the chosen
  arithmetic next to the constants so a future change keeps them consistent.
- **Verification:** Add the arithmetic as a comment next to the constants and, ideally, a
  self-test that reads the workflow YAML and asserts the configured timeouts cover
  `turns * attempts * TURN_DEADLINE + slack`. Run `make e2e-real-api` once after the change
  and record the run per ADR 0040.

### P3-34. State-bytes credential scan is vacuous on macOS

**Remediation home:** Package F, item F4 (Appendix D). Harness coverage only; can ship immediately and must be followed by a live run on the affected platform.

- **Priority:** P3
- **Area:** Tooling, live proof and quality policy
- **Category:** test-gap (the platform-specific claim is not actually checked)
- **Location:** `crates/intention-daemon/tests/real_api_e2e.rs:1277` (`assert_state_directory_excludes_credential`), spawn logic at `:368-371`
- **Confidence:** medium
- **Defect:** On macOS, `spawn_daemon` overrides only `HOME` (to `config_home`), and both the
  configuration path and the platform state directory derive from `HOME`, so the daemon's
  SQLite state lands under `config_home` while the separate `state_home` `TempDir` is never
  touched. `assert_state_directory_excludes_credential(state_home, credential)` then walks
  an empty directory and passes without inspecting the real durable bytes, so the "no state
  byte carries the credential" claim in ADR 0040 decision 6 and EVD-063 is enforced only on
  Linux and Windows.
- **Observed evidence:** `real_api_e2e.rs:368-371`
  (`#[cfg(target_os = "macos")] command.env("HOME", config_home);`, and the code comment
  concedes "Both the configuration and the state directory derive from HOME"), `:442-451`
  (the `state_home` `TempDir` is created and passed to the host), `:1277`
  (`assert_state_directory_excludes_credential(host.state_home.path(), &credential)`), and
  `crates/intention/src/lib.rs:2785-2790` (macOS state directory is
  `HOME/Library/Application Support/intention-relay`).
- **Consequence:** On macOS the credential-containment guarantee is unverified while the
  ADR and evidence register present it as covered, so a macOS-specific leak path (for
  example a cached config copy under the state directory) would go undetected.
- **Remediation:** Scan the directory that actually holds durable state on each platform
  (`config_home` on macOS, `state_home` elsewhere), or assert that the resolved state
  directory is non-empty before scanning so an empty target fails loudly instead of passing
  vacuously. Consider asserting the resolved path through the same resolution function the
  daemon uses.
- **Verification:** A unit test asserting the resolved state directory for each target OS,
  plus a non-empty assertion inside the scan helper; the live run then proves the scan
  covers real bytes. Run `make e2e-real-api` on a macOS host if available and record it.

### P3-35. "No raw provider text" assertion is a tautology

**Remediation home:** Package F, item F5 (Appendix D). Replaces an assertion that cannot fail with the positive invariant.

- **Priority:** P3
- **Area:** Tooling, live proof and quality policy
- **Category:** test-gap (assertion cannot fail)
- **Location:** `crates/intention-daemon/tests/real_api_e2e.rs:1351`
- **Confidence:** high
- **Defect:** The negative case claims to prove that durable facts never carry raw provider
  text, but the check is `!facts_json.contains("\"message\"")` and no
  `ModelRunFactInputDto` variant has a message field, so the assertion can never fail
  regardless of what a provider returns. The claim in the test's own doc comment and in
  ADR 0040 decision 7 is therefore unverified by this assertion.
- **Observed evidence:** `real_api_e2e.rs:1349-1353`;
  `crates/intention-domain/src/model_facts.rs:282-313` (variants carry attempt, failure,
  content, usage, call, `call_id`, outcome, reason only, with no message field); the failure
  text is a closed code through `RunFailureDto`.
- **Consequence:** The credential and provider-text containment claim rests on an assertion
  that is structurally unable to fail, so a regression that embedded provider text in a
  durable fact (for example a new field or a changed failure mapping) would not be caught by
  the live channel.
- **Remediation:** Assert the positive invariant instead: every recorded failure code is in
  the normalized closed set, and every durable string field is the bounded normalized value
  (re-validate each fact through its DTO constructor), or assert that no fact content
  contains the provider's raw error phrase used by the fixture.
- **Verification:** Strengthen the assertion and prove it can fail by a temporary mutation
  (inject the provider phrase into one fact in a local run) before keeping it. Re-run
  `make e2e-real-api` and update EVD-063 if the run is recorded.

### P3-36. New coverage dedup rule is undocumented in policy and untested

- **Priority:** P3
- **Area:** Tooling, live proof and quality policy
- **Category:** docs and policy consistency (gate behaviour change without policy text or self-test)
- **Location:** `quality/run_coverage.py:133` (the `seen_effective` skip), with `quality/features.toml` profiles and `docs/intention-relay/architecture/12-quality-gates-and-makefile.md:150`
- **Confidence:** medium
- **Defect:** The added normalization makes every profile collapse to the same effective
  daemon flag tuple, so when `make coverage` runs all three profiles in one invocation, the
  no-default and all-features per-crate daemon reports are silently not produced (only the
  first, "default", report exists). That is a coverage-gate behaviour change carried only by
  an inline comment: the architecture text still says every required feature profile
  contributes to coverage where the crate supports that profile, no machine-readable policy
  records the equivalence, and `quality/self_test.py` has no fixture for the new branch, so
  nothing fails if the normalization later drops a genuinely distinct flag tuple.
- **Observed evidence:** `quality/run_coverage.py:130-156` (appends `--all-features`, then
  drops `--no-default-features` and `--features` tokens and skips on `seen_effective`).
  `quality/features.toml` declares `default = []`, `no_default = ["--no-default-features"]`,
  `all = ["--all-features"]`. `architecture/12` line 150 and `AGENTS.md` require
  machine policy and architecture documentation updates in the same change. A grep for
  `seen_effective` or equivalent in `quality/self_test.py` returns nothing.
- **Consequence:** A future feature-profile change can silently reduce coverage
  measurements (or stop producing a report operators expect) without any gate failing, and
  the machine-readable policy and the prose disagree about what coverage means.
- **Remediation:** State the daemon equivalence explicitly (a short paragraph in the
  architecture 12 coverage section, or a machine-readable note such as
  `quality/coverage.toml`) and add a `quality/self_test.py` fixture asserting that
  equivalent profiles are skipped while a genuinely different flag tuple still produces its
  own report.
- **Verification:** The new self-test must fail if the normalization is widened to drop a
  distinct tuple; run `python3 quality/self_test.py` (or the make self-test target) and
  `make verify` to confirm the coverage gate still passes with the documented behaviour.

### P3-37. Two live-harness waits are not deadline-bounded

**Remediation home:** Package F, item F6 (Appendix D). Bounds the remaining waits so every harness wait is deadline-bounded.

- **Priority:** P3
- **Area:** Tooling, live proof and quality policy
- **Category:** test-gap (unbounded wait defeats the harness diagnostic)
- **Location:** `crates/intention-daemon/tests/real_api_e2e.rs:1199` (post-restart replay subscribe), with the snapshot reads near `:1259-1270`
- **Confidence:** medium
- **Defect:** The fact-collection path is wrapped in a deadline, but the post-restart replay
  `RunStreamClient::subscribe(...).await.expect("restart replay arrives")` and the
  credential-check `client.session_snapshot(...)` calls are unbounded: the synchronous
  transport's `read_frame` uses a blocking `read_exact` with no read timeout, so a daemon
  that accepts the connection but never answers hangs the run until the CI step timeout
  instead of producing a harness diagnostic. The harness is the component that claims to be
  bounded.
- **Observed evidence:** `real_api_e2e.rs:1196-1200` (subscribe with no timeout wrapper) and
  the `session_snapshot` loop near `:1259-1270`;
  `crates/intention-transport/src/lib.rs:743-762` (`read_frame` has no read timeout); only
  `collect_terminal_run` (`:838-850`) uses `tokio::time::timeout_at`.
- **Consequence:** A stuck daemon turns an expected bounded failure into an opaque CI
  timeout, losing the harness diagnostic and the run report content, which is the same class
  as `P3-33` but caused by an unbounded wait rather than a budget arithmetic mismatch.
- **Remediation:** Wrap the replay subscribe and the snapshot reads in the same bounded
  deadline (`tokio::time::timeout_at` for the async path, and either a bounded retry around
  the synchronous path or a transport-level read timeout), so every wait in the harness is
  deadline-bounded.
- **Verification:** A grep-style guard that every wait in the harness goes through the
  bounded helper, plus a test that a non-answering peer produces the harness diagnostic
  rather than hanging (can be approximated with a test double). Re-run `make e2e-real-api`.

### P3-38. In-repo NOTE claims the session provider profile transaction rolls back; the code commits and the read-back assertion is missing

- **Priority:** P3 (rated by the consolidator; the composition reviewer raised the same text as a potentially PR-blocking uncertainty, which direct verification disproved)
- **Area:** State and configuration / tests
- **Category:** docs plus test-gap (stale defect note, missing end-to-end persistence assertion)
- **Location:** `crates/intention/src/lib.rs:5522` (the NOTE in the session-profile composition test), implementation at `crates/intention-storage-sqlite/src/control_plane.rs:2549-2650`
- **Confidence:** high (verified directly on 2026-09-24)
- **Defect:** The composition test carries a NOTE stating that "The zone-3 sqlite
  `set_session_provider_profile` transaction currently rolls back every write (no
  `tx.commit()` on any path), so an end-to-end read-back assertion here would fail against
  the live backend; the defect is reported to the storage zone." That statement is false in
  the current tree: the storage implementation commits on every path. The NOTE therefore
  documents a data-loss defect that does not exist and excuses the absence of the one
  assertion that would prove durable persistence.
- **Observed evidence:** Direct read of
  `crates/intention-storage-sqlite/src/control_plane.rs:2549-2650` on 2026-09-24 shows
  `tx.commit().map_err(storage_error)?` on all four paths: the insert path (after the
  `INSERT INTO session_provider_defaults`), the idempotent same-operation path (matching
  profile returns `changed: false`, differing profile returns the typed
  `session_provider_default_conflict`), the same-profile touch path (updates
  `last_operation_id` and `updated_at`), and the profile-change path (updates `profile_id`,
  increments `projection_revision`, returns `changed: true`). The `immediate_transaction!`
  macro (`crates/intention-storage-sqlite/src/lib.rs:557-567`) only opens an immediate
  transaction; the operation block commits explicitly. No `tx.rollback()` exists on any
  path.
- **Consequence:** The stale NOTE misleads reviewers and blocked a legitimate assertion
  (the composition reviewer flagged it as a possible PR-blocking defect before
  verification), and because no composition or facade test reads the row back, durable
  persistence of the session provider default, the idempotent same-operation no-op, and the
  stale-revision conflict are verified only at the storage layer, not through the delivered
  operation.
- **Remediation:** Delete or rewrite the NOTE to state the current behaviour, and add the
  missing end-to-end assertion: after `set_session_profile` through the facade, read the
  `session_provider_defaults` row back (through the storage repository or a reopened store)
  and assert `profile_id`, `projection_revision`, and `last_operation_id`. Cover the
  idempotent same-operation no-op and the stale-revision conflict in the same test. Search
  for any other comment repeating the alleged rollback defect and correct it.
- **Verification:** The new read-back test must pass against the live SQLite backend (it
  would have failed under the NOTE's claim), and a grep for "rolls back every write" must
  return no hit. Run `cargo nextest run -p intention --tests` and `make quick`.

## Appendix A. Verified sound (what each reviewer checked and found correct)

These lists record the properties each reviewer actively verified, so the findings above
can be read against the evidence that the rest of the area is healthy.

### A.1 Domain layer

- The numeric tag registry is single-sourced in `intention-domain::canonical::TagRegistry`
  (24 ledger entries, exactly 9 `Wired`) with `TagStatus` reduced to
  `Wired` / `ReservedForSlice3` / `ReservedForSlice4`; the Slice 2 test asserts no duplicate
  ledger values and that no family decoder accepts a reserved tag.
- Goldens are current-version only: all six new fixtures pin `record_version = 1` with
  byte-for-byte encoder equality and a SHA-256 of those bytes; `execution-meaning-v3.txt`
  and `RunExecutionMeaningV3Record` are gone with no dangling references, and the envelope
  goldens were repinned for the metadata-authenticating digest documented in ADR 0036:222
  and PR24-028.
- The wave 2 and wave 6 removals are clean: repository-wide greps for
  `ensure_compatible_with`, `LegacyM4SelectionBindingDto`,
  `legacy_m4_selection_bindings`, `SnapshotBindingSource`,
  `RunExecutionMeaningV3Record`, and `error-v1-legacy.json` return zero code references;
  `workspace_id` is mandatory on `SessionCreatedEventDto` (no serde default), and
  `ReasoningDeltaRecorded.category` has no serde default, so an uncategorized delta fails to
  decode.
- The rejection suites assert real fail-closed behaviour with exact `CanonicalError`
  variants for every newly wired family (missing, duplicate, descending, and unknown fields;
  wrong wire type; truncated payload; trailing bytes; wrong version; invalid UTF-8;
  closed-enum and bool scalar violations; over-limit list counts; non-canonical u64;
  nested-record tag mismatch; digest mismatch), plus stable error codes that never echo
  input values.
- Credential-shape policy is single-sourced: the four role functions live in `canonical.rs`
  and are called by `intention-config` (key-name and secret-value roles),
  `intention-protocol` (identifier and raw-content roles), and the storage tests, so
  verdicts cannot diverge.
- No secrets, POSIX-only literals, or provider-native shapes leak into canonical bytes:
  endpoint, kind, header, and credential checks run before encoding; safe labels and
  `selection_source` are excluded from identity digests; and test paths use
  `std::env::temp_dir()` / `PathBuf`.
- Kind immutability and removal rules are enforced and tested
  (`validate_provider_kind_revision_immutability`, `validate_provider_kind_removal`), and the
  catalog limits are frozen constants validated by equality with their literal values.

### A.2 Protocol and client surface

- Single live version: `ProtocolVersionDto::ensure_compatible_with` and
  `SchemaVersionDto::ensure_compatible_with` are deleted with no remaining code references,
  and `crates/intention-transport/src/lib.rs:681-691` requires exact equality of both peers
  with `CURRENT_PROTOCOL_VERSION` (1.1); no dual-version or same-major tolerance path
  remains.
- The removals are complete: the 1.0 wire fixtures, `tests/protocol_fixtures.rs`,
  `hello-compatible-minor-v1.json`, `LegacyM4SelectionBindingDto` / `0x020C`, and
  `ReloadTransactionDto.migration_result` have no dangling references;
  `quality/architecture.toml:284-302` lists the current protocol test targets (`contracts`,
  `accessors`, `m3_contracts`, `m4_run_stream_contracts`, `control_plane_contracts`) and
  drops `protocol_fixtures`; and the unknown-additive-field tolerance assertion is re-homed
  into `crates/intention-protocol/tests/contracts.rs:307`.
- The removed client types `SessionSubscriptionRecovery` and `SessionSubscriptionReducer`
  have no remaining references, and their tests were removed with them rather than left
  asserting removed behaviour.
- No untyped maps, `serde_json::Value`, provider SDK types, or hard-coded tool names appear
  in the protocol or client public surface, and no POSIX-literal paths are hard-coded in
  production code (fixtures use `std::env::temp_dir()`).
- Control-plane commands and queries are validated at daemon admission with typed errors
  (`crates/intention/src/lib.rs:2048-2166` for queries, `:2604-2667` for commands), and the
  client pre-validates with the same `validate()` functions, so validation is not UI-only.
- The 8-promotion bound on `ReconcileUnavailableQueueAcceptedDto` matches the application's
  `promote(max = MAX_UNAVAILABLE_QUEUE_PROMOTIONS = 8)`, so a legitimate reconciliation page
  cannot exceed it; `MAX_RECONCILIATION_BATCH = 32` governs storage reconciliation
  separately and is consistent with ADR 0037.
- Credential handling is consistent: every new DTO string field is checked with
  `credential_shaped`, negative fixtures assert `credentials_forbidden`, and no real secrets
  were found in the reviewed sources or fixtures.

### A.3 State and configuration

- Single live schema: `open()` executes one `execute_batch(SCHEMA_SQL + SCHEMA_M5_SQL)` with
  no `MIGRATIONS`, no `user_version` read or gate, no `unsupported_storage_schema` branch,
  and no `TEST_SCHEMA_3_SQL` export; `rusqlite_migration` is removed from `Cargo.toml` and
  the workspace graph, matching ADR 0038 wave 4 and the architecture 04 "created directly on
  open" text.
- `current_storage_schema_is_created_completely_and_remains_authoritative` asserts the exact
  29-table inventory (count equality, not just presence), all 15 explicit indexes, and the
  seeded catalog state row, so a leftover legacy table would fail the suite.
- Single TOML shape: `parse_resolve` has one `integer == 1` dispatch; unversioned and
  `[model]` / `api_key` documents fail closed with `invalid_config_schema`; `parse_credential`
  reads only `provider.credential`; and `require_current_schema_version` demands exact
  equality in `from_public_parts`, `ConfigSnapshotDto::new`, and `validate_for_persistence`.
  `migrate_v0`, `RawV0Config`, `collect_v0_issues`, `redacted_safe_digest`, and
  `CandidateAcceptanceOutcomeDto` are gone (verified by repository-wide grep).
- Config credential redaction is consistent: `RawConfigInputDto` and `StartupProviderMaterial`
  implement no `Debug` / `Display` / serde; `ProviderSelectionDto` exposes only
  `credential_configured`; `safe_debug_projection` omits the credential; and tests assert
  the fake credential is absent from errors, serialized candidate wire bytes, and every
  persisted JSON column (`fake_secret_sweep_covers_candidate_serialization_and_validation`,
  `persisted_m3_json_never_contains_the_fixture_credential`,
  `queue_usage_removal_tables_never_persist_fake_secrets` scans every text and JSON column of
  six control-plane tables).
- Transactional atomicity is real for the exercised paths: `immediate_transaction!` opens
  `TransactionBehavior::Immediate`, every write path commits through `tx.commit()` after
  building `CommittedChangeDto`, and the fault tests reopen the file (not the handle) before
  asserting row counts, so they genuinely prove rollback rather than in-memory state.
- Fault points are single-use (`arm_fault` / `fault` clears the armed value) and
  `location_is_absolute_and_faults_are_single_use` pins that behaviour, preventing a stale
  fault from leaking into a later operation in the same test.
- Constraint violations are typed where the PR added them deliberately: catalog digest and
  identity reuse returns `provider_kind_descriptor_digest_conflict` /
  `provider_profile_revision_conflict`; removal candidates return
  `provider_catalog_removal_candidate_conflict`, `provider_catalog_removal_pending_exists`,
  and `provider_catalog_removal_not_pending`; selection rebinding returns
  `provider_selection_conflict`; session defaults return `session_provider_default_stale` and
  `_conflict`; and catalog revision mismatch returns `provider_catalog_revision_conflict`.
- Controlled reload commits atomically (snapshot, audit, and state update in one transaction)
  and its two fault stages (`ReloadSnapshot`, `ReloadAudit`) are covered by a test that
  reopens the database, asserts neither the revision row nor the audit row exists, then
  proves the snapshot is still persistable; the state update deliberately preserves
  `pending_removal` and `activation_recovery_required` statuses.
- No hard-coded POSIX paths in production code: `SqliteDatabaseLocationDto` validates
  absoluteness via `Path::new(...).is_absolute()`, config resolves platform paths per
  `target_os`, and every fixture builds paths from `std::env::temp_dir()` / `TempDir`.
- No credential material is written to disk or into DTOs in this area: the storage DDL has no
  credential column; `insert_selection` and `insert_profile` persist only transport mode and
  the safe header name; and the queue, usage, and removal tables are covered by the
  credential-shape column sweep.
- The controlled-reload candidate layer does not mutate live state: `parse_candidate` builds a
  fresh `ConfigSnapshotDto` (or reuses the previous snapshot on failure), and the only
  durable mutation is `commit_configuration_reload`, which the application layer calls only
  after the candidate is accepted and the expected active revision matches.

### A.4 Application services

- Exactly one execution path per operation:
  `ApplicationService::send_user_turn_and_schedule_with_provider_selection` is the only
  scheduling path (no selection-less sibling); the legacy M4 selection bridge and
  `tests/m4_application_scheduling.rs` are removed; and the crate contains no feature flags,
  `cfg(feature)` branches, or fallback branches.
- Health, discovery, and pricing create no authority: `ProviderHealthService`,
  `ProviderDiscoveryService`, and `PricingPolicyService` are fieldless services whose only
  inputs are read-only probe or discovery ports and plain values; they hold no storage,
  runtime, catalog, or scheduler dependency and cannot produce a `RunId`, reason, lifecycle
  transition, selection, scheduler candidate, or reconciliation result.
- Reload is atomic and fail-closed: a stale expected revision is rejected before any write,
  storage failure propagates, and the outcome carries `fresh_runs_only = true`
  (`tests/m5_control_plane_runtime.rs:311` and `:357`).
- Rotation is redaction-safe and ordered: the frozen-meaning and completeness checks run
  before replacement material is obtained; failures occur before or at the replacement
  boundary; nothing is retried or resumed; and `PrivateCredentialMaterial` is
  non-`Debug`, non-serde, and never crosses a DTO (tests at
  `m5_control_plane_runtime.rs:403`, `:450`, and the `provider_control_plane.rs` unit tests).
- Catalog startup is all-or-nothing on the read and build path: the replacement registry is
  fully built before activation, and a corrupt material or unavailable factory degrades to
  `Blocked` with no partial registry (`tests/m5_catalog_runtime.rs:930` and `:1006`).
- Unavailable-queue promotion is bounded and idempotent: the service passes the closed
  eight-item batch (`MAX_UNAVAILABLE_QUEUE_PROMOTIONS`), storage rejects `max > 8`, and
  promotion selects only `state = 'queued'` rows and marks them promoted, so repeats promote
  nothing twice and exhaustion writes the typed reconciliation marker.
- The control-plane gate serializes acceptance and admission transitions and never mutates
  existing runs or their persisted selections (gate state is only read or replaced wholesale
  under the lock).

### A.5 Composition, daemon and adapters

- Credential containment: `PrivateCredentialState` implements no `Debug` / `Display` / serde;
  `CompositionCredentialSource` and `CompositionDriverRebuildPort` map every read,
  permission, parse, and UTF-8 failure to `credential_rotation_source_unavailable` without
  echoing content; the replacement is committed only after the driver swap succeeds; and the
  composition tests sweep transaction, outcome, and active-snapshot output for the secret
  (`crates/intention/src/lib.rs:97-115`, `:965-1070`, `:5201-5310`).
- The degraded gate fails closed and matches the application policy exactly:
  `control_plane_serving` serves only `Ready`, `Uninitialized`, and `Loading`, the same three
  states `DegradedModeService::assert_execution_ready` accepts
  (`crates/intention-application/src/session_selection.rs:257-272`);
  `provider_control_readiness` falls back to `Blocked` on a poisoned gate; and the capability
  rejection precedes the readiness rejection (`crates/intention-daemon/src/lib.rs:1114-1130`,
  `:1202-1220`; `crates/intention/src/lib.rs:1940-1950`).
- The startup path is panic-free: the only `unreachable!` / `unwrap_or_else` sites in the
  composition's production code are pre-existing (session subscribe) or total (u64
  conversions, zero timestamp); `open_platform` and `open_with_selected_provider` propagate
  typed errors only; and `recover_before_ready` runs before `provider_control_startup`
  (`crates/intention/src/lib.rs:1413-1425`, `:1836-1855`).
- Held-run admission and promotion are bounded and idempotent: promotion is storage-bounded
  to eight per terminal transition and each call site runs once per terminal transition; the
  hold and enqueue operation ids are deterministic per `(session, run)`; and the fixture
  proves a repeated `AdmitRecoveredRun` dispatches exactly once while a stale persisted
  selection stays held with `held_run_admission_verification_failed`
  (`crates/intention/src/lib.rs:1975-2015`, `:5753-5880`).
- No lock-order inversion among the daemon's data, publication, and command gates: the facade
  never touches host state, so `stop_run` holding the registry while taking the facade
  command gate cannot cycle; the terminalizer releases the registry before retrying; and the
  publication-retry worker set is inserted and removed symmetrically
  (`crates/intention-daemon/src/lib.rs:264-300`, `:316-345`, `:560-600`).
- Transport reclaim is Unix-only and fails closed elsewhere: the non-Unix stub returns false
  so a Windows named-pipe conflict still yields `local_daemon_endpoint_in_use`;
  `symlink_metadata` prevents following a symlink; and non-socket paths are never removed
  (`crates/intention-transport/src/lib.rs:864-897`, with tests in `transport_integration.rs`).
- Single-execution-path cleanup is complete in this area: no leftover references to the
  removed post-commit publisher, M4 selection bridge, `WorkspaceRoot::resolve_path_for_tool`,
  optional `ProtocolAcceptedDto::result()`, or the `intention-client` dependency in the
  composition crate; hello negotiation now requires the exact current protocol version on
  both peers.

### A.6 Model and provider adapters

- Adapter isolation: no provider SDK type appears in a public signature (the generic
  `mod wire;` is private, `parse_parameters<T: FromStr>` names no native type, and the
  OpenRouter `translate_request` / `translate_tool` are private); `quality/architecture.toml`
  per-package dependency lists enforce the boundary (`intention-model` has no provider SDK
  dependency).
- Transient-only reasoning: the attachment lives on the in-memory `ModelRequestDto`; the
  runtime keeps attachments in a per-attempt local vector in `drive_attempt` and never
  appends them as facts, message text, or snapshots; no protocol or client DTO embeds
  `ModelRequestDto`; and `ModelRequestDto` is never persisted. Durable reasoning deltas
  remain the pre-existing ADR 0028 and 0037 behaviour and are not the attachment.
- ADR 0041 claims hold literally: `accept_reasoning` normalizes `reasoning_content` to
  `Primary`; it emits at most one presence marker for the empty channel; the assistant
  tool-call message echoes the matching attachment including empty text; the runtime attaches
  per round in order (`tool_round_reasoning_is_attached_to_later_requests_in_round_order`);
  the generic capability declaration is now `reasoning = true` while OpenRouter's is
  unchanged; and the OpenRouter no-op is documented and tested
  (`assistant_reasoning_attachment_never_changes_the_native_request`).
- ADR 0039 claims hold literally: advertisement is built from
  `intention_tools::model_visible_descriptors()` (Active descriptors with a
  `model_parameters_schema`, registry order read, write, edit, execute, glob, grep);
  `ModelRequestDto::with_tools` forces `tool_calls` only for a non-empty list; and both
  adapters translate whatever definitions arrive without `tool_choice`.
- No model-name-based routing, alias table, or fallback exists in `intention-model`, either
  provider crate, or `intention-runtime`.
- Single live version: waves 6 and 7 removed the legacy tool-fragment merge and finish paths
  and the optional tool-executor denial, and `ModelRunExecutionService` now requires the tool
  executor, so there is one execution path per operation with no dual-shape handling.
- Error mapping stays typed and credential-free (`ProviderErrorDto` never carries native
  text; tests assert no credential or provider text leaks), and both
  `async_openai::error::ApiError` test constructors were updated consistently with
  `misalignment: None` (the only two constructors in the workspace).
- Streaming and cancellation: both adapters stop on cancellation, fail closed on incomplete
  streams, and detect duplicate finish or usage records; there are no `unwrap` / `expect`
  calls on provider responses except fixed-code error constructors documented with clippy
  allows, and no unbounded buffer on the request path.

### A.7 Tooling, live proof and quality policy

- ADR 0038 wave 8 removal is complete with no leftovers: `dispatch`, `invoke`, and
  `invoke_with_context` are deleted from `ToolService`; every call site (tool contracts,
  coverage suites, and the new `bounded_contracts`) uses `dispatch_with_cancellation` or
  `invoke_enveloped*`; no test asserts removed behaviour; the `-1` sentinel is gone in favour
  of the typed `ToolProcessStatus` render (`exit_code:0`, `exit_code:{code}`,
  `signal:{signal}`); and a repository-wide grep finds `resolve_path_for_tool` only in
  ADR 0038's removal text.
- The deleted `tests/quality/README.md` and the empty `tests/` tree have no dangling
  reference anywhere (only ADR 0038 wave 8's instruction names the path), and no removed or
  renamed Makefile target is referenced by CI or docs (`quality.yml` only calls `ci-<phase>`;
  the new `e2e-real-api` target is in `.PHONY` and the help text).
- The live harness absorbs only model-level noise while product failures stay fatal: bounded
  attempts retry a completed run with no succeeded call, a run whose durable failed result
  for that tool proves a tool-level rejection (asserting non-empty rejected results), and a
  `SubscriberTooSlow` eviction; any other non-completed run panics. Succeeded and failed
  results are matched to calls by `call_id`, not position, and
  `facts_end_at_snapshot_cursor` requires one contiguous range ending at the snapshot cursor.
- Fixture handling matches the review brief: per-turn sessions reuse one `project_id` and
  `workspace_id` so workspace identity is shared; the edit turn reseeds the fixture via
  `prepare()` before every attempt (and write is retry-safe because
  `resolve_new_file_path` preserves the final component and `std::fs::write` overwrites);
  every daemon start, kill, and fact collection has a deadline; the only fixed sleep is the
  documented `REPLAY_QUIET_WINDOW` probe, and the remaining sleeps are bounded polls.
- No credential leak path was found: `LiveProviderConfig` and `LiveE2eHost` derive no
  `Debug`; no test prints or formats the credential; the daemon is spawned with `env_remove`
  for `INTENTION_REAL_API_*` plus `OPENAI_API_KEY` and `OPENROUTER_API_KEY`; the config
  document is written with mode 0600 in a `TempDir` and passed only through the opaque
  `RawConfigInputDto`; and the credential is asserted absent from serialized durable facts,
  session snapshots, the captured daemon log, and state bytes. The `e2e-real-api` Makefile
  target exports only `INTENTION_*` names read from the gitignored `.env` (now in
  `.gitignore`) and tees to the gitignored `quality/reports/`.
- Supply-chain policy is intact: `deny.toml` advisories version 2 with `ignore = []`,
  `quality/outdated.toml` with `crates = []`, and `check_deny_policy.py` still fails on any
  non-empty advisory ignore or outdated hold with an updated reason string. The new
  `base64@0.22.1` skip carries a reassessment note ("reassess with async-openai or
  openrouter-rs") and matches `Cargo.lock` (`base64` 0.22.1 via `async-openai` 0.42.0 and the
  `reqwest` 0.12 chain; `base64` 0.23.1 via `reqwest` 0.13); `rustls` 0.23.45,
  `async-openai` 0.42.0, and `openrouter-rs` 0.16.0 are the locked versions and
  `THIRD_PARTY_NOTICES.md` lists them (generated with `cargo about generate --locked`).
- The live workflow cannot be triggered by push, pull_request, or schedule; it is
  `workflow_dispatch` only, holds `contents: read`, passes inputs only through step and job
  environment (never interpolated into `run:` text), takes the credential only from the
  `REAL_API_E2E_PROVIDER_KEY` repository secret, and `quality/self_test.py` pins both that
  and that `e2e-real-api` is never a prerequisite of `quick`, `check`, `verify`, `ci`, or
  `ci-*`.
- Cited anchors in this area exist and assert what is claimed: every test named in ADR 0039
  and 0041 and in EVD-062, EVD-063, and EVD-064 was located (`model_contracts`,
  `tool_contracts`, `m5_tool_loop`, `m5_tool_loop_wiring`, `facade_e2e`,
  `generic_chat_contracts`, the adapter unit tests, `real_api_e2e`);
  `quality/architecture.toml` declares `real_api_e2e` for `intention-daemon` and
  `bounded_contracts` for `intention-tools`; the recorded live-run commit `722a8f4` exists
  and differs from HEAD only by the docs commit `1180f69`; and the three ADR 0040 self-tests
  are registered in `self_test.py`'s repository test list.
- DTO-first and bound semantics in `intention-tools` are coherent: `BoundedText` and
  `ExecuteInput` validate on `Deserialize`; `execute_tool` re-runs `validate_bounds` on
  in-process construction; grep caps file count, per-file reads, match count, and aggregate
  retained bytes with the truncation flag set on every drop; edit and write preflight reads
  are size-bounded and fail closed on `TooLarge`, `Unreadable`, and non-UTF-8 input; and the
  model-facing schemas are code-owned registry text with a test tying schema properties to
  the serialized inputs.
- `rustix` was the right tool for the process-group kill: `kill_process_group` and
  `test_kill_process` exist behind `cfg(unix)` and are used only under `cfg(unix)`, and the
  `i32::try_from(u32).unwrap_or(0)` fallback is harmless because rustix's `Pid::from_raw`
  rejects 0 (`NonZeroI32`), so a conversion failure degrades to "direct child only" rather
  than killing the caller's own process group.

## Appendix B. Uncertainties and open questions

Every reviewer reports what it could not settle. These items are not findings; they mark
where a decision or an additional read is required before acting.

### B.1 Domain layer

1. It could not be confirmed from the ADRs whether the application's
   `ir-selection-v1|...` admission digest is an intentional second identity (the
   reconciliation ledger PR24 rows were not read). If it is declared there, `P1-01`
   narrows to deleting the unused domain digest APIs rather than unifying the functions.
2. The domain digest functions require callers to rebuild the identity field table by
   hand, which may be intentional to keep provenance-only setters out of the record; the
   proposed fix assumes the record itself should own that table.
3. Whether `REASONING_DIALECT_VALUES` is deliberately pre-staged for the M6
   normalized-reasoning driver is not stated in ADR 0037 (which says the surface is
   "closed only" pending a complete driver), so `P3-01` could be a planned reservation
   rather than dead code.
4. The `#[serde(default)]` on the command DTO may be defensible as ADR 0038's retained
   "legitimate optional-state fields" class; it was flagged because the doc comment itself
   calls the omitted-field shape legacy.
5. No builds or tests were run (read-only review), so all behavioural claims come from
   reading code and tests; the empty-authority endpoint acceptance in `P3-03` was reasoned
   from `validate_endpoint`'s control flow rather than executed.
6. The 63-byte canonical id bound may be a deliberate stricter domain policy rather than a
   defect; the ADR Appendix table is scoped to public wire families, so `P3-02` reports the
   layer and unit divergence rather than a single wrong number.

### B.2 Protocol and client surface

1. The bare-`String` field style for ids and `schema_version` is documented as `String` in
   ADR 0037 Appendix A, so it may be an accepted (if policy-conflicting) convention rather
   than an oversight; only the instances with a concrete fail-open consequence (`P2-03`)
   plus one untyped status (`P3-08`) were flagged.
2. `ProviderCatalogStatusDto.provider_profiles_negotiated` is hardcoded `true`
   (`crates/intention-application/src/session_selection.rs:840`) and can never be false for
   any observable caller because the daemon gate rejects non-negotiated peers first. The
   PR's own review ledger fix direction (PR24-002 item 7) asked for it to be derived from
   the connection, and the ledger now claims PR24-002 is fixed; this was not raised as a
   finding because no caller can observe the difference until the client advertises the
   capability (`P1-02`).
3. Whether the daemon should validate the incoming request `schema_version` at all is a
   daemon-area question; only the client-side and DTO-shape consequences were reported,
   which is why the run-stream finding (`P2-08`) is graded on the client.
4. The review was read-only without builds or tests, so all conclusions come from code
   reading; `P1-02` was recommended for confirmation with a live two-process control-plane
   call, and the consolidator subsequently confirmed the code-level mechanism directly.

### B.3 State and configuration

1. The `workspace_roots` finding (`P2-09`) assumes the application layer can legitimately
   supply a second `WorkspaceId` for an existing root; if `WorkspaceId` is contractually
   derived one-to-one from the root, the reachable surface narrows to a direct storage
   caller, but the error-mapping defect stands either way. Not every
   `CreateSessionCommandDto` producer outside this area was traced.
2. The test suite could not be run, so it cannot be confirmed whether any existing test
   happens to hit the `workspace_root` unique path via a second `WorkspaceId`; the grep for
   `workspace_root_conflict` found only the same-id and different-root fixture.
3. Whether opaque `*_json` DTO fields are a sanctioned boundary shape depends on a charter
   reading that could not be fully settled: the storage crate header forbids raw strings at
   the boundary, the control-plane section header permits them, and no ADR line found
   explicitly sanctions opaque JSON columns in the DTO layer (this is why `P3-14` carries
   low confidence).
4. For `load_run_config_snapshot` (`P3-13`) the intended caller contract (retry versus
   permanent failure) was not confirmed beyond the trait doc; if corruption is deliberately
   indistinguishable from unavailability at that boundary, the finding is a documentation
   gap rather than a mapping bug.
5. The removal-evidence escaping defect (`P2-11`) depends on whether profile and kind
   identifiers are allowed to contain characters that JSON-escape;
   `validate_provider_string` only rejects empty, over-length, and control characters, so a
   quote or backslash is currently accepted by domain validation and would produce the
   mis-decode.

### B.4 Application services

1. Reachability of the partial-activation ordering (`P2-13`): the composition factory's
   `build` can only fail for a declaration the adapter rejects (`SafeHeader`), and catalog
   derivation currently always emits `Bearer` with no header, so today's composition cannot
   trigger it; a future kind or declaration surface would.
2. Whether summing usage across revisions in `by_profile` is intended
   (`architecture/29` says usage is "aggregated by profile and separately by
   revision/model"); the mislabelled revision and model in `P2-12` is order-dependent
   regardless of that intent.
3. Whether `AcceptProviderCatalogRemovalCommandDto.source_recheck` (`P3-17`) was meant to
   be consumed by the application or reserved for a later source-recheck slice; no ADR text
   in scope defines its effect.
4. Whether the fabricated `provider_profile_revision_id` in health evidence (`P3-20`) is a
   charter-approved placeholder: the code comment claims so, but no architecture or ADR
   text in scope records it.
5. Whether the catalog descriptor envelope's hard-coded `tool_exchange: false` is still the
   intended Slice 2 declaration now that request-side tool advertisement (ADR 0039) ships
   in the same PR; nothing in the catalog path consumes the flag today.

### B.5 Composition, daemon and adapters

1. The composition's own test documents (`crates/intention/src/lib.rs:5522`) that the
   sqlite `set_session_provider_profile` transaction rolls back every write, so session
   provider profile changes never persist; that would be a PR-blocking defect for SL2-007
   if accurate, but the storage code was outside that reviewer's area and was not verified
   there. The consolidator resolved this on 2026-09-24: the NOTE is stale, the code commits
   on every path, and the missing read-back assertion is recorded as `P3-38`.
2. It could not be established whether a process restart is supposed to re-derive the
   catalog or whether the operator must delete the state database; ADR 0037 and
   `architecture/25` only say catalog-affecting changes reject with
   `catalog_change_requires_restart`, which is what makes `P1-03` a defect rather than a
   documented limitation.
3. The false-stale window in the transport probe (`P2-14`) depends on interprocess backlog
   and timeout behaviour under load, which could not be exercised without running tests;
   the code-level mechanism (bounded probe followed by unconditional unlink) is confirmed.
4. `PendingRemoval` is treated as not-serving by both the daemon gate and the application
   gate, which blocks fresh turns for up to 30 minutes, but no production path creating a
   pending removal candidate was found (no wire command prepares a catalog), so this was
   not raised as a finding.
5. The composition hardcodes `CONFIG_SCHEMA_VERSION = (1, 0)` and
   `PROTOCOL_SCHEMA_VERSION_TEXT = "1.1"` because the owning crates expose no text or
   constant equivalent; the protocol literal matches the workspace-wide pattern, so both
   were treated as accepted style rather than findings.

### B.6 Model and provider adapters

1. The `map_openai_error` classification (`P2-16`) is pre-existing and unchanged by this
   PR; it is reported because the review brief explicitly covers SDK error mapping and the
   PR refreshed this SDK and its fixtures.
2. The runtime concatenates `Primary` and `Detail` reasoning fragments into one attachment
   text (the runtime test asserts "think deeper" from a `Primary` plus a `Detail`
   fragment). Harmless today because only the generic adapter echoes and it emits only
   `Primary`, but a future adapter that must echo its primary channel exactly would send
   detail text the gateway never produced.
3. `tool_round: u8` in `drive_attempt` has no bound (ADR 0025 states the first scope adds no
   numeric model-step limit), so after 255 tool rounds it would panic in a debug build or
   wrap in release; pre-existing and not modified by this PR.
4. Whether the six unconsumed model types (`P2-15`) are mandated by the Slice 2 ledger is
   not resolvable from the docs: ADR 0037 assigns a generic "provider-neutral reasoning
   surface (DTO-level)" to `intention-model` and names `m6_reasoning_surface.rs` as
   evidence, but no ADR enumerates `ParserLimitsV1`, `ServerSideParserConfigV1`,
   `ProviderNativePreservationControlsV1`, `ModelCapabilityEnvelopeV1`,
   `ReasoningUsageDto`, or `ResponsesReasoningMode`.
5. Live-provider behaviour was not verified by this reviewer (no network or key use), so
   the ADR 0041 echo acceptance evidence is taken as recorded rather than re-observed.

### B.7 Tooling, live proof and quality policy

1. `ToolProcessStatus::classify` is public and now panics via `unreachable!()` for a status
   with neither a signal nor a numeric code. It is believed unreachable for statuses
   returned by `wait()` / `try_wait()` on both Unix and Windows and it is the ADR 0038
   sanctioned removal of the `-1` sentinel, but a public API that can panic on a
   caller-supplied `ExitStatus` is worth a second opinion; a typed `Unknown` variant or a
   `Result` would remove the doubt.
2. The post-subscribe race in `drive_tool_turn_with`: if a run reached terminal state
   before the subscription registered, the daemon replays an empty tail and
   `collect_terminal_run` returns `Terminal` with zero facts, which trips
   `facts_end_at_snapshot_cursor` as a fatal failure rather than being absorbed. With a
   live provider taking seconds per turn this looks unreachable in practice, but it is a
   construction-time assumption rather than a proven invariant.
3. The `PipeDrain::Stalled` path kills the process group by pgid after `try_wait` has
   already reaped the child, so a microsecond-scale pid-reuse window theoretically exists
   before the group kill. This is standard practice and effectively unreachable, but it is
   the one place where the harness or tool kills a group whose leader no longer exists.
4. EVD-063 says the live harness asserts "the execute child's stdout and typed exit
   status", while the harness asserts the rendered content `exit_code:0`. The render is
   derived from the typed `ToolProcessStatus` in the same change and the text-to-typed
   mapping is pinned by `intention-tools` tests, so it was not treated as a false
   `Verified` row, but the typed `process_status` field itself is not observable in
   durable facts and is not asserted by the live test.
5. The live channel and any build were not re-run (read-only review), so the recorded run
   numbers (2026-09-23, `722a8f4`, 2 passed in 18.32 s, `make verify` green) and the
   gitignored `quality/reports/real-api-e2e/last-run.log` could not be independently
   reproduced; they are consistent with the commit graph and the code but remain
   self-reported.

## Appendix C. Cross-cutting themes

The 58 findings cluster into seven themes. The themes matter more than any single entry
because they describe habits of the change rather than isolated mistakes.

1. **Unconsumed or speculative surface (14 findings).** `P2-01` (domain reasoning DTOs),
   `P2-04` (unread `page_cursor`), `P2-06` (duplicated model families in protocol),
   `P2-07` (nine unproduced event DTOs), `P2-15` (six model types), `P3-01` (dialect table),
   `P3-08` (untyped status that only ever holds one value), `P3-10` (duplicate seed),
   `P3-11` (unused persist method), `P3-16` (unreachable tombstone guard), `P3-17` (ignored
   `source_recheck`), `P3-18` (declared event never emitted), `P3-22` (third digest),
   `P3-30` (impossible failure branch). This is the clearest pattern in the PR: several new
   public surfaces exist before their consumer, and each one carries validation, docs, and
   coverage without an execution path.
2. **More than one source of truth for the same meaning (4 findings).** `P1-01` and `P3-22`
   (four and three shapes of the selection identity), `P2-06` (protocol against model
   vocabularies), `P3-26` (tool wire names against `ToolId`), `P3-27` (kind-to-adapter
   mapping in three places). Each is a place where a future change updates one copy and
   silently leaves another behind.
3. **Fail-open gates and version checks (4 findings).** `P2-03` (unchecked `schema_version`),
   `P2-05` (hand-maintained capability classification), `P2-08` (caller-supplied stream
   schema version), `P3-25` (held-run lookup error treated as not held). All four weaken a
   guarantee that the PR's own ADRs claim as enforced.
4. **Error mapping that hides the cause (4 findings).** `P2-09` (constraint violation as
   `storage_unavailable`), `P2-16` (retryability from an optional type string), `P3-13`
   (corruption as unavailability), `P3-24` (documented degradation but propagated storage
   errors), with `P3-12` as the credential-path variant. The common effect is that the
   operator or caller cannot distinguish "retry", "fix the input", and "repair the data".
5. **Tests that prove less than their claim (9 findings).** `P2-17` (`READY` wording),
   `P3-07` (a comment claiming decode-time validation that the code does not perform),
   `P3-15` (missing escape round trip), `P3-23` (`Debug`-substring non-authority checks),
   `P3-33` (attempt budget exceeding its CI timeouts), `P3-34` (macOS state scan vacuous),
   `P3-35` (tautological containment assertion), `P3-36` (undocumented and untested coverage
   dedup), `P3-38` (stale NOTE excusing a missing read-back assertion). Each one is a gate
   that would not fail when the property it names regresses.
6. **Documentation that drifted from the code (5 findings).** `P3-05` (tombstone identity
   claim), `P3-14` (boundary statement against `*_json` fields), `P3-19` (test-target
   enumerations), `P3-24` (startup degradation claim), `P3-38` (obsolete defect note). The
   counter-examples are worth recording: the ADR 0039 and ADR 0041 claims about
   advertisement order, capability forcing, absence of `tool_choice`, and the transient
   reasoning echo were each verified literally true in the code (Appendix A.6).
7. **Two-owner formats crossing a boundary (3 findings).** `P2-10` (opaque JSON DTO fields),
   `P2-11` (unescaping reader), `P3-21` (hand-rolled writer). One format, two hand-written
   halves, no shared type: the writer escapes, the reader slices, and only a fixture nobody
   wrote would catch the mismatch.

What is solid should be stated with equal weight, because the findings above are the
exception rather than the rule: the single-live-version removal waves are complete and
verify clean (no migrations, no M4 bridge, no dual-version tolerance), credential
containment holds on every path the reviewers traced (no leaks found in code, fixtures,
logs, snapshots, or durable bytes), the canonical codecs, tags, and goldens are
version-current and descriptor-driven, the live channel absorbs only model-level noise and
keeps product failures fatal, and the supply-chain policy remains acknowledgement-free
with the refresh resolved by upgrade.

## Appendix D. Remediation packages

The packages below group the findings by the kind of change they need. They are ordered so
that each package is independently shippable.

### Package A. Correctness fixes (small, high value, recommended before merge)

| Item | Findings | Change | Effort |
| --- | --- | --- | --- |
| A1 | `P1-02` | Advertise `provider_profiles_v1` in the client hello and add a real-daemon control-plane test | small |
| A2 | `P2-03`, `P2-08` | Enforce exact current `schema_version` on the control-plane DTOs and both stream paths | small |
| A3 | `P2-09` | Map `workspace_roots` and turn or run identity constraint violations to typed conflicts | small |
| A4 | `P2-17` | Remove the `READY` wording assertion (or move it into the attempt budget) | trivial |
| A5 | `P2-11`, `P3-15` | Decode removal evidence with a JSON decoder and add the escaped-identity regression fixture | small |
| A6 | `P3-38` | Rewrite the stale NOTE and add the session-default read-back assertion | trivial |

Rationale: A1 restores the delivered client API, A2 and A3 close fail-open gates, A4 and
A5 remove two ways for the evidence to lie, and A6 deletes a false claim that already
misled one reviewer. All six are local, test-covered changes that do not require a design
decision.

### Package B. Surface cleanup in the ADR 0038 spirit

`P2-01`, `P2-02`, `P3-01`, `P3-06`, `P3-10`, `P3-11`, `P3-16`, `P3-17`, `P3-18`,
`P3-22`, `P3-30`, `P3-31`, plus the delete-or-wire decisions for `P2-06`, `P2-07`,
`P2-15`. Each item either removes a surface with no consumer or makes the closed value,
codec, or event real. Where the surface is genuinely reserved for a later slice, the fix is
to declare it in the ADR or roadmap with an evidence anchor rather than to keep it
implicit.

The three delete-or-wire items (`P2-06`, `P2-07`, `P2-15`) carried an unresolved fork when this
package was first written: the paragraph above states both branches. The fork was put to the
owner on 2026-09-24 and answered as **D-16, option (b)** (Appendix G.11.6): audit each of the
three groups against the roadmap's M6 to M9 plan, declare the groups a named slice will consume
in the ADR or roadmap with an evidence anchor, and delete the rest. The package is therefore
executable, but only through that audit; it must not be executed as a blanket deletion or as a
blanket declaration.

### Package C. Design decisions required before implementation

**Status: closed on 2026-09-24.** Sixteen items required a decision, all sixteen were put to
the owner and answered, and the accepted answers are recorded in Appendix G. The decision
identifiers are cited from each finding section. The lists below are kept as the record of what
was open before the decisions.

- `P1-01` → **D-01, option A**: one canonical identity digest (Appendix G.1).
- `P1-03` → **D-02, option A**: startup re-derives the catalog (Appendix G.2).
- `P2-05` → **D-03, option A**: capability classification as protocol data (Appendix G.3).
- `P2-12` → **D-04, option A**: usage aggregation per identity (Appendix G.4).
- `P2-13` → **D-05, option A**: build before durable acceptance (Appendix G.5).
- `P2-14` → **D-06, options A and B**: identity-verified reclaim plus no unconditional `Drop`
  removal (Appendix G.6).
- `P2-10` and `P3-21` → **D-07, option A, full replacement**: no opaque JSON string field
  remains (Appendix G.7).

Three further findings were promoted into this package because they also required a decision,
and all three are now closed:

- `P3-07` → **D-08, option 1 plus deserialize**: decode-time validation for the control-plane
  families (Appendix G.8). Removed from Packages A and B.
- `P3-20` → **D-09, option 2 as the primary choice**: the fabricated profile revision
  disappears (Appendix G.9). Removed from Package B.
- `P3-28` → **D-10, (1a) plus (2) plus (3)**: render from the snapshot AST inside
  `intention-config` while keeping the round trip (Appendix G.10). Removed from Package B.

#### Items reopened for decision on 2026-09-24 (now closed)

The coverage audit of this register found 19 findings with no remediation home. Eight of them
are mechanical and are now Package E (fail-closed corrections) or Package F (test and evidence
hardening); the remaining five could not be implemented without choosing between materially
different outcomes, and they were put to the owner as briefs in Appendix G.11. All five, plus
the Package B fork for `P2-06`, `P2-07`, and `P2-15`, were answered on 2026-09-24:

- `P2-04` → **D-11, option (b)**: delete the request cursor and its validation; the durable
  reconciliation marker stays the single paging authority (Appendix G.11.1).
- `P2-16` → **D-12, option (a)**: classify SDK errors by HTTP status and fix the pre-existing
  defect in this change (Appendix G.11.2).
- `P3-02` → **D-13, option (a)**: count characters at both layers and state the canonical bound
  per field in the ADR 0037 field table; executed with D-01 (Appendix G.11.3).
- `P3-08` → **D-14, option (a)**: replace the untyped `reload_status` string with a closed enum;
  executed inside D-08 (Appendix G.11.4).
- `P3-32` → **D-15, option (a)**: bound the accumulated reasoning echo per round and record an
  unrepresentable attachment as a durable typed failed run, documented in ADR 0041
  (Appendix G.11.5).
- `P2-06`, `P2-07`, `P2-15` → **D-16, option (b)**: audit the three unconsumed surface groups
  against the roadmap's M6 to M9 plan, declare the reserved ones with an evidence anchor, and
  delete the rest (Appendix G.11.6).

Package C is therefore closed for all sixteen items, and no finding in this register is left
waiting for a decision. The implementation order is Appendix G.12.

### Package D. Quality policy and documentation consistency

`P3-19` (test-target enumerations), `P3-36` (coverage dedup rule in policy and self-test),
`P3-14` (boundary statement), `P3-05` (tombstone wording), `P3-24` (startup error
contract), plus the register updates that follow from Packages A to C (EVD rows, ADR field
tables, ownership map).

### Package E. Fail-closed corrections and boundary tightening

Small, mechanical corrections that need no decision. Each one restores fail-closed behaviour
or removes a duplicated authority, and each carries a focused regression fixture.

| Item | Findings | Change | Effort |
| --- | --- | --- | --- |
| E1 | `P3-03` | Reject an empty authority in the HTTPS branch of `validate_endpoint` (`intention-domain`), which `intention-config` delegates to; add rejection fixtures for `https:///v1` and `https://:8080/v1` at both layers | small |
| E2 | `P3-25` | Treat a failed held-run lookup as held (or propagate the error) so the held marker is bypassed only on a positive `false`; the fixture injects a lookup failure and asserts the run stays held and non-terminal | small |
| E3 | `P3-26` | Parse the provider tool name into `intention_tools::ToolId` and match on the typed value, so advertisement and decoding share one name authority; add the enumerate-descriptors round-trip test | small |
| E4 | `P3-04` | Drop the `#[serde(default)]` legacy tolerance on the send-user-turn override fields so absence is a decode error while `null` stays valid, and reword the doc comment to the current optional-state contract | trivial |
| E5 | `P3-13` | Map a malformed persisted `snapshot_json` to `storage_decode_failed` (`codec_error`) and keep `run_configuration_unavailable` for the missing-row case | trivial |

Three further items in this group are executed inside an accepted decision, because they sit
on that decision's code path. They are listed here so the group stays complete:

| Item | Findings | Executed with | Change |
| --- | --- | --- | --- |
| E6 | `P3-29` | D-02 (Appendix G.2) | `activate_startup_catalog` loses its public credential-bearing signature: it becomes a private helper of `open_platform` or takes the parsed startup material |
| E7 | `P3-27` | D-02 (Appendix G.2) | The adapter builder is selected by the typed `ProviderKindDto` instead of the literal `"openrouter"`, with a typed unsupported-kind error instead of a silent fall-through |
| E8 | `P3-12` | D-10 (Appendix G.10) | `restore_credential_document` returns a typed error instead of the credential-free document when the document shape or the re-serialization fails, preserving the redaction guarantee |

Rationale: E1, E2, E4, and E5 close fail-open or misleading-error paths; E3, E6, and E7
remove a second authority for a name, a provider kind, and a credential-bearing entry point;
E8 stops a document-shape failure from being reported as a missing credential.

### Package F. Test and evidence hardening

Fixtures and harness corrections. These change no production behaviour, but they decide
whether the gates can detect a regression at all.

| Item | Findings | Change | Effort |
| --- | --- | --- | --- |
| F1 | `P3-09` | Add removal-lifecycle fault points plus the reopen-and-compare fixture for accept, reject, and expire | small |
| F2 | `P3-23` | Replace the `Debug`-substring non-authority assertions with structural assertions (no authority-bearing dependency or field) plus a routing and fallback fixture; align the EVD-051 wording | medium |
| F3 | `P3-33` | Bring the harness worst case inside the workflow step and job timeouts (lower `TOOL_TURN_ATTEMPTS` or `TURN_DEADLINE`, or raise both timeouts) so a degraded live run reports the harness diagnostic instead of a CI timeout | trivial |
| F4 | `P3-34` | Scan the directory that actually holds durable state on each platform (`config_home` on macOS, `state_home` elsewhere) and fail loudly on an empty target | small |
| F5 | `P3-35` | Replace the tautological "no raw provider text" assertion with the positive invariant (every recorded failure code is in the normalized closed set, every durable string is the bounded normalized value) or with the fixture's raw provider phrase | small |
| F6 | `P3-37` | Wrap the post-restart replay subscribe and the credential-check snapshot reads in the harness deadline, or add a transport read timeout, so every wait is bounded | small |

F3 and F4 change the meaning of the recorded live evidence, so they must be followed by
`make e2e-real-api` and an EVD-063 update; F2 touches EVD-051.

### D.1 Coverage of all 58 findings

Every finding in the index has exactly one remediation home. `C/D-nn` means the finding is
implemented through an accepted decision, with the accepted option in parentheses; every
decision is accepted, so no row is waiting on the owner.

| Finding | Remediation home |
| --- | --- |
| `P1-01` | Package C → D-01 |
| `P1-02` | Package A → A1 |
| `P1-03` | Package C → D-02 |
| `P2-01` | Package B |
| `P2-02` | Package B |
| `P2-03` | Package A → A2, implemented inside D-08 |
| `P2-04` | Package C → D-11 (b), delete the request cursor |
| `P2-05` | Package C → D-03 |
| `P2-06` | Package B → D-16 (b), audit then delete or declare |
| `P2-07` | Package B → D-16 (b), audit then delete or declare |
| `P2-08` | Package A → A2 |
| `P2-09` | Package A → A3 |
| `P2-10` | Package C → D-07 |
| `P2-11` | Package A → A5 |
| `P2-12` | Package C → D-04 |
| `P2-13` | Package C → D-05 |
| `P2-14` | Package C → D-06 |
| `P2-15` | Package B → D-16 (b), audit then delete or declare |
| `P2-16` | Package C → D-12 (a), classify by HTTP status |
| `P2-17` | Package A → A4 |
| `P3-01` | Package B |
| `P3-02` | Package C → D-13 (a), count characters, executed with D-01 |
| `P3-03` | Package E → E1 |
| `P3-04` | Package E → E4 |
| `P3-05` | Package D |
| `P3-06` | Package B |
| `P3-07` | Package C → D-08 |
| `P3-08` | Package C → D-14 (a), closed enum, implemented inside D-08 |
| `P3-09` | Package F → F1 |
| `P3-10` | Package B |
| `P3-11` | Package B |
| `P3-12` | Package E → E8, executed with D-10 |
| `P3-13` | Package E → E5 |
| `P3-14` | Package D |
| `P3-15` | Package A → A5 |
| `P3-16` | Package B |
| `P3-17` | Package B |
| `P3-18` | Package B |
| `P3-19` | Package D |
| `P3-20` | Package C → D-09 |
| `P3-21` | Package C → D-07 |
| `P3-22` | Package B |
| `P3-23` | Package F → F2, referenced from D-09 |
| `P3-24` | Package D |
| `P3-25` | Package E → E2 |
| `P3-26` | Package E → E3 |
| `P3-27` | Package E → E7, executed with D-02 |
| `P3-28` | Package C → D-10 |
| `P3-29` | Package E → E6, executed with D-02 |
| `P3-30` | Package B |
| `P3-31` | Package B |
| `P3-32` | Package C → D-15 (a), bound the echo, typed failed run |
| `P3-33` | Package F → F3 |
| `P3-34` | Package F → F4 |
| `P3-35` | Package F → F5 |
| `P3-36` | Package D |
| `P3-37` | Package F → F6 |
| `P3-38` | Package A → A6 |

Package totals: A 8, B 15, C 16 (sixteen accepted decisions, of which D-16 covers three
findings), D 5, E 8, F 6, which is 58.

## Appendix E. Verification plan

Every fix should be verified with the narrowest relevant test first, then the standard
gates. The commands below are the repository's own interface (`Makefile`), plus the
per-crate test targets named in the findings.

1. **Per-fix focused tests.**
   - A1: `cargo nextest run -p intention-client -p intention-daemon --tests`.
   - A2: `cargo nextest run -p intention-protocol -p intention-client -p intention-daemon --tests`.
   - A3: `cargo nextest run -p intention-storage-sqlite --tests`.
   - A4, A5: `cargo nextest run -p intention-daemon --tests` (live target stays ignored) and
     `cargo nextest run -p intention-storage-sqlite --tests`.
   - A6: `cargo nextest run -p intention --tests`.
2. **Iteration gate.** `make quick` after each commit (formatting, clippy, workspace tests).
3. **Full gate.** `make verify` before handoff: `check` (fmt, feature profiles, clippy
   across profiles, the full test suite, docs-check, architecture), `coverage` (all
   profiles and workspace aggregates against the declared tiers), `deps` (deny, audit,
   machete, outdated, udeps, notices), and `quality-self-test`.
4. **Live evidence.** `make e2e-real-api` after harness changes (A4, A5) and after the
   capability fix if the live path touches the client; record date, commit, provider kind,
   model, and result in EVD-063 per ADR 0040. Never record the key.
5. **Documentation gate.** `make docs-check` for every documentation change, and update the
   machine-readable policy plus architecture text in the same change when coverage,
   feature profiles, or supply-chain policy are touched (AGENTS.md requirement).
6. **Regression evidence for the two highest-risk fixes.** For A1, a test that fails today
   (the real client is rejected) and passes after; for A3, a fixture that reproduces the
   live-run failure mode (a second workspace identity over one root) and asserts the typed
   conflict.
7. **Package E focused targets.**
   - E1: `cargo nextest run -p intention-domain -p intention-config --tests` (rejection fixtures
     for `https:///v1` and `https://:8080/v1` at both layers, plus a valid host with a port).
   - E2: `cargo nextest run -p intention-daemon --tests` with a fixture that injects a held-run
     lookup failure and asserts the run stays held and non-terminal.
   - E3: `cargo nextest run -p intention-daemon -p intention-tools --tests`, including the test
     that enumerates `model_visible_descriptors()` and decodes every advertised name.
   - E4: `cargo nextest run -p intention-domain --tests` with an omission fixture asserting the
     decode error and the existing present and null cases.
   - E5: `cargo nextest run -p intention-storage-sqlite --tests` with a corrupted `snapshot_json`
     row asserting `storage_decode_failed`, and the existing missing-row case asserting
     `run_configuration_unavailable`.
   - E6, E7: `cargo nextest run -p intention --tests`, including the compile-time check that
     `activate_startup_catalog` is no longer public and the unknown-kind typed rejection.
   - E8: `cargo nextest run -p intention-config --tests`, with the rewritten pass-through test
     asserting the typed error.
8. **Package F focused targets.**
   - F1: `cargo nextest run -p intention-storage-sqlite --tests`, with the fault test proven to
     fail before the fix when the transaction is not atomic.
   - F2: `cargo nextest run -p intention-application --tests`, with the structural assertions
     shown to fail under a deliberate temporary authority field or dependency.
   - F3 to F6: `make e2e-real-api` (harness-only changes), then record the run and update
     EVD-063; F4 additionally requires the run on the affected platform.
9. **Decisions D-11 to D-16 (accepted).** Each brief in Appendix G.11 carries its own test plan,
   and the finding's verification paragraph still applies:
   - D-11: assert the request field is gone from the wire shape and keep the response-cursor
     assertions; `cargo nextest run -p intention-protocol -p intention-client -p intention-application --tests`.
   - D-12: unit fixtures for an untyped 400, an untyped 500, and a typed 429 asserting the
     retryability of the resulting `ProviderErrorDto`, then `make e2e-real-api` to confirm the live
     failure class for an invalid credential.
   - D-13: multi-byte identifier fixtures inside and outside the documented bound at both layers,
     plus a test asserting the canonical and wire bounds agree.
   - D-14: a round-trip test for every enum value and a decode rejection for an unknown string.
   - D-15: a reasoning fragment that passes the durable bound but exceeds the attachment bound,
     asserting a typed failed run fact, plus a control-character case.
   - D-16: for every deleted type, its unit tests are removed with it; for every declared type, the
     test target and coverage tier entry the declaration promises.

## Appendix F. Provenance, method limits, and consolidator notes

**Provenance.** The findings come from seven independent read-only reviewer sessions, one
per area, each given the same brief and the same JSON output schema, plus two direct
verifications by the consolidator. Reviewer areas map to the PR's crate and path structure:
domain (`crates/intention-domain`, `crates/intention-types`), protocol and client
(`crates/intention-protocol`, `crates/intention-client`), state and configuration
(`crates/intention-storage`, `crates/intention-storage-sqlite`, `crates/intention-config`),
application (`crates/intention-application`), composition and daemon (`crates/intention`,
`crates/intention-daemon` excluding the live harness, `crates/intention-transport`,
`crates/intention-workspace`, `crates/intention-tui`, `crates/intention-test-support`),
model and providers (`crates/intention-model`,
`crates/intention-provider-generic-chat`, `crates/intention-provider-openrouter`,
`crates/intention-runtime`), and tooling, live proof, and quality policy
(`crates/intention-tools`, `crates/intention-daemon/tests`, `quality/`, `Makefile`,
`deny.toml`, `Cargo.lock`, `.github`, `THIRD_PARTY_NOTICES.md`, `.gitignore`, `tests/`, plus
the documentation cross-check for ADR 0037 to 0041 and the reconciliation registers).

**Consolidator verification log (2026-09-24).**

1. `P1-02` confirmed directly: `REQUIRED_CAPABILITIES` in
   `crates/intention-client/src/lib.rs:52-56` holds three capabilities and no
   `ProviderProfilesV1`; the daemon rejects gated commands and queries at
   `crates/intention-daemon/src/lib.rs:1117-1120` and `:1140-1143`; and
   `require_provider_profiles` (`crates/intention-protocol/src/negotiation.rs:129-131`) is
   referenced only from tests.
2. `P3-38` established directly: `set_session_provider_profile`
   (`crates/intention-storage-sqlite/src/control_plane.rs:2549-2650`) commits on all four
   paths, so the composition NOTE at `crates/intention/src/lib.rs:5522` is stale and the
   end-to-end read-back assertion is missing. The composition reviewer's uncertainty about
   a potential PR-blocking persistence defect is therefore resolved as "no defect, stale
   note".

**Method limits.**

- The review was static: reviewers were instructed not to build, run tests, or read
  `target/`, `.env`, or the gitignored live-run reports. No reviewer executed the failing
  path; trigger paths are traced from code. Where a claim depends on runtime scheduling or
  provider behaviour, the finding says so and the uncertainty appendix repeats it.
- The live provider channel was not re-run during the review; the recorded run remains the
  manual evidence obligation of ADR 0040.
- The reviewers did not read the PR24 review ledger dispositions for every row, so a few
  findings may intersect deliberate decisions recorded there (notably `P1-01` and the
  `provider_profiles_negotiated` note in Appendix B.2). Reading the ledger rows named in
  the findings is the first step of Package C work.
- Related findings were deliberately kept separate where they require separate changes:
  `P1-01` and `P3-22` (dead canonical APIs versus a third ad-hoc digest), `P2-10` and
  `P3-21` (opaque DTO fields versus the hand-rolled writer), `P2-11` and `P3-15` (decoder
  defect versus the missing regression fixture), and `P2-07` and `P3-18` (unproduced event
  family versus the concrete missing emission).
- No P0 finding was established: no defect with an unconditional crash, exploit, or
  data-loss path on a primary code path survived the discipline rules above.

**Register completion (2026-09-24, second pass).** The first pass recorded 58 findings and the
decisions D-01 to D-10 for the eleven items that needed one, but 19 findings had no remediation
home. A coverage audit of this document against its own index found them: eight mechanical
corrections became Package E, six test and evidence tasks became Package F, and five required a
decision, which were put to the owner as briefs D-11 to D-15 together with the delete-or-wire fork
for `P2-06`, `P2-07`, and `P2-15` as D-16. All six were accepted as recommended on the same date,
so every finding in the index now has exactly one remediation home, stated in Appendix D.1 and
repeated in its own section. Nothing in the register is waiting on a decision.

**References.**

- Pull request: `https://github.com/y0tsy/intention-relay/pull/36` (CI run
  `35895541037`, 9 of 9 jobs green at `1180f69`).
- Superseded PR: `https://github.com/y0tsy/intention-relay/pull/24` (closed, not merged;
  comment `5799952122`).
- Governing documents: `AGENTS.md`, `docs/intention-relay/architecture/02-dto-and-contract-policy.md`,
  `docs/intention-relay/architecture/11-implementation-roadmap.md` (Milestone 5+ section),
  `docs/intention-relay/decisions/0037` to `0041`, and
  `docs/intention-relay/reconciliation/` (evidence register, source-of-truth matrix,
  ownership map, PR24 review ledger).

## Appendix G. Accepted remediation decisions (2026-09-24)

Every decision this register requires was put to the owner on 2026-09-24 and answered: the
eleven Package C items, the three findings previously filed under Packages A and B that turned
out to require a decision, and the six items the coverage audit of the same date found without a
remediation home. The answers are recorded here verbatim in intent and expanded into
implementation detail. Where a decision supersedes an earlier package assignment, the
supersession is stated. The accepted set is D-01 to D-16, and no finding in this register is
waiting for a decision.

### G.0 Decision summary

#### Accepted decisions D-01 to D-10 (first round)

| Decision | Finding(s) | Chosen option | One-line intent |
| --- | --- | --- | --- |
| D-01 | `P1-01` | Option A | One canonical identity digest, owned by the record, called by application and storage; the ad-hoc digests are deleted |
| D-02 | `P1-03` | Option A | At startup the catalog is re-derived from the startup document through the normal prepare and accept path; the driver and the catalog can never disagree |
| D-03 | `P2-05` | Option A | The capability classification becomes data on the protocol surface with an exhaustive match; the daemon calls it; one owner for the error code |
| D-04 | `P2-12` | Option A | Usage aggregation returns one result per identity (revision, model) instead of one total labelled with the last row |
| D-05 | `P2-13` | Option A | The replacement registry is fully built and its limits pre-validated before the durable catalog acceptance; activation then cannot fail |
| D-06 | `P2-14` | Options A **and** B | Identity-verified unlink (device and inode) **and** no unconditional path removal in `Drop`; reclaim at the next bind is the only removal path |
| D-07 | `P2-10`, `P3-21` | Option A, full replacement | Every opaque JSON string is replaced by the typed record it encodes; nothing of the string surface remains |
| D-08 | `P3-07` | Option 1 plus deserialize | Every invariant-bearing control-plane DTO decodes through a private raw shape whose `Deserialize` validates, matching the M3 and M4 pattern |
| D-09 | `P3-20` | Option 2 as the primary choice | The fabricated profile revision disappears; the field becomes optional and is absent until the catalog is genuinely wired into the health path |
| D-10 | `P3-28` | (1a) plus (2) plus (3) | The document is rendered from the safe snapshot as a TOML AST inside `intention-config`, the server-side round trip is kept, and the composition no longer renders TOML |

#### Accepted decisions D-11 to D-16 (coverage audit, second round)

These six close the gap the coverage audit found on 2026-09-24: 19 findings had no remediation
home, of which five needed a decision and three more were the delete-or-wire fork inside
Package B. All six were accepted as recommended. Each is expanded in the corresponding brief in
G.11, which now records the accepted option rather than a choice.

| Decision | Finding(s) | Accepted option | One-line intent | Detail |
| --- | --- | --- | --- | --- |
| D-11 | `P2-04` | (b) delete the request field | The reconciliation request no longer carries a page cursor; the durable marker stays the single paging authority and the response cursor is audited | G.11.1 |
| D-12 | `P2-16` | (a) classify by HTTP status and fix here | Retryability comes from the HTTP status (429 and 5xx retryable, other 4xx not), the `type` string is a secondary signal, and the pre-existing defect is fixed in this change | G.11.2 |
| D-13 | `P3-02` | (a) count characters at both layers | The identifier bound is one number per field counted in characters, stated in the ADR 0037 field table; executed with D-01 | G.11.3 |
| D-14 | `P3-08` | (a) closed enum | `reload_status` becomes a closed enum validated by serde; executed inside D-08 | G.11.4 |
| D-15 | `P3-32` | (a) bound the echo, typed failed run | The accumulated reasoning echo is bounded per round and an unrepresentable attachment becomes a durable typed failed run, recorded in ADR 0041 | G.11.5 |
| D-16 | `P2-06`, `P2-07`, `P2-15` | (b) audit, declare the reserved, delete the rest | Each unconsumed surface group is audited against the roadmap's M6 to M9 plan; a group a named slice will consume is declared with an evidence anchor, everything else is deleted | G.11.6 |

### G.1 D-01 (`P1-01`) — one canonical identity digest

**Chosen option:** A. The record owns its identity digest; application and storage call it;
the competing ad-hoc digests are deleted.

**What changes.**

1. In `crates/intention-domain/src/provider_selection.rs`, the canonical record gains an
   identity digest built from the same field table that `encode()` uses, so the bytes and the
   digest can never disagree. The existing canonical digest functions at `:647` and `:675`
   are consolidated into that single entry point rather than kept as a parallel API.
2. In `crates/intention-application/src/provider_catalog.rs:1463-1484`, the local
   `"ir-selection-v1|profile=...|source=..."` format-string digest is deleted.
   `ProviderAdmissionDto.selection_digest` is either populated from the canonical digest or
   removed; if it stays, it must have a reader. `selection_source` must not enter identity,
   because the domain documents it as provenance.
3. In `crates/intention-storage-sqlite/src/control_plane.rs:1116-1117`, the SHA-256 of the
   selection JSON text is replaced by the canonical digest, so the persisted digest column
   and the domain identity are the same function.
4. In `crates/intention/src/lib.rs:1298-1306`, the persisted selection stays the canonical
   record encoding (`"ir-record:" + hex(encode())`). That value is the record, not a competing
   digest, so it is kept as-is; the digest column is what moves to the canonical function.
5. The canonical field table is no longer re-assembled by hand at call sites; the tests that
   currently rebuild it (`crates/intention-domain/tests/m5_control_plane_canonical.rs:1127-1197`)
   read it from the record instead.
6. `P3-02` sits on this same identity surface and is executed with this decision: the canonical
   record counts bytes (`MAX_PROVIDER_ID_CHARS = 63`) while the wire contract and ADR 0037
   Appendix A speak of characters (up to 256), so one identifier can pass one boundary and fail
   the other before it is ever digested. Accepted decision D-13 (Appendix G.11.3) counts characters
   at both layers and puts one number per field in the ADR 0037 field table, so the bytes-versus-
   characters question is settled rather than documented.

**Prerequisite.** Read the PR24 ledger rows for the provider-selection area before editing,
to confirm that no second admission identity was deliberately declared. If one was, the
decision narrows to deleting the unused domain digest APIs, and the ledger row must be cited
in the change.

**Tests.** Build one logical selection through the application path, persist it through
storage, and assert the persisted digest equals the canonical domain digest. Add a guard test
(or lint-style assertion) that no `"ir-selection-v1|"`-shaped digest computation exists
outside the domain. Run `cargo nextest run -p intention-domain -p intention-application -p intention-storage-sqlite --tests`
and `make quick`.

**Documentation.** ADR 0037 field table note that the selection identity is the canonical
record digest; reconciliation row update if the ledger referenced the removed digest.

**Sequencing.** D-01 runs before D-07, because both touch selection persistence and D-07
replaces the field that carries the selection across the storage boundary.

### G.2 D-02 (`P1-03`) — startup re-derives the catalog

**Chosen option:** A. The startup path compares the startup declaration with the active
catalog and, on a difference, runs the normal prepare and accept path in-process so the
catalog catches up with the file.

**What changes.**

1. In `crates/intention/src/lib.rs`, `activate_startup_catalog` (`:1443-1451`) no longer
   returns early on an active catalog. It compares the startup-derived declaration (kind,
   model, endpoint) with the active catalog's active profile declaration.
2. On a difference, the in-process path prepares a catalog candidate from the startup
   document and accepts it, so the normal acceptance path records the new revision and the
   usual evidence (candidate, audit, activation) is produced. On success the active catalog
   matches the file.
3. The invariant "the executing driver kind equals the active catalog kind" is asserted, so a
   `SelectedProvider` whose kind differs from the active catalog can never be constructed.
   This also covers the comparison currently performed at `:1824-1831`, which only checks the
   provider kind against the config snapshot.
4. `open_with_selected_provider` and `run` both reach startup activation; the work must end
   up invoked once per process start, with the second call being a no-op on an already
   reconciled catalog.
5. The `catalog_change_requires_restart` guidance is corrected in
   `crates/intention-config/src/control_plane.rs` (message and doc for
   `reject_catalog_affecting_edits`) and in `architecture/25`, so the advertised recovery
   matches reality. Reload still rejects catalog-affecting edits; the restart now genuinely
   applies them.
6. `P3-29` is executed with this decision, because the method it concerns is this path:
   `activate_startup_catalog` (`crates/intention/src/lib.rs:1443`) is public and takes raw
   configuration text that includes the credential, while its only caller is `open_platform`
   (`:1425`) and the daemon host never calls it (`run()` calls `open_platform` and
   `provider_control_startup` only). It becomes a private helper of `open_platform` or takes the
   already-parsed startup material, so the raw credential text stays inside the private loading
   boundary and the public surface stops carrying a credential-bearing entry point.
7. `P3-27` is executed with this decision, because the invariant in item 3 cannot hold while the
   kind is a string: the catalog factory (`crates/intention/src/lib.rs:307-312`) and the
   credential-rebuild port (`:1046-1051`) both compare the kind to the literal `"openrouter"`
   and fall through to the generic-chat builder, and `:126-130` stores the kind as a `String`
   although the typed `ProviderKindDto` already exists and is used at `:643-651`. Both sites
   dispatch on the typed kind and return a typed unsupported-kind error instead of falling
   through, which is what makes "the driver kind equals the catalog kind" checkable rather than
   asserted.

**Verified constraint that shapes the implementation.** `ProtocolCommandDto` contains no
command that prepares or activates a catalog: the only catalog commands are
`AcceptProviderCatalogRemoval` and `RejectProviderCatalogCandidate`. The re-derivation is
therefore an internal startup path and must not be exposed as a new wire command, which also
keeps the control-plane surface unchanged.

**Tests.** Open a store, activate a catalog, edit the model or kind in the startup document,
re-open, and assert that the active catalog profile declaration matches the file (or that a
typed rejection is returned). Add the driver-kind equals catalog-kind invariant assertion. Add
the unknown-kind fixture asserting the typed unsupported-kind error and a fixture asserting that
the catalog factory and the credential-rebuild port build the same adapter for the same
declaration (`P3-27`), plus a compile-time check that `activate_startup_catalog` is no longer
public (`P3-29`).
Run `make quick` and the daemon facade end-to-end target.

**Documentation.** `architecture/25`, ADR 0037 (startup and catalog contract), and the
`catalog_change_requires_restart` wording in both places.

### G.3 D-03 (`P2-05`) — capability classification as protocol data

**Chosen option:** A. The classification lives on the protocol surface with an exhaustive
match; the daemon calls it; the error code has one owner.

**What changes.**

1. `ProtocolCommandDto` and `ProtocolQueryDto` gain a classification method (for example
   `requires_provider_profiles()` or `is_control_plane()`) implemented with an exhaustive
   `match` and no wildcard arm, so adding a variant fails to compile until the author
   classifies it. The classification sits next to the variants in
   `crates/intention-protocol/src/lib.rs`.
2. `crates/intention-daemon/src/lib.rs:1173-1204` deletes
   `command_requires_provider_profiles` and `query_requires_provider_profiles` and calls the
   protocol classifier (or `negotiation::require_provider_profiles`) instead. The duplicated
   error constructor at `:1148-1153` is removed, leaving
   `crates/intention-protocol/src/negotiation.rs:74` as the single owner of
   `provider_profiles_capability_required`.
3. The existing daemon gate tests (`:2462-2495`) stay as behavioural coverage and gain the
   real-client case, which depends on the `P1-02` fix (client capability advertisement).

**Tests.** A test that enumerates every `ProtocolCommandDto` and `ProtocolQueryDto` variant and
asserts each is classified; with the exhaustive match, a new unclassified variant is a compile
error. Run `cargo nextest run -p intention-protocol -p intention-daemon --tests` and
`make quick`.

**Documentation.** ADR 0037 and `architecture/29` note where the classification lives and that
the gate is derived from the protocol surface rather than a hand-maintained list.

### G.4 D-04 (`P2-12`) — usage aggregation per identity

**Chosen option:** A. Aggregation returns one result per identity instead of one total
labelled with the last iterated row.

**What changes.**

1. `aggregate_usage` in `crates/intention-application/src/session_selection.rs` groups by
   `(provider_profile_revision_id, model_id)` and returns one aggregation per identity rather
   than assigning the last row's identity to a combined total.
2. The projection type and the `by_profile` signature change accordingly, and the protocol
   DTO plus the client method follow. The returned collection is bounded; the bound is
   declared explicitly, consistent with the bounded-collection policy, and the storage-side
   row order no longer influences any reported field.
3. `by_revision_and_model` keeps its current single-identity semantics.

**Tests.** A multi-revision, multi-model fixture in
`crates/intention-application/tests/m5_session_selection.rs` asserting per-identity totals and
identity fields; the existing single-revision test is updated to the new shape. Run
`cargo nextest run -p intention-application -p intention-protocol -p intention-client --tests`
and `make quick`.

**Documentation.** `architecture/29` usage wording and the ADR 0037 field table if the DTO
shape changes.

### G.5 D-05 (`P2-13`) — build the registry before the durable acceptance

**Chosen option:** A. The replacement registry is fully built and its limits pre-validated
before the durable acceptance, so the subsequent activation cannot fail.

**What changes.**

1. In `crates/intention-application/src/provider_catalog.rs`, the auto-accept path of
   `prepare_candidate` builds the replacement registry and admissions map (and validates the
   `MAX_ACTIVE_PRIVATE_ENTRIES` bound) **before** calling `accept_provider_catalog`. The
   current order is durable accept at `:714`, registry build at `:724`, activation at `:1199`.
2. Only after a successful build does the code commit the acceptance and activate the
   registry and gate, so durable state and in-memory state move together. Activation is
   reduced to an operation that cannot fail once the pre-validation has passed.
3. A failing build leaves no durable advance and returns the typed error, so the caller's
   error describes the actual state.

**Tests.** Add an injectable failing factory (a `CountingFactory` variant that fails for a
declared kind or declaration) and assert that no durable advance occurs, or that a typed
recovery-required state is recorded whose durable revision equals the in-memory gate after a
reopen. Run `cargo nextest run -p intention-application --tests` and `make quick`.

**Documentation.** ADR 0037 acceptance contract wording, so the all-or-nothing claim matches
the implementation order.

### G.6 D-06 (`P2-14`) — identity-verified reclaim and no unconditional `Drop` removal

**Chosen options:** A **and** B, both applied.

**What changes.**

1. **A, identity verification.** `reclaim_stale_socket`
   (`crates/intention-transport/src/lib.rs:880-888`) captures the socket's identity (device
   and inode) before the bounded probe and removes the path only when the identity is
   unchanged after the probe. The listener records the identity of the socket it created at
   bind, and `Drop` (`:273-279` and `:335-341`) removes the path only when it still resolves
   to that same socket.
2. **B, no unconditional removal.** `Drop` no longer removes the endpoint path at all. A
   clean shutdown therefore leaves the socket file behind, and the next bind reclaims it
   through the now identity-verified path. This removes the cross-process hazard entirely:
   one host can never unlink another host's live socket, and the retry bind after reclaim
   (`:244-247`) remains the single recovery path.

**Behavioural consequence to document.** After option B, an unclean and a clean exit look the
same on disk (a socket file without a listener). This is intentional and safe because reclaim
is the designated recovery, and it is the reason option A must land in the same change: the
reclaim path is now the only removal path and must be correct.

**Tests.** (a) A live listener whose backlog is saturated is not unlinked by reclaim;
(b) dropping one listener does not remove a socket owned by another; (c) a socket file left by
a previous process is reclaimed on the next bind. Keep the non-Unix fail-closed stub test. Run
`cargo nextest run -p intention-transport --tests` on Unix and `make quick`.

**Documentation.** `architecture/03` transport reclaim text, stating that reclaim is the only
removal path and that it is identity-verified.

### G.7 D-07 (`P2-10`, `P3-21`) — full replacement of the opaque JSON string surface

**Chosen option:** A, full replacement. Nothing of the string surface remains.

**What changes.**

1. In `crates/intention-storage/src/lib.rs`, every free-form JSON string field is replaced by
   the typed record it encodes:
   - `UnavailableRunQueueEntryDto.selection_json` (`:1198`) and
     `EnqueueUnavailableRunInputDto.selection_json` (`:1211`) carry `ProviderSelectionV1`;
   - `ProviderUsageEventInputDto.usage_json` (`:1277`) carries the typed usage record;
   - `ProviderCatalogRemovalCandidateDto.candidate_json` (`:1325`) and
     `CreateProviderCatalogRemovalCandidateInputDto.candidate_json` (`:1338`) carry the typed
     removal-evidence record;
   - `ProviderCatalogProfileEntryDto.safe_projection_json` (`:1032`) carries the typed safe
     projection record.
2. In `crates/intention-application/src/provider_catalog.rs`, the hand-rolled writer
   `removal_candidate_json` and its `json_string` escaper (`:1586-1607`) are deleted; the
   application constructs the typed removal-evidence record instead.
3. In `crates/intention-storage-sqlite/src/control_plane.rs`, the span-slicing reader
   `decode_removed_identity_list` (`:3091`) and `decode_removal_evidence` are deleted and
   replaced by canonical codec decoding, so no layer hand-parses JSON.
4. Where a column still stores text, it stores the canonical encoding produced by the codec
   and the reader uses the same codec; no free-form JSON string crosses the DTO boundary and
   no hand-written parser survives. The `SelectionJson` and `SafeProjectionJson` helper
   surfaces are folded into the typed conversion.
5. The storage crate header contradiction recorded in `P3-14` is resolved in the same change:
   after the replacement, the boundary statement and the field list agree.

**Tests.** Round-trip each record through the typed writer and reader; the escaped-identity
fixture required by `P3-15` becomes the regression test for the removed decoder (it must fail
before the change and pass after); malformed content is rejected at admission; a boundary test
asserts no `*_json` string field remains in the storage DTO surface. Run
`cargo nextest run -p intention-storage -p intention-storage-sqlite -p intention-application --tests`
and `make quick`.

**Documentation.** `crates/intention-storage/src/lib.rs` header, `architecture/02` and
`architecture/04` boundary text, and the EVD-060 note about removed `serde_json::Value`
violations extended to the removal of the string fields.

**Sequencing.** D-07 runs after D-01, because the selection record is one of the replaced
fields.

### G.8 D-08 (`P3-07`) — decode-time validation for the control-plane families

**Chosen option:** Option 1 plus deserialize. Every invariant-bearing control-plane DTO
decodes through a private raw shape whose `Deserialize` validates.

**What changes.**

1. For each new control-plane family, replace the derived `Deserialize` with a private raw
   struct plus a manual `Deserialize` that calls `validate()` and maps the typed error through
   `de::Error::custom`, mirroring the established pattern in the same crate
   (`ProtocolHelloDto`, `RunLiveBatchDto`, `RunSnapshotFrameDto`, `RunStreamFrameDto`,
   `RunSubscriptionResponseDto`, `ProtocolAcceptedDto`, `SessionSnapshotDto`,
   `SessionEventTailBatchDto` in `crates/intention-protocol/src/lib.rs`).
2. The families to convert are the invariant-bearing ones, including
   `ProviderCatalogPageDto` (`crates/intention-protocol/src/contract_families.rs:1787`) and
   the DTOs carrying `schema_version` at `:1788`, `:1857`, `:1955`, `:2095`, `:2167`,
   `:2531`, `:3327`, `:3355`, `:3389`, `:3553`, and `:3583`, together with their nested entry
   types.
3. Because validation now runs at decode, the exact-current-version check required by `P2-03`
   lives inside `validate()` and is therefore enforced on the decode path as well as at
   admission; the two findings are implemented as one change.
4. The client no longer needs a separate validation step for decoded projections, because the
   decode cannot produce an invalid value. The misleading comment at
   `crates/intention-client/tests/session_selection_client.rs:640-645`, which claims
   decode-time validation that did not exist, is corrected as part of the same change.
5. `P3-08` is executed with this decision under accepted decision D-14 (Appendix G.11.4), because
   the family being converted here carries the field it concerns:
   `ConfigurationProjectionDto.reload_status` (`crates/intention-protocol/src/contract_families.rs:3589`)
   is a bare `String` whose only production value is the literal `"active"`
   (`crates/intention/src/lib.rs:1164`) and which is validated as arbitrary bounded text at
   `:3606` and `:3616`. The closed vocabulary is expressed as an enum in this same change rather
   than left as a string no consumer can match exhaustively.

**Tests.** Decode-time rejection fixtures that construct raw JSON directly (blank or over-long
text, duplicate or unsorted catalog entries, inconsistent `has_more` and token, wrong schema
version, credential-shaped value) asserting the typed error code, plus round-trip positives
for each family. Run
`cargo nextest run -p intention-protocol -p intention-client -p intention-daemon --tests` and
`make quick`.

**Documentation.** `architecture/02` states the boundary validation rule: structural shape on
decode, semantic invariants at decode and admission, with the raw-shape plus `Deserialize`
pattern as the mechanism.

### G.9 D-09 (`P3-20`) — the fabricated profile revision disappears

**Chosen option:** Option 2 as the primary choice. The field becomes optional and is absent
until the catalog is genuinely wired into the health path.

**What changes.**

1. `ProviderHealthEvidenceDto.provider_profile_revision_id` becomes optional
   (`Option<String>`) and is `None` on the health path, because the catalog is not wired into
   that path.
2. `deterministic_profile_revision` and the `"health-profile-<hex>"` construction
   (`crates/intention-application/src/provider_control_plane.rs:721`, used at `:499`) are
   deleted, so no value with the shape of a catalog identity is ever fabricated.
3. The protocol DTO field, its validation, and the client fixture
   (`crates/intention-client/tests/control_plane_client.rs:250`, which mirrors the fabricated
   value) are updated, and ADR 0037's field table records the optional field.
4. Consumers handle absence explicitly. Correlation by provider identity remains available.
   Note for the same change: `ProviderHealthEvidenceDto.profile_id` currently carries the
   provider identity (`profile_id: provider_id.clone()` at `:499`), so the field name claims a
   profile identity it does not hold. Either the field is renamed to match what it carries or
   it is resolved from the catalog; leaving the name as-is while the revision field becomes
   absent would leave two misleading identity fields in one DTO.
5. When the catalog is later wired into the health path, the field is populated from the real
   active profile revision and the tests from option 1 become applicable; that future step is
   recorded as a follow-up, not implemented now.

**Tests.** Assert the health evidence carries no profile revision until the catalog is wired;
assert the absence is explicit rather than a placeholder. The non-authority assertions that
`P3-23` calls out are owned by Package F item F2 (Appendix D.1) and are implemented there, since
this change touches the evidence DTO; this decision requires that the EVD-051 wording then
matches the strengthened evidence. Run
`cargo nextest run -p intention-application -p intention-client -p intention-protocol --tests`
and `make quick`.

**Documentation.** `architecture/25`, ADR 0037 field table, EVD-051 wording.

**Related item.** `P3-23` → Package F, item F2.

### G.10 D-10 (`P3-28`) — render from the snapshot AST inside `intention-config`, keep the round trip

**Chosen options:** (1a) plus (2) plus (3), all three.

**What changes.**

1. **(1a) Ownership moves to `intention-config`.** Document construction moves out of the
   composition and into the configuration crate, which the ownership map already assigns to
   configuration reload and editing.
2. **Dependency constraint that shapes the interface.** `intention-config` depends only on
   `intention-domain` and `intention-types` (verified in its manifest), so it cannot accept
   `ConfigurationEditOperationDto` from `intention-protocol`. The configuration crate
   therefore gains its own edit-operation type (a key path plus a typed value, or a closed
   operation enum), and the composition maps the protocol DTO into that type. The mapping is
   the only remaining protocol-aware code in the composition.
3. **(3) Render from the snapshot as a TOML AST.** The document is built from the full safe
   snapshot through the TOML value representation and serializer rather than by interpolating
   five variables into a format string. Consequences: values containing quotes or backslashes
   are escaped by the serializer instead of producing an unparseable document; configuration
   fields the edit does not touch are preserved instead of being dropped, so a future field is
   not silently lost; a value the configuration shape cannot represent produces a typed edit
   error instead of a generic parse failure.
4. **(2) The server-side round trip is kept.** The rendered candidate still flows through
   `ConfigurationReloadService::prepare` and `parse_candidate` and
   `reject_catalog_affecting_edits`, so typed edits and raw TOML edits share one validation
   path, and `restore_credential_document` keeps inserting the private credential as a TOML
   value before validation. The credential never appears in the rendered text.
5. `edited_configuration_toml` and its `format!` interpolation in
   `crates/intention/src/lib.rs` are deleted; the composition calls the configuration crate.
6. `P3-12` is executed with this decision, because it is the same helper in the same file:
   `restore_credential_document` (`crates/intention-config/src/control_plane.rs:741-757`) returns
   `text.to_owned()` when the document is not a table, has no `provider` table, or fails to
   re-serialize, so a document-shape failure reaches the caller as a credential-free document and
   is reported downstream as `missing_provider_credential`. It returns `DtoResult<String>` and
   fails with a typed validation error on those branches, which lets a caller distinguish "no
   credential configured" from "this path cannot edit the document". The redaction guarantee is
   unchanged and the tests that currently pin the pass-through behaviour
   (`crates/intention-config/tests/m5_control_plane_config.rs:849`) are rewritten to assert the
   typed error.

**Tests.** Values with quotes and backslashes round-trip correctly; a configuration field not
mentioned by the edit survives the edit; a non-representable value yields a typed edit error;
the credential never appears in the rendered document or in any error. Run
`cargo nextest run -p intention-config -p intention --tests` and `make quick`.

**Documentation.** `architecture/09` configuration editing, the ownership and dependency map
row for configuration reload and editing, and ADR 0037's typed-edit description.

### G.11 Decision briefs D-11 to D-16 (coverage audit, accepted 2026-09-24)

Six items could not be assigned to an implementable package because each had two or more
materially different outcomes. Each was put to the owner as a brief on 2026-09-24, and all six
were accepted as recommended. Every brief below therefore records the **accepted option** first,
then the constraints verified in the code, then the alternatives that were not taken, so the
implementation can proceed and a later reader can see what was rejected and why.

#### G.11.1 D-11 (`P2-04`) — the reconciliation page cursor

**Question.** Should `ReconcileUnavailableQueueCommandDto.page_cursor` be honoured, or deleted?

**Accepted option: (b).** Delete the request field together with its validation and its
credential check, and keep the response cursor. The command then has exactly one execution path,
and the durable reconciliation marker remains the single paging authority. No wire command or
storage column is added.

**Verified constraints.**

- The request field exists at `crates/intention-protocol/src/contract_families.rs:2392` and is
  validated as bounded text plus a credential check at `:2406` and `:2411`.
- No production code reads it. `crates/intention-application/src/session_selection.rs:586-605`
  builds `ReconcileUnavailableQueueInputDto { now, operation_id, max }` and derives the response
  cursor from `load_queue_reconciliation_marker(...).next_page_cursor`.
- Every call site passes `page_cursor: None`: `crates/intention-daemon/src/lib.rs:2394`,
  `crates/intention-client/tests/session_selection_client.rs:465`,
  `crates/intention-application/tests/m5_session_selection.rs:2517`,
  `crates/intention-protocol/src/lib.rs:2005`, `:2117`, `:2250`, `crates/intention/src/lib.rs:6031`, `:6168`.
- The response cursor is real and read: it comes from the durable marker column
  `next_page_cursor` (`crates/intention-storage/src/lib.rs:1261`, read at
  `crates/intention-storage-sqlite/src/control_plane.rs:2882-2907`) and is asserted at
  `crates/intention-application/tests/m5_session_selection.rs:2536`. So the marker, not the
  request cursor, is the paging authority today.
- The only production insert of a reconciliation marker writes the cursor as `NULL`
  (`crates/intention-storage-sqlite/src/control_plane.rs:2794`:
  `INSERT INTO unavailable_queue_reconciliation_markers(...) VALUES (?1,?2,'promotion_exhausted',NULL,NULL)`),
  so in production the response cursor is absent and no cursor can be fed back. Honouring the
  request cursor is therefore a feature addition, not a correction of existing behaviour.
- Protocol fixtures that exercise the request cursor are
  `contract_families.rs:6405`, `:7016`, `:7716` and `tests/control_plane_contracts.rs:66`.

**Options considered.**

- **(a) Honour the cursor.** Pass it into `ReconcileUnavailableQueueInputDto`, and reject a stale
  or unknown cursor with a typed error. This introduces a second paging authority beside the
  durable marker, so the marker and the cursor must be kept consistent, and the semantics of a
  cursor that is not the marker's must be defined (what makes a cursor stale?). **Not taken:**
  the marker already carries the durable paging state, the request field has never been produced
  by any caller, and ADR 0038's minimum-surface rule applies to a field no producer supplies and
  no reader consumes. If a cursor-based loop is genuinely planned later, (a) must state in the
  ADR which of the two authorities wins on a mismatch.
- **(b) Delete the request field** together with its validation, and keep the response cursor.
  **Accepted.**

**Tests.** Assert the wire shape no longer carries the field, and move the fixtures that build it
(`contract_families.rs:6405`, `:7016`, `:7716`, `tests/control_plane_contracts.rs:66`) to the
response-cursor assertions that stay. If (a) is ever revisited, the required test is a double
reconciliation that feeds the first response cursor back and asserts continuation or a typed
rejection.

**Documentation.** ADR 0037's reconciliation command description, and the queue-reconciliation
row of `architecture/29` if it describes a paging loop.

**Interaction.** Independent of D-01 to D-10 and D-12 to D-16; touches the same command family as
`P3-17`, so the two should be implemented in one change to avoid two passes over the
removal-command surface.

#### G.11.2 D-12 (`P2-16`) — SDK error classification by HTTP status

**Question.** Should SDK errors be classified by HTTP status, and should the pre-existing defect
be fixed in this change or filed separately?

**Accepted option: (a).** Retryability is derived from `error.status_code` (429 and 5xx
retryable, every other 4xx non-retryable), the `type` string stays only as a secondary signal,
and the pre-existing defect is fixed in this change, because PR #36 already owns this file's SDK
graph and fixtures.

**Verified constraints.**

- `map_openai_error` at `crates/intention-provider-generic-chat/src/lib.rs:684-687` derives
  retryability from `error.api_error.r#type` and treats a missing type as retryable:
  `matches!(error.api_error.r#type.as_deref(), None | Some("rate_limit_exceeded" | "server_error"))`.
- The `async-openai` 0.42 `ApiErrorResponse` in the same match arm exposes
  `status_code: reqwest::StatusCode`, which the mapping never reads.
- The identical expression exists on `origin/main`, so the defect is pre-existing and this PR
  did not introduce it. The PR does refresh the SDK (`async-openai` 0.42.0) and its fixtures,
  which is why the area reviewer examined the mapping.
- The consequence is retryability, not data loss: a permanent 4xx without a `type` is retried to
  the attempt maximum, and a 5xx with a non-matching type is labelled non-retryable.
- The live channel asserts normalized failure classes for an invalid credential, so a
  classification change is observable there.

**Options considered.**

- **(a) Classify by `status_code`** (429 and 5xx retryable, other 4xx non-retryable), keep the
  `type` string as a secondary signal, and fix it in this change. **Accepted.** The
  classification decides how long the runtime retries a permanent rejection, and the fix is a
  small local change with unit-level verification.
- **(b) The same fix, filed as a separate change**, because the defect is pre-existing and the
  owner may want PR #36 to keep its declared scope. **Not taken:** it was a legitimate scope
  choice, but the owner accepted the fix here rather than leaving the defect open with an
  explicit owner and target.
- **(c) Leave the mapping and document the current behaviour** in the provider-adapter
  documentation, accepting that retryability is approximate. **Rejected:** it keeps a permanent
  rejection retried to the attempt maximum.

**Where the change lands.** `map_openai_error` at
`crates/intention-provider-generic-chat/src/lib.rs:684-687` keeps its `OpenAIError::ApiError`
arm, loses the `matches!(... r#type ...)` classification as the primary signal, and reads
`status_code` instead; the `type` string is consulted only for a status the taxonomy does not
classify by itself.

**Tests.** Unit fixtures constructing `ApiErrorResponse` values with an untyped 400, an untyped
500, and a typed `rate_limit_exceeded` 429, asserting the resulting `ProviderErrorDto`
retryability; then `make e2e-real-api` to confirm the live channel still records the same
normalized failure class for an invalid credential.

**Documentation.** The provider-adapter error-mapping section, if one exists, and the failure
taxonomy row that names `generic_chat_provider_unavailable`.

**Interaction.** Independent of every other decision. Because option (b) was not taken, the
finding is closed by this change rather than carried as an open item.

#### G.11.3 D-13 (`P3-02`) — the identifier bound unit and value

**Question.** Should both layers count characters, or should the stricter canonical byte bound be
documented as deliberate?

**Accepted option: (a).** Both layers count characters, and ADR 0037 Appendix A states one number
per identifier field, so the bound identity enforces is the bound the document promises.
Executed with D-01 (Appendix G.1, item 6).

**Verified constraints.**

- The canonical record validates with `value.len()` (bytes) at
  `crates/intention-domain/src/provider_catalog.rs:98-105`, with
  `MAX_PROVIDER_ID_CHARS = 63`, and the test `provider_ids_are_limited_to_63_characters` pins 63.
- The wire contract validates the same fields with `valid_text(field, 256, ...)`, which counts
  characters (`crates/intention-protocol/src/contract_families.rs:158-172`).
- ADR 0037 Appendix A states "up to 256 scalar values" for `profile_id`, `revision_id`,
  `provider_kind_id`, and `model_id`.
- Consequence: one identifier can pass the wire boundary and fail the canonical record, and a
  multi-byte identifier inside the documented character bound is rejected by the domain.
- This surface is the identity surface that D-01 canonicalises, so the bound that identity
  enforces must be the bound the ADR states.

**Options considered.**

- **(a) Count characters at both layers and align the bound value**, with the ADR field table
  stating which number wins (63 characters or 256 characters) for each identifier field.
  **Accepted:** identity is computed over the canonical value, so a unit mismatch between the two
  boundaries means the same logical identifier is two different identifiers depending on the door
  it enters. The number per field is chosen when D-01 executes and is written into the ADR 0037
  Appendix A table in the same change.
- **(b) Keep bytes in the canonical record and document the deliberately stricter canonical bound**
  in the ADR field table, while making the wire bound consistent with it or explicitly recording
  the difference as a documented narrowing. **Not taken:** acceptable only if the byte bound is
  genuinely intended as the contract, in which case the wire bound must stop promising 256
  characters, and nothing in the ADR claims bytes.
- **(c) Leave both and document the divergence** in the ADR without changing code. **Rejected:**
  it documents the inconsistency instead of fixing it.

**Tests.** Fixtures with a multi-byte identifier inside and outside the documented bound at both
layers, asserting consistent acceptance, plus a test asserting the canonical and wire bounds are
equal or explicitly recorded as different.

**Documentation.** ADR 0037 Appendix A field table, which is already in D-01's documentation
list; this is why the finding is executed with D-01.

**Interaction.** Executed with D-01 (Appendix G.1, item 6).

#### G.11.4 D-14 (`P3-08`) — the `reload_status` vocabulary

**Question.** Should `ConfigurationProjectionDto.reload_status` become a closed enum, or be
deleted?

**Accepted option: (a).** The field becomes a closed enum (for example
`ConfigurationReloadStatusDto { Active, ... }`) validated by serde, with the producer, the ADR,
and the fixtures updated; executed inside D-08, which already converts this DTO family to
decode-time validation. The alternative below is recorded because the coverage audit verified it
was available, not because it stays open.

**Verified constraints.**

- The field is a bare `String` at `crates/intention-protocol/src/contract_families.rs:3589`,
  validated only as bounded text with a credential check at `:3606` and `:3616`.
- Its only producer is `crates/intention/src/lib.rs:1164` (`reload_status: "active".to_owned()`).
- **No production reader exists.** A workspace-wide search for `.reload_status` finds only the
  two validation references in the DTO itself and two test assertions
  (`crates/intention-client/tests/control_plane_client.rs:475`, `crates/intention/src/lib.rs:4983`).
- The field is not mentioned in any document under `docs/`.
- Sibling families in the same crate already model closed vocabularies as enums
  (`ProviderCatalogActivationState`, `ProviderReadinessDto`, `ConfigurationCommitOutcomeDto`).

**Options considered.**

- **(a) Closed enum** (for example `ConfigurationReloadStatusDto { Active, ... }`) validated by
  serde, with the producer, the ADR, and the fixtures updated. **Accepted:** the reload lifecycle
  does have closed states, an unchecked string is the one shape no consumer can match
  exhaustively, and D-08 already converts this family to decode-time validation. Folded into D-08.
- **(b) Delete the field** together with its validation, because no production consumer reads it
  and no document requires it; add it back with the consumer that needs it. **Not taken:** it is
  defensible under ADR 0038's minimum-surface rule and the verified absence of any production
  reader means it costs nothing today, but it becomes wrong the moment a client is written
  against the projection, and the enum is cheaper than a later contract change.

**Tests.** A round-trip test for every enum value and a decode rejection for an unknown string;
the two test assertions that read the string
(`crates/intention-client/tests/control_plane_client.rs:475`, `crates/intention/src/lib.rs:4983`)
are updated to the typed value. If (b) is ever revisited, the required test is that the wire
shape no longer carries the field.

**Documentation.** ADR 0037's configuration projection description, and `architecture/09` if it
describes the reload status.

**Interaction.** Folded into D-08 (Appendix G.8, item 5).

#### G.11.5 D-15 (`P3-32`) — the per-round reasoning attachment bound

**Question.** Which bound should the reasoning attachment enforce, and how is an attachment the
DTO cannot represent recorded?

**Accepted option: (a).** The accumulated reasoning echo is bounded per round, and an attachment
the DTO cannot represent becomes a durable typed failed run (`RoundOutcome::Failed` with a
dedicated code) instead of an `Err` out of `execute`, with the per-round bound recorded in
ADR 0041 beside the durable per-fact and per-run bounds.

**Verified constraints.**

- The round's reasoning is accumulated and validated only at round end by
  `AssistantReasoningDto::new` at `crates/intention-runtime/src/lib.rs:1346`, called from
  `:900` and `:997` inside `drive_provider_round`.
- That constructor rejects more than 512 KiB or a control character other than `\n`, `\r`, `\t`
  (`crates/intention-model/src/lib.rs:238-243`).
- The same text was already accepted durably at 512 KiB per fact and 4 MiB per run, and the
  domain's `BoundedText` rejects only above 1 MiB or NUL
  (`crates/intention-tools/src/lib.rs`), so text between 512 KiB and 4 MiB per fact is durable
  but un-attachable.
- The `?` propagates out of `drive_attempt`, so `execute` returns a DTO validation error and the
  run is terminalized by the daemon's outer error path with a validation code rather than a typed
  provider failure fact.

**Options considered.**

- **(a) Bound the accumulated echo per round and record an unrepresentable attachment as a
  durable typed failed run** (`RoundOutcome::Failed` with a dedicated code), documenting the
  per-round bound in ADR 0041 next to the durable per-fact and per-run bounds. **Accepted:** it is
  the only option that keeps the run's terminal state inside the documented failure taxonomy.
- **(b) Truncate or omit the attachment** when it exceeds the representable bound, keeping the
  run alive. **Rejected:** it silently discards reasoning evidence and makes the durable and
  attachable views of the same round disagree, which is the class of inconsistency this review was
  asked to find.
- **(c) Raise the attachment bound to the durable per-fact bound** and move the
  control-character rejection to the durable boundary. **Not taken:** it moves the bound instead
  of removing the mismatch, and the 4 MiB per-run bound still exceeds what a single attachment can
  carry, so the same failure returns for a long round; it is only viable together with a per-round
  budget stated in ADR 0041.

**Tests.** A reasoning fragment that passes the durable bound but exceeds the attachment bound,
asserting a typed failed run fact rather than an `execute` error, plus a control-character case.
Run `cargo nextest run -p intention-runtime -p intention-model --tests`, then `make quick`.

**Documentation.** ADR 0041 (same-run reasoning round trip) must state the per-round echo bound
beside the durable bounds; `architecture/02` if the attachment is described as a boundary DTO.

**Interaction.** Independent of D-01 to D-14. Option (c) was not taken, so the model DTO
constructor and its tests are untouched by this decision.

#### G.11.6 D-16 (`P2-06`, `P2-07`, `P2-15`) — delete or declare the unconsumed surfaces

**Question.** Should the unconsumed protocol and model surfaces be deleted now, or declared as
reserved for a later slice?

**Accepted option: (b).** Each of the three groups is audited against the roadmap's M6 to M9
plan; a group a named slice will consume is declared in the ADR or roadmap with an evidence
anchor, and every group no planned slice claims is deleted together with its unit tests. The
audit result is what decides which group is which, so neither a blanket deletion nor a blanket
declaration is authorised by this decision.

**Verified constraints.**

- `P2-06`: the protocol crate duplicates the reasoning, header, and parser families of
  `intention-model` with no consumer.
- `P2-07`: nine catalog and configuration event DTOs exist only in
  `crates/intention-protocol/src/contract_families.rs` plus its unit tests; no service constructs
  them, no domain event carries them, and no storage or stream path emits them. The application
  module documentation at `crates/intention-application/src/session_selection.rs:1-14` claims the
  services construct them, which no service does.
- `P2-15`: six public model types have no production consumer.
- The roadmap names no control-plane event family and no reasoning family for M6 to M9 in the
  terms these types use, so a declaration of intent cannot be copied from the roadmap as written;
  it would have to be added.
- ADR 0038 requires that an outdated execution path be removed rather than kept readable, and
  this review's criteria treat a surface serving exactly one caller, fixture, or scenario as
  ad-hoc code.

**Options considered.**

- **(a) Delete all three groups now.** The event family and the duplicated model families return
  together with their producer in one later change. **Not taken as a blanket rule:** it is the
  correct outcome for every group the audit finds unclaimed, and that is exactly what the accepted
  option does with those groups.
- **(b) Audit each group against the roadmap's M6 to M9 plan**, declare the genuinely reserved
  ones in the ADR or roadmap with an evidence anchor (the slice that will consume them), and
  delete the rest. **Accepted:** it is the only option that distinguishes a surface reserved for a
  named slice from a surface written speculatively, and the review's criteria ("no ad-hoc
  single-scenario code") do not forbid reserved surface as long as it is declared where a reader
  can find it and carries the test target and coverage tier its declaration promises.
- **(c) Keep all three and declare them all reserved**, accepting that the surface, its
  validation, its coverage obligation, and its documentation stay while nothing consumes it.
  **Rejected:** it fails the ADR 0038 test for every group the audit cannot tie to a planned slice.

**Audit order.** Run the audit for `P2-07` after D-03, because the capability classification
decides whether any event family becomes reachable through the control plane.

**Tests.** For every deleted type, remove its unit tests with it; for every declared type, add the
test target and coverage tier entry that the declaration promises, so the declaration is not
cheaper than the deletion.

**Documentation.** The roadmap slice that claims a reserved type, the ADR 0037 field or event
tables, the ownership map if a type is assigned to a crate's planned surface, and the corrected
application module documentation that currently claims services construct the event DTOs.

**Interaction.** Independent of D-01 to D-15, but the audit for `P2-07` must happen after D-03,
because the capability classification decides whether any event family becomes reachable through
the control plane.

### G.12 Ordering and interaction of the decisions

| Order | Decisions | Reason |
| --- | --- | --- |
| 1 | D-01 | Identity is a prerequisite for the typed replacement of the selection record in D-07 |
| 2 | D-08 with the `P2-03` exact-version check | One change: validation at decode is where the schema-version rule becomes enforceable |
| 3 | D-07 | Replaces the storage boundary fields, including the selection record from D-01 |
| 4 | D-03 with the `P1-02` client fix | The real-client gate test needs the client to advertise the capability first |
| 5 | D-02, D-05 | Both concern catalog lifecycle ordering; D-05 fixes the acceptance order that D-02's startup re-derivation relies on |
| 6 | D-06 | Independent transport change |
| 7 | D-09, D-10 | Independent; D-10 also closes the escaping and field-preservation defects |
| 8 | D-04 | Independent application change |

Findings that remain in Packages A, B, and D and are unaffected by these decisions keep their
earlier assignment; the three findings promoted here (`P3-07`, `P3-20`, `P3-28`) are removed
from those packages and are tracked as D-08, D-09, and D-10 respectively.

**Where Packages E and F and decisions D-11 to D-16 sit in this order.**

| Order | Items | Reason |
| --- | --- | --- |
| with 1 | E1 (`P3-03`), D-13 (`P3-02`) | Both are validator-level corrections on the identity path that D-01 canonicalises, so they must land before the digest is frozen |
| with 4 | E6, E7 (`P3-29`, `P3-27`), D-16 for `P2-07` | The first two are edits inside D-02's own startup path; the `P2-07` audit must follow D-03, which decides whether any event family becomes reachable |
| with 5 | E5 (`P3-13`) | Same storage loader family as the acceptance ordering D-05 changes |
| with 7 | E8 (`P3-12`), D-14 (`P3-08`) | D-14 rides inside D-08's family conversion, and E8 fixes the helper D-10 keeps; one change owns each file |
| after 8 | E2, E3, E4 (`P3-25`, `P3-26`, `P3-04`), D-11 (`P2-04`), D-12 (`P2-16`), D-15 (`P3-32`) | Independent corrections with no dependency on the earlier decisions; D-11 should share its change with `P3-17` |
| after 8 | Package F (F1 to F6) | The evidence hardening should follow the behaviour changes so the strengthened assertions describe the final behaviour, except F3 and F4, which are harness-only and can ship immediately |
| after 8 | D-16 for `P2-06` and `P2-15` | The audit needs the roadmap read that the `P2-07` pass also uses, but nothing depends on its outcome |

No decision is blocked: all sixteen are accepted, and the only sequencing constraint inside the
second round is that `P2-07` is audited after D-03.

### G.13 Consequences for the documents that must move with the code

| Document | Change |
| --- | --- |
| ADR 0037 | Selection identity is the canonical record digest; startup catalog re-derivation; classification lives on the protocol surface; acceptance ordering; optional health revision field; typed-edit rendering owner |
| ADR 0038 | No change required; the decisions remove surface rather than add it |
| `architecture/02` | Boundary validation rule (decode-time validation with the raw-shape pattern) and the removal of opaque JSON string fields |
| `architecture/03` | Reclaim is the only endpoint-removal path and is identity-verified |
| `architecture/04` | Storage boundary no longer carries free-form JSON strings |
| `architecture/09` | Configuration editing is rendered from the snapshot AST inside `intention-config` |
| `architecture/25` | Startup catalog contract and the corrected `catalog_change_requires_restart` guidance |
| `architecture/29` | Usage aggregation per identity; capability classification ownership |
| Ownership and dependency map | Configuration editing owner restated with the new interface |
| Evidence register | EVD-051 (health evidence), EVD-060 (storage boundary), and the rows touched by the catalog and selection changes |
| `quality/architecture.toml` | Test targets added by the new fixtures where a target is new |

Rows added by the coverage audit and decisions D-11 to D-16:

| Document | Change |
| --- | --- |
| ADR 0037 Appendix A field table | The identifier bound as one number per field counted in characters (D-13), and the reconciliation command description without the request cursor (D-11) |
| ADR 0041 | The per-round reasoning echo bound beside the durable per-fact and per-run bounds (D-15) |
| `architecture/02` | The identifier bound as enforced at each boundary (D-13) |
| `architecture/09` | The reload status vocabulary as a closed enum (D-14) |
| `architecture/29` | The durable marker as the single queue-reconciliation paging authority (D-11) |
| Roadmap M6 to M9 sections | The slice that claims each reserved surface kept by D-16, or the record that the group was deleted |
| Provider adapter documentation | The SDK error classification rule by HTTP status (D-12) |
| Evidence register | EVD-063 after F3 and F4 change what the live evidence means; EVD-051 after F2 strengthens the non-authority evidence |

## Appendix H. W1 verification outcome (V1, 2026-09-25)

Five read-only verifiers checked the eight W1 commits against the accepted option text,
each item's own regression test, the removed surface, and the named documents.

| Slice | Items | Verdicts | Verifier findings |
| --- | --- | --- | --- |
| A | D-01, P2-02, D-13, P3-22, P3-24 | implemented, implemented, deviated, implemented, partial | P1-01's profile-digest half unfinished (P2); D-13 bound not aligned (P3); no storage-failure fixture for P3-24's abort paths (P3); guard scans only `crates/**` and exact literals (P3) |
| B | A2, D-08, P2-03, P2-08 | implemented, implemented, partial, implemented | P2-03 fixtures lack a malformed-text case and an admission-path rejection (P3); `architecture/02` generalizes decode enforcement that 31 pre-Slice-2 DTOs still lack (P3) |
| C | E2, E3, A4, F3, F4, F5, F6 | implemented, implemented, implemented, implemented, implemented, implemented, partial | unbounded synchronous harness waits (`client.health`, `send_command`) (P2); F5 invariant vacuous on an empty fact set (P3); post-loop assertions on provider-chosen arguments (P3); a second six-name wire table in the durable discriminator (P3) |
| E | A3, E5, A6 | implemented, implemented, implemented | run-identity lookup failures collapse to permanent not-found and the trait doc omits `storage_unavailable` (P2); NOTE claim "none returns without committing" false for two rejection branches (P3); A6 read-backs miss the touch and profile-change paths (P3); commit `9056b83` overstates the race (P3) |
| F | D-12, P3-30, P3-31 | partial, implemented, implemented | the retryability rule has no documentation anchor (P3) |

### H.1 Adjudications

- **D-13 (deviated).** The accepted option requires both layers to count characters and the
  bound value to be aligned, with one number per identifier field in ADR 0037 Appendix A.
  The shipped change keeps the canonical-record bound at 63 characters against the 256
  characters the ADR already promises (the parent counted characters, so the deviation was
  the bound value 63 versus 256, not a unit mismatch) and documents the divergence, which
  is option (c), explicitly rejected. Repair: the canonical record counts
  characters and enforces **256** for `profile_id`, `revision_id`, `provider_kind_id`, and
  `model_id`, so the number the ADR already promises wins at both layers; the multi-byte
  fixtures assert consistent acceptance at both layers.
- **P3-24 (partial).** Its remediation is a code change (fallible startup calls route through
  `blocked(error.code())`), so the missing fixture is a real gap, not an optional extra.
- **P1-01 (profile half).** D-01 item 3 names only the selection column, but the finding and
  the accepted option cover the canonical profile revision digest too: the durable column is
  still a JSON-text hash while `provider_profile_revision_digest` has no production caller.
  The domain function already exists, so the repair is the storage call site.
- **History correction.** The commit body of `9056b83` claims a lost race that the
  `UNIQUE(workspace_root)` constraint already prevented; the defect was the error mapping.
  The message is not rewritten for wording, and this appendix records the correction.

### H.2 Repair routing folded into W2

| Repair | From | Content | Owner |
| --- | --- | --- | --- |
| R1 | P1-01 | Storage writes the canonical profile revision digest | W2-A |
| R2 | D-13 | Character counting and one aligned bound (256) at both layers; ADR 0037 Appendix A and `architecture/02` state the single number per field | W2-D, with the `architecture/02` sentence on W2-B |
| R3 | P3-24 | Fallible startup calls route through `blocked` and gain their fixtures | W2-A |
| R4 | guard scope | Extend the source guard beyond `crates/**` and record its literal-shape limits | W2-D |
| R5 | P2-03 | Malformed-text version fixture; admission-path rejection test | W2-E (fixture), W2-C (admission test) |
| R6 | `architecture/02` | Scope the decode-enforcement sentence to the control-plane families and record the 31 pre-Slice-2 DTOs as a follow-up card | W2-B |
| R7 | F6 | Bound every synchronous harness wait (transport read timeout or a wrapped call) | W2-F |
| R8 | F5 | Require a non-empty terminal fact set before the invariant loop | W2-F |
| R9 | A4 residue | Keep post-loop assertions off provider-chosen argument text | W2-F |
| R10 | E3 residue | Declare the durable tool-result discriminator table as a separate boundary | W2-A |
| R11 | A3 residue | Map run-identity lookup failures as unavailable, not permanent not-found; update the trait doc | W2-A |
| R12 | A6 | Correct the NOTE's "none returns without committing" claim | W2-C |
| R13 | A6 | Cover the touch and profile-change commit paths with durable read-backs | W2-C |
| R14 | D-12 | Document the SDK error classification rule by HTTP status in the provider adapter documentation | W2-B |
| R15 | P2-03 / `architecture/02` | Keep the schema-version fixtures loud on a version bump | W2-E |

Parked with owners: the daemon-side incoming subscription version check (P2-08 "Consider"),
the 31 pre-Slice-2 invariant DTOs, and the typed tool-name bridge. The
`incompatible_protocol_version` category nuance is closed against the statements W4-F
added: architecture 02 states the category split explicitly (Validation in the protocol
and client, Unavailable in transport) and architecture 03 records the handshake category
as transport-only with the decode-time failure owned by validation, so the two documents
and the code now agree instead of the entry waiting on them.

## Appendix I — Wave V2 verification outcome

Six read-only verifiers (slices A–F, no builds, JSON reports at
`target/verify-w2-{a..f}.json`) checked the committed W2 stack `ab8cfdc..fbc0fb8`.
The controller gates on the same head are `make quick` (1090 passed, 2 skipped,
rustfmt and clippy clean), `make docs-check`, and a live `make e2e-real-api` run
(2 passed in 30.53 s, recorded in EVD-063).

### I.1 Verdicts

| Slice | Commits | Items | Verdict |
| --- | --- | --- | --- |
| A — storage boundary | `5722852` | D-07 (`P2-10`, `P3-21`), D-05 (`P2-13`), A5 (`P2-11`, `P3-15`), `P3-14`, `P3-16`, R1, R3, R10, R11 | all implemented; 4 P3 |
| B — capability gate and hello | `64088cd`, `dd3be15` | D-03 (`P2-05`), A1 (`P1-02`), R14 | implemented; R6 partial (3 P3) |
| C — startup | `fa9f0c8` | E6 (`P3-29`), E7 (`P3-27`), R5b, R12, R13 | implemented; D-02 partial (2 P2, 3 P3) |
| D — domain and codecs | `58ba4a6`, `dd3be15` | E1 (`P3-03`), E4 (`P3-04`), `P2-01`, `P3-01`, `P3-05`, `P3-06`, R2, R4 | all implemented (3 P3) |
| E — queue and session events | `219a8bc`, `dd3be15` | D-11 (`P2-04`), R5a, R15 | implemented; `P3-17` and `P3-18` partial (1 P2, 3 P3) |
| F — transport, harness, runtime | `f5c7b44`, `0cd0a03`, `fbc0fb8` | D-06 (`P2-14`), R8, R9, D-15 (`P3-32`) | implemented; R7 partial (5 P3) |

No P0 or P1 defects were found. Three P2 findings remain (two D-02 residuals,
one `P3-18` delivery question) and 21 P3 findings, all routed below.

### I.2 Adjudications

- **J-01.** D-02's two residuals are in scope now: startup must not deadlock on
  its own durable pending removal until expiry, and endpoint removal must be
  two-directional (R16, R17).
- **J-02.** `P3-18` stands as a workflow-boundary publication: the composition
  validates the committed event and keeps no durable copy, because Slice 2 has
  no durable append seam for the control-plane event family. The concept and
  architecture promises are corrected to that behavior, and durable delivery is
  parked with an explicit anchor (R18, R19); no durable copy is fabricated.
- **J-03.** The harness budget claim is recomputed (about 82 minutes worst case
  on the current constants, not 66) and the CI relation is re-declared with a
  self-test that fails when a constant and the declared budget diverge (R20).
- **J-04.** The `P3-17` provenance constant stays storage-side and is documented
  and guarded rather than re-introduced as a wire field (R22).
- **J-05.** History is not rewritten for commit-message or bundling deviations
  (`64088cd`, `219a8bc`, `9056b83`); each deviation is recorded here instead.
- **J-06.** R6's "follow-up card" pointer is anchored to the register parking
  list rather than left as prose (R23).
- **J-07.** A1's negative path is completed with a real-client gated command
  probe, and the unit assertion names the exact rejection code (R24).
- **J-08.** The domain validator rejects malformed authority hosts instead of
  only empty ones (R25).
- **J-09.** Appendix H's sentence "63 canonical bytes" is corrected: the parent
  already counted characters; the deviation was the bound value (63 versus 256)
  (R27).
- **J-10.** R7's gap is closed or documented with an anchor, `kill_daemon` is
  bounded, and the write-turn fixture reseeds its target file (R28–R30).
- **J-11.** The D-05 residual (a post-commit registry activation can still fail
  on a poisoned lock) is repaired as an invariant path, not documented away
  (R32).

### I.3 Repair routing

| R | Finding | Repair | Owner |
| --- | --- | --- | --- |
| R16 | D-02 residual | Recover a durable pending removal at startup instead of returning `provider_catalog_removal_pending_exists` until expiry | W3-C |
| R17 | D-02 residual | Make the endpoint comparison two-directional so a dropped declared endpoint re-derives | W3-A |
| R18 | `P3-18` / docs | Correct `architecture/29` and `m4plus_concept` to the validated boundary publication with no durable copy | W3-E |
| R19 | `P3-18` / D-16 | Anchor the durable session-event delivery as a declared future slice in the audit | W3-F |
| R20 | harness budget | Recompute the declared budget, keep the step and job relation true, and add a self-test binding constants to the declared budget | W3-G |
| R21 | D-07 residual | Strengthen the opaque-boundary guard (`serde_json::Value`, suffixed and differently named string fields) and fail closed on an unreadable subtree | W3-D |
| R22 | `P3-17` residual | Document the storage-side provenance literal and guard it against wire re-introduction | W3-C |
| R23 | R6 residual | Anchor the pre-Slice-2 DTO follow-up in `architecture/02` to the register parking list | W3-E |
| R24 | A1 residual | Add a real-client gated command rejection probe and assert the exact rejection code | W3-G |
| R25 | E1 residual | Reject non-empty malformed authority hosts (`https://]/v1`, `https://[::1]]/v1`, backslash hosts) | W3-A |
| R26 | D-07 item 5 | Extend the EVD-060 row to the typed boundary it now describes | W3-E |
| R27 | register wording | Correct Appendix H's unit sentence; record the V2 verdicts in the register tail | W3-E |
| R28 | R7 residual | Bound or explicitly document the non-Unix synchronous I/O path with an anchor | W3-G |
| R29 | R7 residual | Bound the `kill_daemon` fallback wait | W3-G |
| R30 | R9 residual | Reseed the write-turn target file so a stale artifact cannot satisfy the effect | W3-G |
| R31 | R11 residual | Align the state-and-config storage trait doc with the snapshot query's actual error mapping | W3-D |
| R32 | D-05 residual | Make the post-commit registry activation infallible or pre-validate before the durable accept | W3-C |
| R33 | E7 residual | Replace the tautological agreement assertion or drive `CompositionDriverRebuildPort` | W3-A |
| R34 | E7 residual | Have the startup fixtures call `open_platform` instead of re-implementing its sequence | W3-A |
| R35 | R4 residual | Fail the source guard closed on an unreadable subtree and record its scan limits | W3-A |
| R36 | V2 uncertainty | Audit the `provider_profile_tombstoned` wire mappings with no remaining producer: declare or delete | W3-F |
| R37 | V2 finding | Collapse the duplicated id-to-kind mapping (config serde versus the composition array) into one owner | W3-A |
| R38 | E6 residual | Pin the private activation signature with a source guard or record the accepted limit | W3-A |

Parked with owners after V2: the daemon-side incoming subscription version check,
the 31 pre-Slice-2 invariant DTOs (R23 anchor; delivery owned by Milestone 9's
decode-time enforcement closure), the typed tool-name bridge, the
`incompatible_protocol_version` category nuance, the durable
`SessionProviderProfileChanged` append layer (R19 anchor; owned by Milestone 6
under the reserved declarations), and the D-09
health-evidence follow-up (ADR 0037: populate
`ProviderHealthEvidenceDto.provider_profile_revision_id` from the real active
profile revision once the catalog is wired into the health path).

## Appendix J — Wave V3 verification outcome

Seven read-only verifiers (JSON reports `target/verify-w3-{storage,harness-docs,audit,config,health,d09,selection}.json`)
checked the committed W2 head plus the uncommitted W3 working tree, and the
controller gates on the frozen tree are `make quick` (1124 passed, 2 skipped),
`python3 quality/check_docs.py`, and a live `make e2e-real-api` run (2 passed in
25.48 s) on the reworked harness.

### J.1 Verdicts

| Slice | Items | Verdict |
| --- | --- | --- |
| storage | F1 (`P3-09`), `P3-10`, `P3-11`, R21, R31, the deviation hunks | all implemented; 5 P3 |
| harness and docs | R20, R24, R28, R29, R30, `P3-19`, `P3-36`, R18, R23, R27 | implemented; R26 partial (1 P2, 3 P3) |
| audit deliverable | the D-16 audit's 22 deletions, 1 declaration, 5 parked items | P2-15 and R36 sound; P2-06 and P2-07 weak (stale line anchors); parked anchors real but three lack a named slice |
| config | D-10, E8, D-14, R17, R25, R34, R35, R38 | implemented; R33 and R37 partial (5 P3) |
| health | D-09, D-14 consumption, F2 | D-14 and F2 implemented; D-09 partial before the controller rename (3 P3) |
| D-09 end to end | G.9 items 1–5 | all implemented after the rename and the documentation follow-up (2 P3, both repaired) |
| selection and usage | D-04, R16, R22, R32, the differ-differ fix, the usage-scoping conclusion, the loader fix | D-04, R22, the differ-differ fix, the scope conclusion, and the loader fix implemented; R16 and R32 partial (2 P2, 3 P3) |

No P0 defects were found. Two P2 implementation gaps remain (R16, R32), one P1
and one P2 are audit and documentation defects (stale anchors, a doc comment
that contradicts R18), and the rest are P3 hardening items.

### J.2 Adjudications

- **K-01.** The D-16 audit's protocol line anchors are stale by about 87 lines
  and one range now covers an unrelated type. W4 relocates every symbol by name
  and signature before deleting; the audit JSON is treated as a symbol list,
  not as a range list (R39).
- **K-02.** The R16 reverted-document case is a real gap, not a policy: a crash
  residue plus a reverted startup document opens gated and neither adopts nor
  rejects the pending removal. It is repaired in W4 (R40).
- **K-03.** The R32 adoption branch keeps fallible steps after its durable
  accept; it is repaired with the same invariant rule the candidate path
  already follows (R41).
- **K-04.** `session_selection.rs` still documents durable append and subscriber
  delivery for the session-profile event, which R18 corrected in the documents.
  The code comment is corrected in W4 (R42); the publication semantics stay as
  J-02 decided.
- **K-05.** EVD-051 cited the client fixture for the "reports no profile
  revision" claim, but only the application test asserted it. The client fixture
  now asserts typed absence, so the citation is true (done in the wave).
- **K-06.** The new D-09 wire assertion now proves the key is present and null
  rather than merely not a string; a future `skip_serializing_if` would fail it
  (done in the wave).
- **K-07.** An absent revision serializes as `null`, matching this DTO's sibling
  optional fields; "absent" is read as an explicit null, not an omitted key.
- **K-08.** W3 commit messages record the W2 bundling deviations instead of
  rewriting history (J-05 stands).
- **K-09.** The `UsageService::by_revision_and_model` collapsing path has no
  production caller today; it stays declared with a pinning fixture rather than
  being deleted, because D-04's identity grouping is its documented contract.

### J.3 Repair routing

| R | Finding | Repair | Owner |
| --- | --- | --- | --- |
| R39 | audit P1 | Relocate every audit symbol by name before deleting; add the warning to the first audit step | W4-A |
| R40 | R16 residual | Adopt or reject a durable pending removal even when the startup document matches, instead of opening a gated platform | W4-B |
| R41 | R32 residual | Remove the post-adoption fallible steps (or pre-validate them before the durable accept) on the adoption branch | W4-B |
| R42 | audit P2 | Correct the `session_selection.rs` port documentation to the R18 publication semantics | W4-C |
| R43 | R20 residual | Bind the harness budget self-test to the structural counts (tool-turn count, per-attempt calls, snapshot reads) instead of scalars only | W4-D |
| R44 | R21 residual | Close the guard evasions (member visibility, non-`json` names, multi-line fields, aliases and generics, `serde_json::Map`, non-`src` paths) or record each as an accepted limit | W4-D |
| R45 | F1 residual | Assert the complete candidate and state column sets (not a subset and a count) in the removal fault fixtures | W4-D |
| R46 | F1 residual | Restore a fixture for the `provider_selection_conflict` branch removed with the dead writer | W4-D |
| R47 | `P3-10` residual | Pin the seed path against a reworded second create statement | W4-D |
| R48 | R25 residual | Reject a non-numeric port and a non-breaking-space host | W4-E |
| R49 | R33 residual | Replace the textual rebuild pin with a driven rebuild assertion or record why the path is unobservable | W4-E |
| R50 | R37 residual | Collapse the second kind enumeration in `intention-config` onto the single owner | W4-E |
| R51 | R38 residual | Extend the activation-signature pin to a differently named public wrapper | W4-E |
| R52 | R30 residual | Note or close the evicted-subscriber race that can satisfy a later attempt's byte check | W4-D |
| R53 | differ-differ P3 | Observe the intermediate adoption in the two-change fixture | W4-B |
| R54 | D-04 P3 | Add negative fixtures for the usage-set bound, duplicates, and unsorted entries | W4-B |

The documentation follow-ups from G.9 (ADR 0037 field rows, the recorded
follow-up, `architecture/25`, EVD-051, and the parking-list anchor) are already
in the wave, so R53 and R54 are the only new slice-level fixtures left in the
repair lane.

### J.4 Live-channel observation (W4 watch item)

The W3 live run recorded in EVD-063 first failed on commit `c9b4c6a` with the
harness's own gap message "the run stream closed after 30 facts" during the
`edit` turn, and the repeated turn on the identical commit observed the
succeeded call (2 passed in 28.61s). The loss is therefore an intermittent
live-channel event rather than a reproducible defect: the harness already
retries each tool turn and distinguishes a model-rejected argument from a
product failure, but it treats a lost stream as a plain retry with no recorded
cause. W4-G keeps this as a watch item and records the cause (a turn-deadline
expiry, a closed subscriber, or a provider stall) in the run report if it
recurs.

## Appendix K — Wave W4 execution outcome

Six implementation cards ran in parallel on the W3 head `5d29ffe` (card letters are
this wave's execution lanes, recorded in `target/w4-{a..f}.json`; they are independent
of the owner letters in J.3). No card committed; the controller reviewed every report,
closed the controller-side items, and owns the wave's commits and gate.

### K.1 Card verdicts

| Card | Scope | Repairs | Verdict |
| --- | --- | --- | --- |
| A — protocol deletions | `crates/intention-protocol` | D-16 steps 1–2, R39 | done: the eight event DTOs and the seven declaration families are deleted after every symbol was re-resolved by name; `SessionProviderProfileChangedEventDto` and `validate_safe_event_fields` are kept with their live producer and consumer; 106/106 tests, clippy clean, zero references to the deleted symbols inside the crate and no cross-crate reference |
| B — model deletions | `crates/intention-model` | D-16 step 4 | done: the six public model types plus the parser constants are deleted (511 lines); the surviving effort, header, and credential-transport types keep their tests; 27/27 tests, clippy clean; the domain `MODEL_CAPABILITY_TAXONOMY_V1` and the protocol copies are distinct owners and were not touched here |
| C — application repairs | `crates/intention-application` | R36, R40, R41, R42, R53, R54 | done: the two tombstone mapping arms and their dead fixture are removed and a workspace-wide grep guard fails if the code returns; `startup()` adopts a rebuilt durable pending removal through the normal acceptance path instead of leaving the platform gated; the adoption branch keeps no fallible step after its durable accept; the port docs state the R18 semantics; a fixture observes the intermediate adoption and three negative fixtures pin the usage-set bound, duplicates, and unsorted entries; 207/207 tests, clippy clean |
| D — harness and storage | `quality/self_test.py`, `intention-storage`, `intention-storage-sqlite`, `intention-daemon` tests | R43, R44, R45, R46, R47, R52 | done: the budget self-test derives the harness's structural counts and three mutation fixtures must trip it; the guard parses struct bodies instead of `pub` lines; the removal fault fixtures compare the complete candidate, state, and audit column sets; the `provider_selection_conflict` branch has its fixture back; the seed path pin normalizes the create statement; the live byte checks bind to the observed run's own succeeded call; storage suites 120/120, clippy clean |
| E — domain and configuration | `intention-domain`, `intention-config`, `intention` | R48, R49, R50, R51 | done: a non-numeric port and a non-breaking-space host are rejected with cross-layer fixtures; a driven credential-rebuild test replaces the textual pin and the unobservable rejection branch is recorded; configuration resolves the kind id through the single owner; the activation pin asserts exactly the private definition and its one call site; 229/229, 51/51, 72/72, clippy clean |
| F — documents | `docs/intention-relay/**` | D-16 steps 3, 5, 7 | done: the roadmap reserves the durable session-event delivery for M6–M9 without renumbering; architecture 22/29, roadmap 11, ADRs 0028/0035/0037, the reconciliation registers, the evidence rows, the exclusion rows, the source-of-truth matrix, and the concept-supersession index no longer claim a deleted declaration; `make docs-check architecture` green |

### K.2 Controller actions

- The R40 adoption semantics made three composition fixtures stale. The controller moved
  them to the new doctrine: `pending_removal_survives_restart_with_durable_material_and_accepts`
  now reads the adoption from the durable repository instead of asserting a gated
  readiness and accepting by command, `reload_during_pending_removal_preserves_the_lifecycle_across_restart`
  proves the reload commit leaves the durable pending row untouched and the restart adopts
  that same row, and `startup_open_accepts_a_second_change_after_adopting_a_durable_pending_removal`
  expects removal revision two as the durable maximum because the second change is an
  ordinary replacement. A new fixture,
  `startup_open_adopts_a_durable_pending_removal_when_the_document_matches`, covers the
  matching-document case the new doctrine made reachable. The stale differ-differ comment
  in `activate_startup_catalog` now names `startup()` as the adopting step. `intention`
  passes 73/73 with clippy clean.
- The `incompatible_protocol_version` parking entry is closed in Appendix H against the
  statements card F added: architecture 02 states the category split explicitly and
  architecture 03 scopes the handshake category to the transport while decode-time
  failures belong to validation.
- R19's declaration now lives in the roadmap's "Reserved declarations carried by M6–M9"
  section, anchored to `m4plus_concept.md`, architecture 29, and R19.
- The R36 documentation side is consistent: ADR 0037 drops the `provider_profile_tombstoned`
  row and `provider_control_plane.rs` no longer names it.

### K.3 Accepted limits

- R44: the struct-body parser closes member visibility, non-`json` names, multi-line
  fields, aliases, `serde_json::Map`, and non-`src` paths, and fails closed; non-`json`
  string carriers, generic alias targets, and macro-generated fields remain accepted
  limits.
- R51: the activation pin stays a source-text guard; the exact-two-reference assertion is
  what catches a differently named public wrapper.
- R52: the byte checks now require the observed run's own succeeded call to name the
  effect file, so an evicted leftover run cannot satisfy them alone; a late unrelated
  workspace write beyond call-identity binding remains possible.
- The W4-D lane records that the budget arithmetic omissions (readiness health calls and
  the write-side `SYNC_IO_TIMEOUT`) stay a W3 P3 outside the repair list.

### K.4 Gate hardening from the W4 run

The first `make verify` on the W4 tree reproduced a flaky `intention-tools` contract
failure already seen once during W3: under the full workspace run (1134 tests)
`execute_formats_success_and_truncates_both_streams` failed after 5.04 s with
`tool_execute_external_effect_unknown`, while the same test passed five consecutive
isolated runs and the whole `intention-tools` package passed 108/108.

Root cause: `READER_DRAIN_GRACE` bounded the post-exit pipe collection to a fixed five
seconds without a progress signal, so reader threads descheduled by the parallel load
were classified as a descendant holding the pipes open and a legitimate command with
large output was reported as an unknown effect. The drain is now progress-aware:
`ProgressReader` counts consumed bytes, an observed increase re-arms the stall window,
`drain_pipes` still returns `Stalled` after a full window without progress, and no drain
outlives the execute deadline. Two latent defects surfaced by the new unit tests were
fixed with it: an absent reader now counts as already collected (before, `drain_pipes`
could never complete when a caller piped only one stream), and a partially collected
pair keeps its gathered side instead of dropping it on a failed match.

A second pre-existing flake blocked the following run:
`intention-storage-sqlite::m4_model_context`
`context_read_never_returns_starting_context_after_concurrent_terminalization` failed in the
`--no-default-features` profile, and the same assertion had already failed once in
`target/pr24-make-verify-2.log` before W4. The fixture inferred "the read observed the
terminalization" from completion timestamps (`writer_finished < reader_finished`), an inference
a descheduled reader thread violates legitimately: it can return a pre-commit snapshot after the
writer committed. The fixture now records whether terminalization was already durable when the
read began (`SeqCst` atomic), validates either outcome's typed code and the coherence of a
starting snapshot, and adds a deterministic post-race read that must fail with
`run_model_context_unavailable` once terminalization is durable.

Verification: `intention-tools` 110/110 including the two new `drain_progress_tests`, each
mutation-proven (removing the re-arm and removing the absent-reader pre-seed each fail
`reader_progress_extends_the_drain_beyond_the_stall_window`), clippy clean; the repaired
`m4_model_context` fixture passes three consecutive runs and fails the stale-read mutation that
removes the `Starting` status check.

### K.5 V4 sweep and residuals

Seven read-only verifier slices re-read all 58 D.1 rows against the W4 head `881dfd4` (clean
tree) and produced 58 `verified` verdicts: Package A 8, Package B 8 + 7, Package C 8 + 8,
Package D and F 11, Package E 8. The aggregated record is `target/v4-report.json`; the seven
delegated sessions could not write into the repository themselves, so they returned their
payloads inline and those payloads remain the detailed evidence behind each row.

The sweep found two text residuals that are fallout of this wave's deletions, and both are
repaired with it: three admission doc comments (application `session_selection.rs` twice,
composition `intention/src/lib.rs` once) still claimed that admission requires a
"not tombstoned" predicate, which the accepted-membership authority no longer evaluates, and
the model test name `reasoning_effort_and_mode_are_closed_and_snake_case` still named the
deleted mode type. `intention-application`, `intention-model`, and `intention` re-ran at
307/307 with clippy clean after the repairs.

Residuals the sweep recorded, all outside the remediation scope of their rows:

- `ReasoningHistoryManifestDto` exists in both `intention-domain` (`src/reasoning_history.rs`)
  and `intention-protocol` (`src/contract_families.rs`) with no production constructor or
  consumer outside tests and the canonical tag registry, while ADR 0037 calls
  `reasoning-history-manifest-v1` wired. It is the same no-consumer class as the D-16
  families but sits outside the audited symbol list; it stays parked as a follow-up with the
  ledger row to correct when it is resolved.
- No committed search guard covers the deleted protocol (P2-06, P2-07) or model (P2-15)
  symbols, or `persist_resolved_run_provider_selection` (P3-11). Only `intention-domain`
  carries `removed_domain_surfaces_do_not_reappear`, so those deletions rest on the search
  performed at this head.
- P1-01: the `ir-profile-v1` revision-id derivation remains in
  `crates/intention-application/src/provider_catalog.rs`; it feeds `profile_revision_id`, not
  the durable digest column, and the competing-digest guard covers only the `ir-selection-v1`
  literal and the provider-selection namespace.
- P2-13: `roll_forward_acceptance` commits the catalog acceptance before the registry rebuild
  on the PR24-004 recovery path; a failure yields typed `Blocked` and is retried idempotently.
  This is explicitly outside the D-05 auto-accept scope.
- P2-08: the optional daemon-side incoming subscription schema-version check stays parked with
  an owner; the guarantee is client-side. P2-03 and P3-07 leave the 31 pre-Slice-2 DTOs,
  including `ForkBaseSnapshotV1/V2`, outside the decode-time version and validation scope,
  documented in architecture 02.
- P3-21: the SQLite column keeps the name `candidate_json` with a storage-owned encode/parse
  pair, which G.7 item 4 permits because no free-form JSON crosses the DTO boundary.
- P3-25: a failed held-run lookup returns early with no distinct log or error surface, so it is
  indistinguishable from a legitimately held run; the remediation did not require one.
- P3-34 and P3-36 rest on code plus logs (no macOS host, and the read-only sweep could not run
  the `features.toml`-swapping fixture), and the opt-in live harness tests were not re-run in
  the sweep.
- Gate provenance: `target/w4-verify5.log` ends at the notices-check step because the
  controller's cancelled wait killed that process group, and it predates the wave's final
  commits; the same tree passed the affected phases, and `target/w4-deps.log` plus
  `target/w4-selftest.log` record the two remaining phases green afterwards.

End of register.

<!-- pr24-review-1: complete -->
