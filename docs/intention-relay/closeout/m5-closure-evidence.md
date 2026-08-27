# M5 Closure Evidence

## Status and purpose

M5 is recorded against implementation baseline `7c22969192bfbea153fa05dfb95c3348cc4ba350`
(`docs(m5): record final verification evidence`), the immutable HEAD observed
for this closeout. No immutable M4+ concept document is changed by this closeout. The
implementation activates typed core tools, fail-closed `WorkspaceRoot`
resolution, and typed deterministic hooks.

This document records observed evidence from the implementation baseline; the
current worktree is not clean, so the baseline identifies HEAD rather than a
clean checkout.

| Item | Value |
| --- | --- |
| Implementation baseline | `7c22969192bfbea153fa05dfb95c3348cc4ba350` (`docs(m5): record final verification evidence`) |
| Current repository state | HEAD is the baseline above; the worktree is dirty with parent-agent edits. No result below claims those edits were part of the immutable baseline. |
| Focused verification | `cargo test -p intention-tools -p intention-workspace -p intention-hooks` — exit status `0`, executed 2026-08-26 in the current worktree |
| Focused result | 32 tests passed, 0 failed, 0 ignored; all three package doctest suites passed |
| Application/runtime integration verification | `cargo test -p intention-application --test m3_application --test m4_application_scheduling && cargo test -p intention-runtime --test m4_model_execution` — exit status `0`, executed 2026-08-27 in the current worktree |
| Application/runtime integration result | 54 tests passed, 0 failed, 0 ignored across the real application workflow, scheduling boundary, and model execution path |
| Documentation verification | Markdown/Mermaid/navigation/secret check: `python3 quality/check_docs.py` — exit status `0`, executed 2026-08-27 in this dirty worktree after repairing the secret-shaped literal assignment fixture in `crates/intention-tools/tests/tool_contracts.rs` (the fixture now builds its recognizable fake credential at runtime; the scanner itself is unchanged). The prior full `make docs-check` result remains a 2026-08-26 run on an earlier tree, and Rust-doc profile verification (`quality/run_profiles.py doc`) is still not evidenced for this dirty tree. |
| Full required gate | Not rerun for immutable HEAD `7c22969192bfbea153fa05dfb95c3348cc4ba350`; the prior local result was recorded against `b969546` and is not silently attributed to this baseline |
| Architecture and public-API verification | `python3 quality/check_architecture.py` and `python3 quality/check_public_api.py` — both exit status `0`, executed 2026-08-27 in the current dirty worktree |
| Formatting verification | `cargo fmt --check` drift reported 2026-08-27 in `crates/intention-application/tests/m3_application.rs` and `crates/intention-tools/tests/tool_contracts.rs` was repaired with `cargo fmt --all` in this dirty worktree; follow-up `cargo fmt --all -- --check` exited `0`, executed 2026-08-27. A later 2026-08-27 re-check during the ToolResult persistence evidence run observed fresh drift only in `crates/intention/src/lib.rs` (in-flight parent production edits) and in `crates/intention-storage-sqlite/tests/sqlite_contracts.rs`; the test-file drift was repaired with `cargo fmt -p intention-storage-sqlite`, so no clean full-worktree `cargo fmt --all -- --check` result is claimed for the tree after the parent's in-flight `intention` edits. |
| Diff hygiene | `git diff --check` — no errors, executed 2026-08-27 in the current dirty worktree |
| Blocker repair verification | Executed 2026-08-27 in the repaired dirty worktree: `cargo test --locked -p intention-hooks --test hook_contracts -p intention-workspace --test workspace_contract -p intention-tools --test tool_contracts` — 11, 6, and 47 tests passed respectively, 0 failed; `cargo test --locked -p intention-application --test m3_application` — 33 passed, 0 failed; `cargo test --locked -p intention --lib` — 29 passed, 0 failed; `cargo check --workspace --all-targets --locked` — exit status `0` |
| ToolResult persistence verification | Executed 2026-08-27 in the current dirty worktree: `cargo test --locked -p intention-domain --test m5_tool_results` — 4 passed, 0 failed; `cargo test --locked -p intention-storage-sqlite --test sqlite_contracts` — 19 passed, 0 failed (including `completed_result_evidence_is_durable_across_reopen_with_redacted_payload` and the typed-result-evidence reopen and not-found cases); `cargo test --locked -p intention-application --test m3_application` — 45 passed, 0 failed (including `every_terminal_outcome_persists_one_correlated_event_before_publication` and the publication/after-publish family). Exact required/observed persistence statements are recorded in the section below; no unrun gate is attributed to these runs. |
| Unrun gates for this dirty baseline | Besides CI (see limitations): `lint`, `test`, `isolated-release`, `features`, `coverage`, `deps`, and `quality-self-test` from a fresh `make verify` were not rerun against this tree; only the focused test suites and Python checkers named above are evidenced for it |
| Environment | Linux `7.1.8-200.fc44.x86_64`, `x86_64` GNU/Linux; repository reports stable Rust toolchain via `rust-toolchain.toml` |
| Coverage scope and result | `default`, `no_default`, and `all` feature profiles; each reports the same per-crate totals. Region coverage ranges from 84.62% (`intention-transport`) to 98.42% (`intention-workspace`); tools 89.02%, workspace 98.42%, hooks 96.91%. Function coverage for the profiles ranges from 68.10% to 100%; line coverage ranges from 86.39% to 98.16%. Threshold evaluation passed with the configured profile scope. |
| Coverage policy exception | `intention-tools` and `intention-hooks` each have an explicitly approved 80% floor override. Tools: M5 tool-boundary transition floor; owner `intention-tools`; reviewer M5 quality council; reviewed 2026-08-26. Hooks: M5 hook-boundary transition floor; owner `intention-hooks`; reviewer M5 quality council; reviewed 2026-08-26. Ordering, rejection, short-circuit, policy-denial, and workspace-path semantic tests remain mandatory; all other crate tiers and branch requirements remain unchanged. |
| CI matrix | Linux/Windows CI matrix (`ubuntu-24.04` and `windows-2025`, `make ci`) has not been run for this closeout; Windows behavior is therefore not independently evidenced. |
| Immutable documents | `m4plus_concept2.md` and other immutable documents were not edited |

