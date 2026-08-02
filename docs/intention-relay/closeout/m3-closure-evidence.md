# M3 Closure Evidence

## Status and purpose

**Closed at immutable implementation baseline
`e0ddc86c2df7f84394a9f15b5b7a331ad3e8b68b`.**

M3 adds durable SQLite-backed sessions, explicit append-only events, snapshots,
queueing, canonical credential-free configuration revisions, recovery-before-
ready, and durable one-shot replay. The baseline passed local `make verify` and
the required Linux and Windows `make ci` matrix before this separate closeout
documentation commit.

M3 implementation decisions are documented in [Workspace and Crate
Map](../architecture/01-workspace-and-crate-map.md), [Daemon, Transport, and
Adapters](../architecture/03-daemon-transport-and-adapters.md), [Sessions,
Runs, Events, and Storage](../architecture/04-sessions-runs-events-and-storage.md),
[Configuration, Security, and
Observability](../architecture/09-configuration-security-and-observability.md),
and [Test-Driven Delivery and
Verification](../architecture/10-test-driven-delivery-and-verification.md).

| Item | Value |
| --- | --- |
| M3 implementation baseline SHA | [`e0ddc86c2df7f84394a9f15b5b7a331ad3e8b68b`](https://github.com/y0tsy/intention-relay/commit/e0ddc86c2df7f84394a9f15b5b7a331ad3e8b68b) (`fix(m3): use platform-native fixture config paths`) |
| Baseline contents | Four M3 implementation commits beginning at `b33db0a`, followed by the focused cross-platform fixture-path correction in `e0ddc86`. |
| Baseline parent | [`58f281bcea533280df5666579588e4bb3a794bfa`](https://github.com/y0tsy/intention-relay/commit/58f281bcea533280df5666579588e4bb3a794bfa) (`docs(closeout): record M2 acceptance evidence`) |
| Local verification command | `make verify` — exit status `0`, executed 2026-08-02T21:12:06Z at the implementation SHA. |
| Local verification environment | Linux `7.1.5-201.fc44.x86_64`, `x86_64` GNU/Linux; `rustc 1.97.1 (8bab26f4f 2026-07-14)`; `cargo 1.97.1 (c980f4866 2026-06-30)`. |
| Required CI command | `make ci` on `ubuntu-24.04` and `windows-2025` at the implementation SHA. |
| Linux CI result | Passed, [`ubuntu-24.04 make ci`](https://github.com/y0tsy/intention-relay/actions/runs/30767414565/job/91548487802), completed 2026-08-02T21:18:20Z. |
| Windows CI result | Passed, [`windows-2025 make ci`](https://github.com/y0tsy/intention-relay/actions/runs/30767414565/job/91548487743), completed 2026-08-02T21:28:29Z. |
| Matrix workflow | [`Quality` run 30767414565](https://github.com/y0tsy/intention-relay/actions/runs/30767414565), successful for the exact implementation baseline SHA. |
| CI coverage artifacts | [Linux quality reports](https://github.com/y0tsy/intention-relay/actions/runs/30767414565/artifacts/8839438065) and [Windows quality reports](https://github.com/y0tsy/intention-relay/actions/runs/30767414565/artifacts/8839507565). |
| Closure documentation commit | This separate documentation-only commit, created after baseline verification and CI completion. |

## Verification configuration

### Pinned tools exercised locally

| Tool | Verified version |
| --- | --- |
| `cargo-nextest` | `0.9.140` |
| `cargo-llvm-cov` | `0.8.7` |
| `cargo-deny` | `0.20.2` |
| `cargo-audit` | `0.22.2` |
| `cargo-udeps` | `0.1.61` |
| `cargo-machete` | `0.9.2` |
| `cargo-outdated` | `0.19.0` |

### Feature profiles and test evidence

The machine-readable profile policy ran default, `--no-default-features`, and
`--all-features` through check, lint, test, doctest, documentation, and
coverage paths. No critical feature combination is enabled. Each nextest run
reported **126 tests passed, 0 skipped**; all doctests passed.

### Coverage evidence

`make verify` produced branch-aware reports at:

- `quality/reports/coverage-default.json`
- `quality/reports/coverage-no_default.json`
- `quality/reports/coverage-all.json`

The active M3 Tier B crates met their 90% line threshold in every required
profile. The baseline quality-gate output records: `intention-application`
100.00%, `intention-runtime` 100.00%, `intention-storage` 95.00%, and
`intention-storage-sqlite` 96.98%. Tier C composition crate `intention` is
85.03%, satisfying its 85% threshold.

## M3 acceptance evidence matrix

| Requirement | Automated proof and recorded result |
| --- | --- |
| Bundled SQLite and migrations | `sqlite_contracts` covers supported/future schema behavior and migration safety. `make verify`, Linux CI, and Windows CI passed with `rusqlite 0.40.1` using the bundled feature and `rusqlite_migration 2.6.0`. |
| Active ownership and dependencies | `make architecture` and `quality-self-test` passed locally and in the green matrix, proving active M3 crate policy, intended config edges, and public DTO boundaries. |
| Canonical config snapshots | Config/storage/composition contract suites prove credential-free `ConfigSnapshotDto` persistence: same revision plus equal snapshot is idempotent; the same revision plus different snapshot returns a typed conflict; started/promoted run selection remains immutable. |
| Workspace identity | M3 domain contracts prove durable `WorkspaceId` with declared `WorkspaceRootDto`. M5 filesystem containment is not claimed. |
| Semantic repository atomicity | SQLite fault-injection tests cover event, projection, and snapshot phases and prove operation rollback with no partial projection, event envelope, or per-state snapshot. |
| Event taxonomy and replay | Durable unscoped snapshot/tail replay-or-resync contracts pass. Any matching, nonexistent, or cross-session `run_id: Some` request returns `HistoryUnavailable` without unfiltered session state. Safely represented scoped replay remains deferred. |
| Stable queue tickets | Queue fixtures prove zero-based monotonic never-reused tickets, removal/idempotency behavior, and one active run. |
| Cancellation and promotion | Runtime/storage fixtures prove `Starting -> Cancelling -> Cancelled` and atomic terminal promotion which keeps the queued turn's proposed `RunId`, configuration snapshot, and revision despite later daemon configuration changes. |
| Recovery-before-ready | Durable restart composition tests prove unfinished work is interrupted before facade readiness and no external work resumes automatically. |
| Restart-only TOML and state location | Composition tests prove startup-selected snapshots, no live reload claim, platform AppData/state selection, and typed failure rather than process-CWD fallback. |
| Tier B quality | Local and CI `make ci` execute coverage policy. The coverage figures above meet every active M3 Tier B threshold, including recovery and failure paths. |
| Windows named-pipe transport | The successful Windows job log explicitly records `intention-transport::transport_integration windows_named_pipe_fixture_binds_negotiates_frames_and_cleans_up` as passed in every required feature-profile test run. |
| Supply chain | Local `make verify` passed locked metadata, notice freshness, `cargo deny`, audit, udeps, machete, outdated, and policy self-tests. The reviewed target-specific duplicate exceptions remain exactly `hashbrown@0.16.1` and `syn@2.0.119` for the documented `rusqlite 0.40.1 -> sqlite-wasm-rs 0.5.5` WASM chains; native bundled SQLite retains `hashbrown 0.17.1` and `syn 3.0.3`. Reassess these exact skips whenever the cited graph or target selection changes. |

## Scope boundaries retained for later milestones

- M3 provides durable **one-shot replay-only** subscriptions. An unscoped
  request can return snapshot/tail or typed resync; every `run_id: Some`
  request returns typed `HistoryUnavailable` resync and never unfiltered
  session state. Persistent live streaming, post-commit fan-out/buffering,
  slow-peer policy, and safely represented run-scoped replay remain
  `@todo(m4-streaming)` work.
- M3 does not implement model/provider execution, tool execution, timers, or a
  scheduler. Recovery never resumes external effects automatically.
- M3 persists workspace identity and declared root but does not claim M5
  filesystem containment or process-CWD enforcement.
- M3 applies TOML once at startup. Live reload, configuration-edit UX, and
  restart coordination are not accepted here.

## Exceptions, failures, and follow-up

| Item | Disposition |
| --- | --- |
| Initial Windows CI run at `6889592f35f36d5bbc24d263507ae11ca3d05dbd` failed because Unix-only `/tmp/...` fixture configuration paths were not absolute under Windows. | Fixed in `e0ddc86` by constructing fixture paths from `std::env::temp_dir()`. The replacement baseline passed local `make verify`, Linux CI, and Windows CI. |
| Target-specific WASM duplicate dependencies `hashbrown@0.16.1` and `syn@2.0.119`. | Retained as narrowly version-pinned, documented `cargo-deny` exceptions. They must be reassessed whenever the cited SQLite/WASM dependency chain or target selection changes. |

## Final baseline recording rule

The immutable baseline above was committed, verified locally with `make verify`,
and accepted by the completed Linux/Windows CI matrix. This closeout commit is
documentation only; later changes must establish their own baseline and
evidence.
