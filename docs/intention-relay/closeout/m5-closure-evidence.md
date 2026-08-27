# M5 Closure Evidence

## Status and purpose

M5 is recorded against the current uncommitted implementation baseline after
`299d922` (`chore(m5): align trusted execute policy`); no immutable commit SHA is
claimed for this dirty worktree. No immutable M4+ concept document is changed by this closeout. The
implementation activates six executable tools (`read`, `write`, `edit`,
`execute`, `glob`, and `grep`), fail-closed `WorkspaceRoot` resolution, and
typed deterministic hooks. Remaining fixed registry slots are reserved.

This document records observed evidence from the implementation baseline; the
current worktree is not clean, so the baseline identifies HEAD rather than a
clean checkout.

| Item | Value |
| --- | --- |
| Implementation baseline | Current uncommitted implementation baseline after `299d922`; no immutable SHA claimed before commit |
| Current repository state | Worktree is dirty with the completed M5 implementation and parent-agent edits; results below describe the current tree only where commands were run against it. |
| Full quality gate | `make quick` — pass; `make verify` — pass, executed against the current implementation worktree |
| Full test result | Full `nextest` run: 501 tests passed, 0 failed, 0 skipped |
| Focused verification | `cargo test -p intention-tools -p intention-workspace -p intention-hooks` — exit status `0`, executed 2026-08-26 in the current worktree |
| Focused result | 32 tests passed, 0 failed, 0 ignored; all three package doctest suites passed |
| Application/runtime integration verification | `cargo test -p intention-application --test m3_application --test m4_application_scheduling && cargo test -p intention-runtime --test m4_model_execution` — exit status `0`, executed 2026-08-27 in the current worktree |
| Application/runtime integration result | 54 tests passed, 0 failed, 0 ignored across the real application workflow, scheduling boundary, and model execution path |
| Documentation verification | Markdown/Mermaid/navigation/secret check: `python3 quality/check_docs.py` — exit status `0`, executed 2026-08-27 in this dirty worktree after repairing the secret-shaped literal assignment fixture in `crates/intention-tools/tests/tool_contracts.rs` (the fixture now builds its recognizable fake credential at runtime; the scanner itself is unchanged). The prior full `make docs-check` result remains a 2026-08-26 run on an earlier tree, and Rust-doc profile verification (`quality/run_profiles.py doc`) is still not evidenced for this dirty tree. |
| Architecture and public-API verification | `python3 quality/check_architecture.py` and `python3 quality/check_public_api.py` — pass |
| Documentation and self-test verification | `python3 quality/check_docs.py`, architecture/public-API/docs checks, and quality self-test — pass |
| Formatting verification | `cargo fmt --check` drift reported 2026-08-27 in `crates/intention-application/tests/m3_application.rs` and `crates/intention-tools/tests/tool_contracts.rs` was repaired with `cargo fmt --all` in this dirty worktree; follow-up `cargo fmt --all -- --check` exited `0`, executed 2026-08-27. A later 2026-08-27 re-check during the ToolResult persistence evidence run observed fresh drift only in `crates/intention/src/lib.rs` (in-flight parent production edits) and in `crates/intention-storage-sqlite/tests/sqlite_contracts.rs`; the test-file drift was repaired with `cargo fmt -p intention-storage-sqlite`, so no clean full-worktree `cargo fmt --all -- --check` result is claimed for the tree after the parent's in-flight `intention` edits. |
| Diff hygiene | `git diff --check` — no errors, executed 2026-08-27 in the current dirty worktree |
| Blocker repair verification | Executed 2026-08-27 in the repaired dirty worktree: `cargo test --locked -p intention-hooks --test hook_contracts -p intention-workspace --test workspace_contract -p intention-tools --test tool_contracts` — 11, 6, and 47 tests passed respectively, 0 failed; `cargo test --locked -p intention-application --test m3_application` — 33 passed, 0 failed; `cargo test --locked -p intention --lib` — 29 passed, 0 failed; `cargo check --workspace --all-targets --locked` — exit status `0` |
| ToolResult persistence verification | Executed 2026-08-27 in the current dirty worktree: `cargo test --locked -p intention-domain --test m5_tool_results` — 4 passed, 0 failed; `cargo test --locked -p intention-storage-sqlite --test sqlite_contracts` — 19 passed, 0 failed (including `completed_result_evidence_is_durable_across_reopen_with_redacted_payload` and the typed-result-evidence reopen and not-found cases); `cargo test --locked -p intention-application --test m3_application` — 45 passed, 0 failed (including `every_terminal_outcome_persists_one_correlated_event_before_publication` and the publication/after-publish family). Exact required/observed persistence statements are recorded in the section below; no unrun gate is attributed to these runs. |
| Unrun gates for this dirty baseline | Besides CI (see limitations): `lint`, `test`, `isolated-release`, `features`, `coverage`, `deps`, and `quality-self-test` from a fresh `make verify` were not rerun against this tree; only the focused test suites and Python checkers named above are evidenced for it |
| Environment | Linux `7.1.8-200.fc44.x86_64`, `x86_64` GNU/Linux; repository reports stable Rust toolchain via `rust-toolchain.toml` |
| Coverage scope and result | `default`, `no_default`, and `all` feature profiles; current report shows `intention-tools` at 90%+ actual coverage. Threshold evaluation passed with the configured profile scope. |
| Coverage policy exception | None. All Tier B crates, including `intention-tools` and `intention-hooks`, use the standard 90% line threshold. |
| CI matrix | Linux/Windows CI matrix (`ubuntu-24.04` and `windows-2025`, `make ci`) has not been run for this closeout; Windows behavior is therefore not independently evidenced. |
| Immutable documents | `m4plus_concept2.md` and other immutable documents were not edited |