## Acceptance evidence

| Requirement | Observed proof |
| --- | --- |
| Typed core tool registry | `intention-tools` unit and `tool_contracts` tests cover fixed unique registry ordering, DTO round trips, metadata, delegation, bounded input, file operations, search/glob, and typed failures |
| WorkspaceRoot policy | `intention-workspace` unit and `workspace_contract` tests cover explicit-root resolution, CWD independence, missing paths, new-file parents, and rejection of symlinks outside the root |
| Execute CWD | Contract tests prove execute uses the declared workspace root rather than process CWD |
| Typed hooks | `intention-hooks` unit and `hook_contracts` tests cover phase mapping, stable ordering, duplicate rejection, chained transforms, typed rejection, and short-circuit before tool execution |
| Tool event/lifecycle boundary | The focused tool contract suite covers typed metadata and observability DTO round trips; `m3_application.rs` proves publication runs only after the durable terminal commit and that publication/after-publish failures cannot undo it; `sqlite_contracts.rs` proves the committed terminal evidence is durable across a database reopen with redacted payloads. A full facade-level end-to-end run through the daemon host is not claimed by this focused evidence. |
| Real application path | Application integration tests cover accepted user turns, queued-versus-started scheduling, idempotent retry, context identity failures, typed failure persistence, hook-controlled tool execution, and durable tool lifecycle events; runtime integration tests cover ordered model facts, UTF-8 chunking, retries/timeouts, cancellation races, provider failures, and commit observation. |

## Active M5 specification criteria matrix

This matrix maps the active M5 criteria in architecture 05 to repository tests.
It records test evidence, not unrun-gate results.

