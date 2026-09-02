# Intention Relay

Intention Relay is a local-first, single-user coding-agent system being built
as a Rust workspace. A standalone daemon process owns the application runtime
and all durable state; desktop (Tauri) and terminal (TUI/REPL) presentations
are planned adapters over one typed local protocol and one shared Rust client.
The project is under active development: the daemon-side backend is
implemented through the closed M0-M5 milestones, post-M5 foundation work is in
progress, and no user-facing UI or released product exists yet.

Everything here is development-machine software: there are no deployed users,
no externally persisted data, and no third-party consumers. Backward
compatibility is neither required nor in demand (see `AGENTS.md`).

## What it does

- **Daemon-owned sessions and runs.** A user project maps to durable sessions;
  an accepted user turn can start one agent-execution run with a tracked
  lifecycle (`Starting`, `Running`, `Cancelling`, `Failed`, `Cancelled`,
  `Interrupted`, ...), durable event history, and replay.
- **One typed protocol, one shared client.** All adapters reach the daemon
  through `intention-client` over a private, per-user local transport (Unix
  domain sockets on Unix, named pipes on Windows). DTOs are the only things
  that cross crate, process, and persistence boundaries.
- **Model-driven runs with workspace tools.** Provider-neutral model drivers
  (OpenRouter and generic OpenAI-compatible Chat Completions) stream into a
  run loop that can call typed, `WorkspaceRoot`-bounded tools (`read`,
  `write`, `edit`, `execute`, `glob`, `grep`) with deterministic typed hooks
  and durable, redacted tool-result evidence.
- **SQLite-first durable state.** One composition crate selects the SQLite
  adapter; sessions, events, snapshots, queue tickets, model facts, and tool
  evidence live in a single-version schema, with recovery-before-readiness on
  daemon start.
- **Security posture.** Single-user, endpoint and state kept under the current
  user's platform directories; fail-closed path, symlink, and configuration
  policies; credentials stay in the private config layer and never appear in
  public DTOs, errors, events, snapshots, or logs. These are logical product
  controls in a trusted-local process, not an OS sandbox.

## Implementation status

Milestones M0 through M5 are closed with recorded evidence; each milestone
below is detailed in its closure document under
[docs/intention-relay/closeout/](docs/intention-relay/closeout/).

| Milestone | Status | Scope |
| --- | --- | --- |
| M0 Quality foundation | Closed | Reproducible Makefile-orchestrated quality pipeline (format, lint, feature profiles, nextest, docs, architecture, coverage, supply-chain gates) with pinned tools and a metrics manifest. |
| M1 Contracts, configuration, workspace skeleton | Closed | Tier-A crate boundaries (`intention-types`, `intention-domain`, `intention-protocol`, `intention-config`), DTO-first policy, TOML config with redacted projections, compile-only skeletons for every later crate. |
| M1+ Quality policy hardening | Closed | Machine-readable policies (`quality/*.toml`) enforce workspace dependency graphs, executable test targets, public-API surface, coverage tiers, and feature profiles. |
| M2 Local protocol, client, daemon bootstrap | Closed | Private local IPC, hello/negotiation, correlated request/response codec, shared bootstrap client with startup lock and readiness polling, in-memory fixture composition, minimal TUI proof adapter. |
| M3 SQLite sessions, events, snapshots, queue | Closed | Durable SQLite-backed sessions, append-only events, snapshots, turn queueing, canonical credential-free config revisions, recovery-before-ready, durable one-shot replay. |
| M4 Model contract, providers, one streaming run | Closed | Provider-neutral model contracts and validated stream facts; private OpenRouter and generic Chat Completions drivers; durable model facts; one daemon-owned streaming run with reconnect/replay and run-scoped delivery. |
| M5 Typed tools, WorkspaceRoot, hooks | Closed | Production model-tool loop hosted by the real daemon binary: six executable tools, fail-closed `WorkspaceRoot` resolution, deterministic typed hooks, durable and redacted tool-result evidence, daemon-host end-to-end tests on Linux and Windows. |
| M5+ Post-M5 alignment | **In progress** | Accepted activation home for the post-M5 stack (ADR 0035) delivered as four slices: 1) contracts and versions, 2) control plane, 3) harness, 4) UI foundation. Slice 1 (ADR 0036) is merged into `main` (canonical execution-meaning codec and digest fixtures, negotiated capability families and contract-family DTOs, storage schema-3 preservation). Slices 2-4 are not yet implemented. |
| M6-M9 | Planned | M6 Tauri bridge and primary desktop UI; M7 Plan/Build policies, physical plans, and Build Autopilot; M8 VFR and Headroom; M9 hardening and acceptance verification. See the [implementation roadmap](docs/intention-relay/architecture/11-implementation-roadmap.md). |