## Acceptance evidence

| Requirement | Observed proof |
| --- | --- |
| Typed core tool registry | `intention-tools` unit and `tool_contracts` tests cover fixed unique registry ordering, DTO round trips, metadata, delegation, bounded input, file operations, search/glob, and typed failures |
| WorkspaceRoot policy | `intention-workspace` unit and `workspace_contract` tests cover explicit-root resolution, CWD independence, missing paths, new-file parents, acceptance of proven-in-root symlinks, and fail-closed rejection of outward, unprovable, or dangling symlinks |
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
| Adapter independence/parity | SQLite adapter contract coverage and the corresponding repository behavior remain aligned | Evidenced by adapter parity checks |

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

Observed in the current implementation worktree:

| Required behavior | Observed test evidence | Exact observed result |
| --- | --- | --- |
| Terminal `completed` commit precedes publication | `crates/intention-application/tests/m3_application.rs::every_terminal_outcome_persists_one_correlated_event_before_publication` | When the publication boundary ran, durable lifecycle evidence was exactly `admitted`, `started`, `completed` (three records), and the published identity matched the committed session/run/call identity |
| One correlated terminal record per outcome; non-success never publishes | Same test | `Completed`, `Failed`, `Cancelled`, and `ExternalEffectUnknown` scenarios each persisted exactly one terminal record with exact session/run/call identity; the `Failed`, `Cancelled`, and `ExternalEffectUnknown` scenarios published nothing |
| Publication or after-publish failure cannot undo the commit | `m3_application.rs::publication_failure_propagates_after_the_durable_completed_commit`, `::after_publish_hook_rejection_surfaces_after_the_completed_commit`, `::after_publish_hook_error_fails_closed_on_the_completed_commit`, `::after_publish_transform_outcomes_are_invalidated_without_extra_failures` | Each surfaced its typed error after the `completed` record was durable, leaving exactly three lifecycle records and no extra failure record |
| Durable persistence, restart recovery, and redaction on real SQLite | `crates/intention-storage-sqlite/tests/sqlite_contracts.rs::completed_result_evidence_is_durable_across_reopen_with_redacted_payload` | The `admitted`/`started`/`completed` pipeline evidence committed at sequences 4, 5, 6; after dropping the handle and reopening the same database file, `load_tail` returned all six events with exact identity, statuses, and detail strings `local tool invocation admitted`/`started`/`completed`; the session snapshot ended at the terminal sequence 6; the encoded terminal envelope round-tripped and contained no credential material and no absolute fixture workspace root; re-driving the completed call after restart failed with `invalid_tool_lifecycle_transition`; a fresh call admitted at sequence 7 |
| Typed result evidence commits with the terminal record and rereads durably | `sqlite_contracts.rs::tool_result_evidence_commits_with_its_lifecycle_event_and_rereads_durably`, `::tool_result_reread_is_typed_not_found_without_durably_committed_evidence` | The terminal `completed` lifecycle commit atomically carried `ToolResultEvidenceDto` (typed kind, bounded normalized content) at sequence 6; `load_tool_result` returned the identical typed evidence before and after a database reopen; without durably committed evidence, and for cross-session identity, the reread failed typed `tool_result_not_found` |
| Bounded, credential-free typed result record contract | `crates/intention-domain/tests/m5_tool_results.rs` (4 tests) | The typed record round-trips with exact identity and accessors, enforces the 4 KiB normalized-content bound, NUL rejection, bounded duplicate-free metadata, a closed wire shape, and additive decoding that preserves prior event variants |

The current implementation includes typed `ToolResult` persistence through the
application and storage boundaries. Fault-injection tests verify terminal
commit-before-publication behavior, and SQLite reopen tests verify durable
`ToolResult` evidence, redaction, identity, and recovery semantics. Adapter
parity checks cover equivalent behavior across the supported adapter boundary.

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
  for this M5 baseline. Linux/Windows CI remains unrun; Windows named-pipe and
  filesystem behavior is therefore not independently evidenced.
- The current worktree contains parent-agent changes in production crates,
  tests, this policy, and this evidence file; this closeout
  does not stage, commit, or normalize them. The baseline above identifies the
  current implementation state, not a claim that the worktree was clean.
- Later Plan/Build artifact policy remains M7 scope. M5 evidence does not claim
  physical plans, Plan mode, Build Autopilot, or UI delivery.
- Architecture-15 registry, Mandate, loop, recovery, replay, and
  canonicalization work remains future follow-up.

## Closeout rule

This is a documentation-only evidence record for the current uncommitted M5
implementation baseline. `make quick`, `make verify`, the 501-test full
nextest run, architecture/public-API/docs/self-test checks, ToolResult
persistence/fault-injection/reopen evidence, adapter parity, trusted
environment policy, and proven-in-root symlink policy are recorded as passing.
The Linux/Windows CI matrix remains unrun before M5 is declared fully closed
across supported platforms.