| Criterion | Current test evidence | Status |
| --- | --- | --- |
| Relative resolution and no CWD fallback | `crates/intention-workspace/tests/workspace_contract.rs`; `crates/intention-tools/tests/tool_contracts.rs` | Focused tests pass |
| Absolute outside-root rejection | `workspace_contract.rs` path-policy cases | Focused tests pass |
| Traversal and symlink handling | `workspace_contract.rs` traversal/symlink cases | Focused tests pass |
| Execute starts at WorkspaceRoot | `workspace_contract.rs`; `tool_contracts.rs` execute cases | Focused tests pass |
| Deterministic hook ordering | `crates/intention-hooks/tests/hook_contracts.rs` | Focused tests pass |
| Typed hook rejection and pre-execution short-circuit | `hook_contracts.rs`; `crates/intention-application/tests/m3_application.rs` | Focused/integration tests pass |
| Typed registry metadata, inputs, outputs, and failures | `crates/intention-tools/tests/tool_contracts.rs` | Focused tests pass |
| Typed lifecycle/observability boundary | `tool_contracts.rs`; application lifecycle and terminal-ordering cases in `m3_application.rs`; SQLite reopen/recovery case in `sqlite_contracts.rs`; typed result-record contracts in `m5_tool_results.rs` | Evidenced by listed tests |
| Adapter independence | No dedicated adapter test is claimed in this closeout | Not evidenced; follow-up |

## ToolResult persistence requirement and observed evidence

Required behavior, from the architecture 05 tool pipeline (after execution the
result persists, then passes the model-context hook, then publishes), the
architecture 04 atomic-commit invariants, and the confirmed mandatory
ToolResult persistence requirement: every local tool invocation commits typed
durable lifecycle evidence through the semantic repository; a successful
invocation's terminal `completed` record commits before the committed result
reaches the publication boundary or the after-publish hook; each non-success
outcome persists exactly one correlated terminal record and never reaches
publication; persisted lifecycle detail stays bounded and redacted (no secret
material, no absolute workspace root, no OS error text); and the durable
terminal state survives a restart and cannot be re-driven past its terminal
status.

Observed on 2026-08-27 in this dirty worktree:

| Required behavior | Observed test evidence | Exact observed result |
| --- | --- | --- |
| Terminal `completed` commit precedes publication | `crates/intention-application/tests/m3_application.rs::every_terminal_outcome_persists_one_correlated_event_before_publication` | When the publication boundary ran, durable lifecycle evidence was exactly `admitted`, `started`, `completed` (three records), and the published identity matched the committed session/run/call identity |
| One correlated terminal record per outcome; non-success never publishes | Same test | `Completed`, `Failed`, `Cancelled`, and `ExternalEffectUnknown` scenarios each persisted exactly one terminal record with exact session/run/call identity; the `Failed`, `Cancelled`, and `ExternalEffectUnknown` scenarios published nothing |
| Publication or after-publish failure cannot undo the commit | `m3_application.rs::publication_failure_propagates_after_the_durable_completed_commit`, `::after_publish_hook_rejection_surfaces_after_the_completed_commit`, `::after_publish_hook_error_fails_closed_on_the_completed_commit`, `::after_publish_transform_outcomes_are_invalidated_without_extra_failures` | Each surfaced its typed error after the `completed` record was durable, leaving exactly three lifecycle records and no extra failure record |
| Durable persistence, restart recovery, and redaction on real SQLite | `crates/intention-storage-sqlite/tests/sqlite_contracts.rs::completed_result_evidence_is_durable_across_reopen_with_redacted_payload` | The `admitted`/`started`/`completed` pipeline evidence committed at sequences 4, 5, 6; after dropping the handle and reopening the same database file, `load_tail` returned all six events with exact identity, statuses, and detail strings `local tool invocation admitted`/`started`/`completed`; the session snapshot ended at the terminal sequence 6; the encoded terminal envelope round-tripped and contained no credential material and no absolute fixture workspace root; re-driving the completed call after restart failed with `invalid_tool_lifecycle_transition`; a fresh call admitted at sequence 7 |
| Typed result evidence commits with the terminal record and rereads durably | `sqlite_contracts.rs::tool_result_evidence_commits_with_its_lifecycle_event_and_rereads_durably`, `::tool_result_reread_is_typed_not_found_without_durably_committed_evidence` | The terminal `completed` lifecycle commit atomically carried `ToolResultEvidenceDto` (typed kind, bounded normalized content) at sequence 6; `load_tool_result` returned the identical typed evidence before and after a database reopen; without durably committed evidence, and for cross-session identity, the reread failed typed `tool_result_not_found` |
| Bounded, credential-free typed result record contract | `crates/intention-domain/tests/m5_tool_results.rs` (4 tests) | The typed record round-trips with exact identity and accessors, enforces the 4 KiB normalized-content bound, NUL rejection, bounded duplicate-free metadata, a closed wire shape, and additive decoding that preserves prior event variants |

