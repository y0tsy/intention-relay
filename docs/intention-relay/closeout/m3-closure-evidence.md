# M3 Closure Evidence

## Status

**Local evidence recorded; M3 is not closed.** This record distinguishes the
successful local checks below from the remaining immutable-baseline and
platform-CI evidence. The M3 implementation was verified in an uncommitted,
dirty working tree; planned tests and architecture decisions below are not
execution evidence.

M3 implementation decisions are documented in [Workspace and Crate
Map](../architecture/01-workspace-and-crate-map.md), [Daemon, Transport, and
Adapters](../architecture/03-daemon-transport-and-adapters.md), [Sessions,
Runs, Events, and Storage](../architecture/04-sessions-runs-events-and-storage.md),
[Configuration, Security, and
Observability](../architecture/09-configuration-security-and-observability.md),
and [Test-Driven Delivery and
Verification](../architecture/10-test-driven-delivery-and-verification.md).

## Evidence baseline

| Field | Value |
| --- | --- |
| Evidence worktree SHA | **Pending — an immutable evidence SHA has not been recorded.** |
| Working-tree status before verification | **Uncommitted, dirty M3 working tree.** |
| Verification environment/platform | Local Linux environment; this is not Linux CI evidence. |
| Pinned tool check result | **Pending.** |
| Baseline date/time (UTC) | **Pending.** |
| Evidence recorder | **Pending.** |

## Required command results

Record the literal command, exit status, relevant concise output/artifact path,
and execution date for each applicable check. Do not replace a failed, skipped,
or unavailable command with a claim of success.

| Check | Required record | Result |
| --- | --- | --- |
| Focused M3 suites | Exact commands for application, runtime, storage, SQLite, config/domain/protocol M3 contracts, and composition restart/replay tests. | **Pending.** |
| Iteration gate | `make quick` command, exit status, and output/artifact reference. | **Pending.** |
| Documentation gate | `make docs-check` command, exit status, and output/artifact reference. | Local check recorded below after this update. |
| Full acceptance gate | `make verify` command, exit status, and output/artifact reference. | `make verify` — exit 0; 126 tests passed; executed locally on the uncommitted M3 working tree. This is not a CI result. |
| Tier B coverage | Per-crate figures for `intention-application`, `intention-runtime`, `intention-storage`, and `intention-storage-sqlite`; each must be at least 90%. | `quality/reports/coverage-default.json` line coverage: application 100.00%, runtime 100.00%, storage 95.00%, SQLite storage 96.98%. |
| Feature profiles | Default, no-default-features, all-features, and declared critical combinations. | **Pending.** |
| Architecture/dependency policy | `make architecture` result, including active-crate/config edges and DTO-only public boundary checks. | **Pending.** |
| Dependency/supply-chain gate | `make deps` result as part of `make verify`, if run. Record the reviewed `deny.toml` exceptions: `hashbrown@0.16.1` only for the WASM `rusqlite 0.40.1 -> sqlite-wasm-rs 0.5.5 -> rsqlite-vfs 0.1.1` chain, and `syn@2.0.119` only for its WASM `rusqlite 0.40.1 -> sqlite-wasm-rs 0.5.5 -> wasm-bindgen 0.2.126 -> wasm-bindgen-macro-support 0.2.126` chain. Record the native retained paths `rusqlite 0.40.1 -> hashlink 0.12.1 -> hashbrown 0.17.1` and native proc-macro `syn@3.0.3`, the retained bundled SQLite configuration, and `rusqlite_migration 2.6.0`. Reassess these exact skips whenever the cited chain or target selection changes and before closeout. | **Pending.** |

## M3 acceptance evidence matrix

| Requirement | Required evidence | Recorded result |
| --- | --- | --- |
| Bundled SQLite and migrations | SQLite contract/migration result proves supported `rusqlite_migration` setup and typed future-schema rejection. | **Pending.** |
| Active ownership and dependencies | Architecture result proves active M3 crates and the intended config snapshot dependency edges without a cycle. | **Pending.** |
| Canonical config snapshots | Config/storage/composition result proves credential-free `ConfigSnapshotDto` revision persistence: equal snapshots with the same `ConfigRevisionId` are idempotent, while different snapshots with that ID fail with typed conflict; started/promoted-run selection remains immutable. | **Pending.** |
| Workspace identity | Session creation/projection fixtures prove durable `WorkspaceId` plus declared `WorkspaceRootDto`; M5 containment is not claimed. | **Pending.** |
| Semantic repository atomicity | Fault-injection outcome tests at event, projection, and snapshot stages prove each injected failure rolls back fully: no new projection, event envelope, or per-state snapshot persists. | **Pending.** |
| Event taxonomy and replay | Contract/fixture evidence proves documented M3 event facts, session sequence ordering, and unscoped snapshot/tail replay with typed resync. Matching, nonexistent, and cross-session `run_id: Some` requests must return `HistoryUnavailable` without unfiltered session state; safely represented scoped replay remains deferred. | **Pending.** |
| Stable queue tickets | Queue fixtures prove zero-based, monotonic, never-reused tickets, removal, idempotency, and one active run. | **Pending.** |
| Cancellation and promotion | Runtime/storage fixtures prove `Starting -> Cancelling -> Cancelled` and atomic terminal promotion that preserves the queued turn's durable proposed `RunId`, config snapshot, and revision despite a later daemon config change. | **Pending.** |
| Recovery-before-ready | Durable restart fixture proves unfinished runs become interrupted before readiness and no external work resumes. | **Pending.** |
| Restart-only TOML | Configuration/composition result proves startup-selected snapshots; no live TOML reload is claimed. | **Pending.** |
| AppData state location | Platform-state fixture proves typed failure rather than process-CWD fallback. | **Pending.** |
| Tier B quality | Coverage artifact proves each active M3 Tier B crate meets 90%, including recovery/failure branches. | Local `make coverage` — exit 0. `quality/reports/coverage-default.json` records Tier B line coverage: application 100.00%, runtime 100.00%, storage 95.00%, SQLite storage 96.98%; Tier C `intention` is 85.03%. |

## Remaining closeout evidence

- Record an immutable evidence worktree SHA and baseline metadata.
- Run and record Linux and Windows `make ci` results; no CI pass is claimed here.
- Record the required M2 Windows named-pipe evidence.

## Explicit M3 boundaries retained for later milestones

- M3 provides durable **one-shot replay-only** subscriptions. An unscoped request
  can return snapshot/tail or typed resync; every `run_id: Some` request
  (matching, nonexistent, or cross-session) returns typed `HistoryUnavailable`
  resync and never unfiltered session state, because M3 session-contiguous
  snapshot/tail DTOs cannot safely represent filtered run state. This is not a
  full stream. `@todo(m4-streaming)` defers persistent live streaming,
  post-commit fan-out/buffering, slow-peer policy, and safely represented
  run-scoped replay to M4.
- M3 does not implement model/provider execution, tool execution, timers, or a
  scheduler. Recovery never resumes those external effects automatically.
- M3 persists workspace identity and declared root but does not claim M5
  filesystem containment or process-CWD enforcement.
- M3 applies TOML once at startup. Live reload, configuration-edit UX, and
  restart coordination are not accepted here.
- M3 does not close the broader roadmap milestone until Package 7 supplies the
  evidence fields above from actual execution.

## Exceptions, failures, and follow-up

| Item | Owner | Disposition |
| --- | --- | --- |
| None recorded yet. | **Pending.** | **Pending.** |