Everything beyond M5 is roadmap direction recorded in
[architecture](docs/intention-relay/architecture/README.md) and
[decision records](docs/intention-relay/decisions/README.md), not delivered
behavior. No roadmap document implements or silently supersedes the closed
M0-M5 behavior.

## Crate map

All workspace crates live under [crates/](crates/) unless noted. Coverage
tiers A (95%), B (90%), and C (85%) are enforced by the machine-readable
policy in [quality/coverage.toml](quality/coverage.toml).

### DTO foundations (Tier A, active)

| Crate | Responsibility |
| --- | --- |
| [intention-types](crates/intention-types) | Shared, dependency-light DTOs: validated identifiers, schema versions, safe errors, time, pagination, event envelopes, model/tool value DTOs. |
| [intention-domain](crates/intention-domain) | Domain DTOs and value validation, commands/queries, domain events, run modes, model facts, tool results, canonical codec and run-execution-meaning records. |
| [intention-protocol](crates/intention-protocol) | Versioned public local-protocol DTOs: handshake/negotiation, command/query wrappers, contract families, negotiated capability DTOs. |
| [intention-config](crates/intention-config) | Versioned TOML parsing, migration, validation, path selection, and credential-free public configuration projections. |

### Durable storage and application core (Tier B, active)

| Crate | Responsibility |
| --- | --- |
| [intention-storage](crates/intention-storage) | DTO-only semantic storage contracts: repository, unit-of-work, snapshots, event log, replay. |
| [intention-storage-sqlite](crates/intention-storage-sqlite) | SQLite-backed durable implementation (single-version schema 3), selected only by the composition crate. |
| [intention-runtime](crates/intention-runtime) | Deterministic run lifecycle decisions, model-loop coordination, cancellation, over DTO-only storage. |
| [intention-application](crates/intention-application) | Workflow orchestration: sessions, turns, scheduling, tool invocation, publication over durable outcomes. |

### Model drivers (Tier C, active)

| Crate | Responsibility |
| --- | --- |
| [intention-model](crates/intention-model) | Provider-neutral model contracts (messages, tool calls/results) and validated stream facts; no SDK or runtime resources. |
| [intention-provider-openrouter](crates/intention-provider-openrouter) | OpenRouter driver; `openrouter-rs` stays a private implementation detail. |
| [intention-provider-generic-chat](crates/intention-provider-generic-chat) | Generic OpenAI-compatible Chat Completions driver; `async-openai` stays private. |

### Tools, workspace, and hooks (Tier B, active)

| Crate | Responsibility |
| --- | --- |
| [intention-tools](crates/intention-tools) | Typed, bounded tool contracts and the fixed registry; `read`, `write`, `edit`, `execute`, `glob`, and `grep` are executable; remaining fixed slots are reserved. |
| [intention-workspace](crates/intention-workspace) | `WorkspaceRoot` resolution and fail-closed filesystem policy (no CWD fallback, symlink containment). |
| [intention-hooks](crates/intention-hooks) | Typed, deterministic hook registration and dispatch around tool execution. |

### Transport, client, and daemon (active)

| Crate | Responsibility |
| --- | --- |
| [intention-transport](crates/intention-transport) | Private per-user IPC: Unix sockets / Windows named pipes, bounded length-prefixed JSON framing (1 MiB frames), hello and negotiation. Tier C. |
| [intention-client](crates/intention-client) | Shared bootstrap, dispatch, subscription, and reconnect client for adapters, with advisory startup lock and daemon launch. Tier C. |
| [intention](crates/intention) | Composition root and durable `DaemonApplicationFacade`; the only crate that selects SQLite and concrete drivers. Tier C. |
| [intention-daemon](crates/intention-daemon) | Daemon host library plus the thin `intention-daemon` binary (the only binary in the workspace). Tier C. |

### Adapter slots and reserved crates

| Crate | Status | Responsibility |
| --- | --- | --- |
| [intention-tui](crates/intention-tui) | Proof adapter (library only, no binary) | Minimal terminal-facing proof over the shared client (connect, subscribe). |
| [intention-tauri](crates/intention-tauri) | Skeleton | Reserved Tauri bridge/UI adapter slot (M6). |
| [intention-vfr](crates/intention-vfr), [intention-headroom](crates/intention-headroom), [intention-plans](crates/intention-plans) | Skeleton | Compile-only placeholders for VFR, Headroom/CCR, and Plan/Build artifact features (M7/M8 scope). |
| [intention-test-support](crates/intention-test-support) | Non-production | Durable integration fixtures and contract scenarios used by tests. |
| [quality/harness](quality/harness) | Non-production | Workspace member proving the quality pipeline. |

Architecture rules worth knowing: only the `intention` composition crate
touches SQLite or selects concrete providers; presentation adapters may only
use `intention-client`, `intention-protocol`, and `intention-transport`
boundaries; provider SDKs and Tokio/transport resources stay private to their
owner crates.