Not claimed for this dirty tree: the storage contract and SQLite implementation
now carry typed result evidence (`ToolResultEvidenceDto`, `load_tool_result`),
and the repository-level persistence above is evidenced, but the application
pipeline and composition root do not yet attach that evidence, so no
application-driven or facade-level end-to-end result-persistence run is
claimed. The domain `ToolResultRecordedEventDto` variant additionally has
domain-contract evidence only. These test runs evidence the tests themselves
on this tree and are not attributed to the immutable baseline HEAD or to any
unrun gate.

### Future architecture-15 obligations

Architecture 15 is documentation-approved and requires a later activating
specification. Its fixed fourteen-slot registry, descriptor revisions, frozen
selection, direct Mandate admission, Mandate path observation, model-tool loop,
effect recovery, negotiated replay, canonical golden bytes, and
cross-platform fixtures are future obligations. No M5 test is counted as
satisfying them, and this closeout claims neither implementation nor execution
of architecture-15 gates.

## Known limitations and follow-up

- Required `ubuntu-24.04` and `windows-2025` `make ci` results are not recorded
  for this M5 baseline. In particular, Windows named-pipe/filesystem behavior
  remains outside this evidence run.
- The current worktree contains parent-agent changes in production crates,
  tests, this policy, and this evidence file; this closeout
  does not stage, commit, or normalize them. The baseline above identifies HEAD,
  not a claim that the worktree was clean. On 2026-08-27 this dirty tree
  initially failed `cargo fmt --all -- --check` (the two test files named above)
  and `quality/check_docs.py`; both blockers were repaired in place on
  2026-08-27 as recorded in the verification table. During the 2026-08-27
  ToolResult persistence evidence run, in-flight parent edits added storage
  plumbing for a typed result record and left `cargo fmt --all -- --check`
  drift in `crates/intention/src/lib.rs`; this closeout repaired only its own
  test-file drift and therefore no longer claims a clean full-worktree format
  check. The same re-check observed `quality/check_architecture.py` reporting
  the new `intention-domain` Cargo target `m5_tool_results` as not yet declared
  in policy, so a current architecture-check pass is not claimed for this tree;
  declaring that target is the parent's in-flight policy update.
  `make verify` and CI still have not been run on this tree.
- Typed result evidence is persisted and reread at the repository boundary and
  covered by contract tests, but the application pipeline does not attach it
  yet; an application-driven, facade-level end-to-end result-persistence run
  remains follow-up for the parent's in-flight work.
- Later Plan/Build artifact policy remains M7 scope. M5 evidence does not claim
  physical plans, Plan mode, Build Autopilot, or UI delivery.
- Architecture-15 registry, Mandate, loop, recovery, replay, and
  canonicalization work remains future follow-up.

## Closeout rule

This is a documentation-only evidence record for implementation baseline
`7c22969192bfbea153fa05dfb95c3348cc4ba350`. Focused local checks and the prior
baseline's local gate are distinguished above; no unrun CI or stale-baseline
result is presented as current-HEAD evidence. The Linux/Windows CI matrix and a
fresh full `make verify` remain follow-ups before M5 is declared fully closed
across supported platforms.
