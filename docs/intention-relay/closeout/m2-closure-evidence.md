# M2 Closure Evidence

## Status and purpose

**Pending immutable implementation baseline and required CI evidence.**
M2 adds private local transport, a shared client, daemon bootstrap, an
in-memory fixture composition, protocol extensions, a stateful one-shot
recovery handle, and a TUI proof adapter. This document is deliberately not a
closure claim until the implementation commit identified below has passed
`make verify` and the Linux and Windows `make ci` matrix on that same SHA.

M2 follows the completed [M1+ Quality Hardening Evidence](m1-plus-quality-hardening-evidence.md)
baseline. Its intended architecture and implementation decisions are described
in [Daemon, Transport, and Adapters](../architecture/03-daemon-transport-and-adapters.md)
and [Implementation Roadmap](../architecture/11-implementation-roadmap.md).

| Item | Value |
| --- | --- |
| M2 implementation baseline SHA | Pending implementation commit |
| Local verification command | Pending `make verify` at the implementation SHA |
| Required CI command | `make ci` on `ubuntu-24.04` and `windows-2025` at the same SHA |
| Linux CI result | Pending |
| Windows CI result | Pending |
| Closure documentation commit | Pending, created only after implementation verification and CI complete |

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
| Recovery | `SessionSubscriptionRecovery` uses its recorded last sequence on a fresh request connection and clears state on typed resync. | Pass. |
| TUI adapter proof | `tui_contract` reaches ready health and the fixture subscription only through `TuiProofClient` → `IntentionClient`; architecture policy rejects a daemon production dependency. | Pass. |
| Tier C coverage | `quality/reports/coverage-{default,no_default,all}.json` meet every active M2 Tier C threshold. | Reported actual values, all ≥85%. |
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

Before calling M2 immutable, commit the reviewed implementation change set,
record the exact SHA, rerun `make verify` from that commit, and wait for both
required `make ci` jobs to succeed on that same SHA. Only then replace every
pending value above with observed evidence and create a separate closeout
documentation commit, modeled after M0/M1 closeout practice. No dirty-worktree
result, unavailable platform, cross-compile-only check, or inferred pass may
be recorded as acceptance evidence.