## Running the daemon today

The workspace has exactly one binary: `intention-daemon`. It serves a real,
durable daemon over a private per-user endpoint. There is no interactive
client or UI binary yet; today the daemon is driven through the shared
`intention-client` crate, and the working end-to-end examples live in the
daemon integration tests (for example
[crates/intention-daemon/tests/facade_e2e.rs](crates/intention-daemon/tests/facade_e2e.rs),
which spawns the real binary, drives it over real IPC, and executes a real
`read` tool through the production model-tool loop).

Prerequisites for a manual run: a configured provider (see below) and a Rust
toolchain (see [Prerequisites](#prerequisites)).

```text
# Place a valid config file first (see Configuration), then:
cargo run -p intention-daemon                 # default endpoint instance
cargo run -p intention-daemon -- my-instance  # named logical endpoint
```

The daemon resolves its configuration and state from platform-standard
locations, recovers unfinished runs to `Interrupted` before serving, and
prints only safe error codes on startup failure (exit status non-zero). The
endpoint argument is a logical safe instance name; endpoint filesystem paths
never appear in protocol DTOs or errors.

## Configuration

The daemon reads one versioned TOML file, `config.toml`, inside an
`intention-relay` directory under the current user's platform configuration
location: Linux `$XDG_CONFIG_HOME` (default `~/.config`), macOS
`~/Library/Application Support`, Windows `%APPDATA%`. An explicit absolute
path override exists in `intention-config` for fixtures and controlled runs;
there is no working-directory fallback.

Minimal shape (schema version 1), using placeholders:

```toml
schema_version = 1

[provider]
kind = "generic-chat-completion-api"  # or "openrouter"
model = "<model-id>"
endpoint = "https://<provider>/v1"    # required for generic-chat-completion-api
credential = "<your-api-key>"
```

Notes:

- Supported provider kinds are `openrouter` and
  `generic-chat-completion-api`; anything else is rejected with a typed safe
  error. `endpoint` is optional for `openrouter` (the driver uses the
  OpenRouter API default).
- By explicit product decision the credential is open text in this private
  file. Keep the file readable only by your user (e.g. mode `0600` on Unix);
  never commit it. The configuration crate keeps raw text opaque: after
  parsing, credentials are not serialized, displayed, or included in errors,
  DTOs, events, snapshots, diagnostics, or protocol frames. Public
  projections expose only `credential_configured`.
- Configuration is read at daemon startup only. Controlled live reload,
  credential rotation, health checks, discovery, and pricing are accepted
  post-M5 directions in the M5+ control-plane slice, not current behavior.
- Malformed TOML or a future schema version fails typed validation, and the
  configuration file content is deliberately omitted from error output.

## Prerequisites

- **Platforms.** Linux and Windows are the CI-verified platforms
  (`ubuntu-24.04`, `windows-2025`); the codebase also carries macOS path
  mapping. Code, tests, and fixtures must not hard-code POSIX-only paths.
- **Rust.** The pinned stable toolchain is `1.97.1`
  ([rust-toolchain.toml](rust-toolchain.toml)) with `rustfmt`, `clippy`, and
  `llvm-tools-preview`. Coverage CI additionally uses pinned
  `nightly-2026-07-31`; all quality tools are pinned in
  [quality/tools.toml](quality/tools.toml).
- **Python 3** for the quality scripts behind the Makefile.
- **Network** on first setup: `make bootstrap-tools` installs the exact pinned
  toolchains and quality tools (this target is mutating and networked;
  everything else is offline once installed).
- Everything builds from the committed `Cargo.lock`; the workspace uses
  edition 2024, denies `unsafe_code`, and applies a strict Clippy policy
  (see [Cargo.toml](Cargo.toml)).

## Build, test, and quality commands

Use the root [Makefile](Makefile) for all quality work. Start with `make
quick` while iterating and run `make verify` before acceptance.

| Command | Purpose |
| --- | --- |
| `make bootstrap-tools` | Mutating/networked: install exact pinned toolchains and quality tools. |
| `make quick` | Fast local loop: tools check, `fmt-check`, lint, default-profile tests. |
| `make check` | Complete non-mutating source gate: format, features, `check-cargo`, lint, tests/doctests, docs, architecture. |
| `make docs-check` | Rustdoc across feature profiles, then Markdown link/Mermaid/secret-pattern validation. |
| `make coverage` | Branch-aware coverage for all profiles, enforcing the declared tiers (see [quality/coverage.toml](quality/coverage.toml)). |
| `make verify` | Full acceptance gate: `check` plus coverage, dependency gates, and quality self-tests; removes only generated LLVM coverage artifacts. |
| `make deps` | Supply-chain gates: deny, audit, outdated, machete, udeps, notices check. |
| `make notices` / `make notices-check` | Regenerate / verify [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) against the locked graph. |
| `make ci` | Local alias for the full gate; GitHub Actions runs the per-job aliases (`ci-lint-arch`, `ci-test`, `ci-coverage-default|no-default|all`, `ci-selftest`, `ci-deps`) in parallel. |

`make verify` requires pinned tools, the committed lockfile, and performs no
hidden dependency or tool installation. See
[Quality gates and Makefile](docs/intention-relay/architecture/12-quality-gates-and-makefile.md)
for the detailed contract.

## CI

The blocking workflow is
[.github/workflows/quality.yml](.github/workflows/quality.yml), run on pushes
and pull requests to `main`:

- `lint-arch` and `test` jobs on `ubuntu-24.04` and `windows-2025`;
- `coverage-default`, `coverage-no-default`, `coverage-all`, `selftest`, and
  `deps` jobs on `ubuntu-24.04`;
- pinned toolchains and per-job tool scopes, rust-cache, `mold` on Linux,
  sccache for coverage builds, and uploaded quality reports/metrics.

Two supporting workflows are not part of the blocking gate: a weekly cache
cleanup ([cache-cleanup.yml](.github/workflows/cache-cleanup.yml)) and a
manual, observational cargo `--jobs` build benchmark
([quality-benchmark.yml](.github/workflows/quality-benchmark.yml)). Dependabot
is enabled for dependency updates
([dependabot.yml](.github/dependabot.yml)).

## Documentation

- [AGENTS.md](AGENTS.md): agent instructions and engineering rules for this
  repository.
- [docs/README.md](docs/README.md): documentation index.
- [docs/intention-relay/README.md](docs/intention-relay/README.md):
  authoritative product reference material, closeouts, decisions,
  reconciliation, and the legacy-derived baseline.
- [docs/intention-relay/architecture/README.md](docs/intention-relay/architecture/README.md):
  target architecture with reading paths (principles, crate map, DTO policy,
  quality gates, TTD, roadmap).
- [Implementation roadmap](docs/intention-relay/architecture/11-implementation-roadmap.md):
  milestone-by-milestone delivery plan and the M5+ slice order.
- [docs/intention-relay/closeout/](docs/intention-relay/closeout/):
  immutable closure evidence for each milestone, including CI results and
  coverage.
- [docs/intention-relay/decisions/README.md](docs/intention-relay/decisions/README.md):
  accepted architecture decision records (ADR 0001-0036).
- [docs/reference/README.md](docs/reference/README.md): preserved legacy
  research material, not an implementation dependency.
- [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md): generated license notices
  for registry dependencies.

## Roadmap limitations

What `main` does not yet provide (all of it is documented roadmap work):

- No desktop (Tauri/M6) or usable terminal application; `intention-tui` is a
  proof library and `intention-tauri` is an empty slot.
- No Plan/Build artifact policy, physical plans, or Build Autopilot (M7); no
  VFR or Headroom behavior (M8).
- No controlled live reload, credential rotation, health checks, discovery,
  or pricing (M5+ slice 2); no continual harness, programmatic-caller
  policy, Goal domain, or session branching (M5+ slices 3-4).
- Out of scope for v1: Web/remote transport, multi-user access, sandboxed
  execution, and automatic run resumption.

The v1 boundary and non-goals are stated precisely in
[architecture 00](docs/intention-relay/architecture/00-principles-and-scope.md).

## Development and contribution

- Read [AGENTS.md](AGENTS.md) before contributing. The authoritative product,
  architecture, and roadmap context lives under
  [docs/intention-relay/](docs/intention-relay/README.md).
- Follow test-driven delivery: every milestone starts from failing contract,
  architecture, and outcome tests ([TTD policy](docs/intention-relay/architecture/10-test-driven-delivery-and-verification.md)).
- Use the quality gates: `make quick` while iterating, `make verify` before
  handoff; CI must stay green on pull requests to `main`.
- Keep machine-readable policies ([quality/](quality)) and architecture
  documentation in sync with code changes: new production crates must be
  declared with a responsibility, test target, and coverage tier before
  production code is accepted.
- Single live version, no backward compatibility: when an execution path
  becomes outdated, remove it; do not add compatibility layers, fallback
  branches, or migration fixtures for older schema/protocol/format versions.
- Use only pinned dependencies and tools; keep the lockfile committed; never
  run hidden installs inside the gates.
- Never commit or expose secrets (API keys, tokens, passwords) in code,
  configuration, logs, or examples. Use placeholders only.
- All code, comments, docs, and commits are in English. Use Conventional
  Commit messages (`type(scope): description`) and keep changes surgical.
- The workspace is Apache-2.0 licensed; registry dependency licenses are
  tracked in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
