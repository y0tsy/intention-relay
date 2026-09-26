# M2 Closure Evidence

## Status and purpose

**Closed at immutable implementation baseline `4f90e5e67178228d849b3f44239d58968d571c21`.**
M2 adds private local transport, a shared client, daemon bootstrap, an
in-memory fixture composition, protocol extensions, a stateful one-shot
recovery handle, and a TUI proof adapter. The baseline passed local
`make verify` and the required Linux and Windows `make ci` matrix before this
separate closeout documentation commit.

M2 follows the completed [M1+ Quality Hardening Evidence](m1-plus-quality-hardening-evidence.md)
baseline. Its intended architecture and implementation decisions are described
in [Daemon, Transport, and Adapters](../architecture/03-daemon-transport-and-adapters.md)
and [Implementation Roadmap](../architecture/11-implementation-roadmap.md).

| Item | Value |
| --- | --- |
| M2 implementation baseline SHA | [`4f90e5e67178228d849b3f44239d58968d571c21`](https://github.com/y0tsy/intention-relay/commit/4f90e5e67178228d849b3f44239d58968d571c21) |
| Local verification command | `make verify` passed on 2026-08-02 at the implementation SHA |
| Required CI command | `make ci` on `ubuntu-24.04` and `windows-2025` at the implementation SHA |
| Linux CI result | Passed, [`ubuntu-24.04 make ci`](https://github.com/y0tsy/intention-relay/actions/runs/30748331908/job/91497744209) |
| Windows CI result | Passed, [`windows-2025 make ci`](https://github.com/y0tsy/intention-relay/actions/runs/30748331908/job/91497744154) |
| Closure documentation commit | This separate documentation commit, created after baseline verification and CI completion |

## Implemented scope to verify

- cross-platform private local IPC, Unix domain sockets on Unix and Windows
  named pipes on Windows;
- safe logical endpoint identifiers under per-user runtime or application
  configuration locations, with no endpoint filesystem paths in public DTOs or
  safe errors;
- Unix-only endpoint parent `0700` and listener socket `0600` policy;
- private bounded length-prefixed UTF-8 JSON framing with a 1 MiB maximum
  payload, one negotiated correlated request and response per connection;
- cross-platform `fs4` advisory startup locking with initial connection,
  locked recheck, process launch only for unavailable endpoints, and bounded
  readiness polling;
- readiness that requires compatible hello, all M2 capabilities, a correlated
  negotiated-version health response, and `DaemonReadinessDto::Ready`;
- snapshot-tail/resync subscription responses and a stateful recovery handle
  that reconnects over a new one-shot connection using the last accepted
  sequence, without claiming a persistent live stream;
- deterministic test-only fixture session configuration so two clients can
  compare equal daemon-owned health, snapshot, and tail DTOs;
- a TUI production adapter that only uses `intention-client`, with a dev-only
  fixture host proving its successful health and subscription path;
- a required Linux/Windows CI matrix, where Windows executes named-pipe bind,
  hello, framed request/response, and cleanup acceptance coverage.

## Required acceptance evidence matrix

| M2 criterion | Automated proof | Required recorded result |
| --- | --- | --- |
| Workspace quality gate | Exact implementation SHA runs `make verify`. | Local exit status, tool versions, coverage reports. |
| Linux quality gate | `ubuntu-24.04` runs `make ci`. | Green CI job for the implementation SHA. |
| Windows transport | `windows-2025` runs `make ci`; `windows_named_pipe_fixture_binds_negotiates_frames_and_cleans_up` exercises named pipes. | Green CI job for the same SHA. |
| Endpoint boundary | Unix fixture checks `0700`, `0600`, and owned-socket cleanup; Windows fixture checks named-pipe lifecycle. | Linux and Windows evidence separated by platform. |
| Bootstrap readiness | Client fixtures reject missing capabilities, health/version divergence, non-`Ready` states, malformed response versions, and correlation mismatch. | Pass. |
| Shared daemon state | `daemon_bootstrap` concurrently bootstraps two clients, then compares typed health, snapshot, and subscription tail for one explicit fixture SessionId. | Pass. |
| Recovery | The client's one-shot snapshot-and-tail subscription returns the current durable projection with a contiguous tail or a typed resync; the test-only `SessionSubscriptionRecovery` wrapper was removed under ADR 0038 (no production caller). | Pass. |
| TUI adapter proof | `tui_contract` reaches ready health and the fixture subscription only through `TuiProofClient` → `IntentionClient`; architecture policy rejects a daemon production dependency. | Pass. |
| Tier C coverage | `quality/reports/coverage-{default,no_default,all}.json` meet every active M2 Tier C threshold. | Local default: transport 87.06%, client 89.39%, composition 94.07%, daemon library 89.60%; thin daemon `main.rs` is the sole reviewed exclusion because Windows `llvm-cov` does not merge child-process instrumentation, while `daemon_bootstrap` exercises the real binary. All ≥85%. |
| Supply chain | `cargo deny`, audit, udeps, machete, outdated and locked metadata run under `make verify`. | Pass. |

## Scope boundaries and deferred work

M2's composition facade is intentionally in-memory and non-durable. It proves
the local protocol/client/daemon path without claiming durable application
behavior.

- **M3 owns:** SQLite sessions, append-only events, durable snapshots and event
  tails, queues, retention, restart recovery, durable subscription replay, and
  persistent streaming semantics.
- **M4 owns:** model/provider drivers, streaming execution, and live run
  lifecycle behavior.
- **Deferred transport/lifecycle decisions:** idle shutdown, explicit daemon
  stop, upgrades with connected adapters, subscription buffer sizing, live
  stream shape, read/write deadlines, and slow-client eviction.

## Final baseline recording rule

The immutable baseline above was committed, verified locally with `make verify`,
and accepted by the completed Linux/Windows CI matrix. This closeout commit is
documentation only; later changes must establish their own baseline and evidence.
