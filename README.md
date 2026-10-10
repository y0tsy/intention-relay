# Intention Relay

Intention Relay is a local-first, single-user coding-agent system being built
as a Rust workspace. A standalone daemon process owns the application runtime
and all durable state; the terminal application (fullscreen TUI, interactive
REPL, and headless prompt command) is a delivered adapter over one typed local
protocol and one shared Rust client, and desktop (Tauri) presentation remains a
planned adapter over the same client.
The project is under active development: the daemon-side backend is
implemented through the closed M0-M5 milestones, the M5+ retrospective stack
(the no-backward-compatibility removal program, request-side tool
advertisement, the opt-in live-provider channel, and the same-run reasoning
round trip) is merged, and the post-M5 foundation (Milestone 5+) is in progress
with its seven slices defined and three activated. No released product exists
yet.

Everything here is development-machine software: there are no deployed users,
no externally persisted data, and no third-party consumers. Backward
compatibility is neither required nor in demand (see `AGENTS.md`).

## What it does

- **Daemon-owned sessions and runs.** A user project maps to durable sessions;
  an accepted user turn can start one agent-execution run with a tracked
  lifecycle (`Starting`, `Running`, `Completed`, `Failed`, `Interrupted`). A
  reconnecting client re-reads current state: there are no cursors, replays,
  event-log snapshots, model facts, or event envelopes.
- **One typed protocol, one shared client.** All adapters reach the daemon
  through `intention-client` over a private, per-user local transport (Unix
  domain sockets on Unix, named pipes on Windows). DTOs are the only things
  that cross crate, process, and persistence boundaries.
- **Model-driven runs with workspace tools.** Provider-neutral model drivers
  (OpenRouter and generic OpenAI-compatible Chat Completions) stream into a
  run loop that can call typed, `WorkspaceRoot`-bounded tools (`read`,
  `write`, `edit`, `execute`, `glob`, `grep`) with one deterministic typed
  tool sequence and durable, redacted tool-result evidence.
- **SQLite-first durable state.** One composition crate selects the SQLite
  adapter; the durable store is nine current-state tables (`projects`,
  `workspace_roots`, `sessions`, `runs`, `turns`, `messages`, `tool_results`,
  `configuration_revisions`, `tui_settings`) in a single-version schema,
  `messages` plus
  `tool_results` are the transcript, and every state change commits in one
  SQLite transaction before publication, with recovery-before-readiness on
  daemon start.
- **Security posture.** Single-user, endpoint and state kept under the current
  user's platform directories; fail-closed path, symlink, and configuration
  policies; credentials stay in the private config layer and never appear in
  public DTOs, errors, durable records, or logs. These are logical product
  controls in a trusted-local process, not an OS sandbox.

## Implementation status

Milestones M0 through M5 are closed; the table below summarizes the scope
of each.

| Milestone | Status | Scope |
| --- | --- | --- |
| M0 Quality foundation | Closed | Reproducible Makefile-orchestrated quality pipeline (format, lint, nextest, docs, architecture, coverage, supply-chain gates) with pinned tools. |
| M1 Contracts, configuration, workspace skeleton | Closed | Tier-A crate boundaries (`intention-proto`, `intention-config`; M1's `intention-domain` was later folded into `intention-proto`), DTO-first policy, TOML config with redacted projections, compile-only skeletons for every later crate. |
| M1+ Quality policy hardening | Closed | Machine-readable policies (`quality/*.toml`) enforce the crate classification with its declared responsibilities, workspace and external dependency edges, named integration test targets, and coverage tiers. |
| M2 Local protocol, client, daemon bootstrap | Closed | Private local IPC, correlated request/response codec, shared bootstrap client with startup lock and readiness polling, in-memory fixture composition, minimal TUI proof adapter (the hello/version handshake was later replaced by one typed wire). |
| M3 SQLite sessions, transcript, queue | Closed | Durable SQLite-backed sessions, runs, turns, and transcript rows; turn queueing; canonical credential-free config revisions; recovery-before-ready. |
| M4 Model contract, providers, one streaming run | Closed | Provider-neutral model contracts and validated stream facts; private OpenRouter and generic Chat Completions drivers; durable model evidence in the transcript; one daemon-owned streaming run with reconnect to current state and run-scoped delivery. |
| M5 Typed tools and workspace | Closed | Production model-tool loop hosted by the real daemon binary: six executable tools, fail-closed `WorkspaceRoot` resolution, one deterministic typed tool sequence, durable and redacted tool-result evidence, daemon-host end-to-end tests on Linux and Windows. |
| M5+ Post-M5 foundation | **In progress** | Accepted activation home for the post-M5 stack (defined in the [implementation roadmap](docs/intention-relay/architecture/11-implementation-roadmap.md); the instruction sources slice is owned by [architecture 30](docs/intention-relay/architecture/30-instruction-sources-and-system-context.md)) delivered as seven slices: 1) contracts and versions, 1.5) core simplification, 2) terminal UX (TUI, REPL, and headless CLI in one `intention-tui` binary), 3) instruction sources and system context, 4) control plane, 5) Goal domain, 6) UI foundation. Slice 1 (canonical execution-meaning codec and digest fixtures, negotiated capability families and contract-family DTOs, single live storage schema; the canonical codec and contract-family machinery was later removed by the typed-serde JSON contract policy) is merged into `main`. Slice 1.5 is activated and its current-state core is merged into `main`: it froze a current-state core where the nine production crates stay, DTOs remain only at the IPC wire, SQLite, and provider-SDK boundaries, tool inputs and outputs are schema-validated JSON, identity reduces to eight newtypes, the event log, snapshots, cursors, and resync gave way to the nine current-state tables written in one SQLite transaction per state change with publication from the committed values, the typed hook plane was removed, hello/version negotiation gave way to one typed wire, and `intention-client` is fully asynchronous. Slice 2 is activated: `intention-tui` is the terminal application — one binary with a revue-based fullscreen UI, an interactive REPL, and a headless prompt command, all contract-equivalent over the shared client, with session listing, the answer and reasoning channels of transient text deltas, and a closed typed exit-status set; the fullscreen UI shows a welcome surface (the product lockup and a version/`AGENTS.md`/`MCPs`/`Skills` overview) until a session is asked for and opens no session at launch, draws one window (the chat panel with markdown answers, a multi-line input block, and the registry-driven command hint band above it, and the sessions panel docked to the window's bottom rows over one light/dark palette system, with a `/theme` command, its argument band, and the theme picker panel), caches the committed transcript layout, streams the live tail through the same block functions as the committed rows, and layers `Ctrl+C` and `Esc` into consecutive-press arms for interrupting, history-pushing, and quitting, with a single `Esc` cancelling a live run. Slices 3-6 are defined and not implemented. |
| M6-M9 | Planned | M6 Tauri bridge and primary desktop UI; M7 Plan/Build policies, physical plans, and Build Autopilot; M8 VFR and Headroom; M9 hardening and acceptance verification. See the [implementation roadmap](docs/intention-relay/architecture/11-implementation-roadmap.md). |

Everything beyond M5 outside the activated M5+ slices is roadmap direction
recorded in [architecture](docs/intention-relay/architecture/README.md), not
delivered behavior. No roadmap document implements or silently supersedes the
closed M0-M5 behavior.

## Crate map

All workspace crates live under [crates/](crates/) unless noted. Coverage is
enforced by the machine-readable policy in
[quality/coverage.toml](quality/coverage.toml) under
[architecture 12](docs/intention-relay/architecture/12-quality-gates-and-makefile.md): per-crate line
tiers of 75% (`core`), 60% (`standard`), and 20% (`edge`), with `intention-proto`
exempt at 0% and outside collection.

### DTO foundations

| Crate | Responsibility |
| --- | --- |
| [intention-proto](crates/intention-proto) | Shared, dependency-light DTOs plus the typed public local-protocol surface: validated identifiers, schema versions, safe errors, time, pagination, model/tool value DTOs, run lifecycle and projection values, the one typed request/result/reply wire over NDJSON, and the typed command/query/frame payloads (protocol owned by [architecture 03](docs/intention-relay/architecture/03-daemon-transport-and-adapters.md) and [architecture 02](docs/intention-relay/architecture/02-dto-and-contract-policy.md)). |
| [intention-config](crates/intention-config) | Versioned TOML parsing, validation, path selection, and credential-free public configuration projections. |

### Durable storage and application core

| Crate | Responsibility |
| --- | --- |
| [intention-storage](crates/intention-storage) | DTO-only repository contract plus the bundled SQLite implementation behind it (single current schema, created directly on open); the SQLite backend is selected only by the composition crate. |
| [intention-engine](crates/intention-engine) | Commands, queries, and semantic use-case workflows plus deterministic run execution, interruption handling, context-window accounting, and recovery-before-ready, over DTO-only storage. |

### Model drivers

| Crate | Responsibility |
| --- | --- |
| [intention-providers](crates/intention-providers) | Provider-neutral model contracts (messages, tool calls/results, validated stream facts) plus the OpenRouter and generic Chat Completions translation adapters; `openrouter-rs` and `async-openai` stay private implementation details. |

### Tools and workspace

| Crate | Responsibility |
| --- | --- |
| [intention-tools](crates/intention-tools) | Tool contracts with JSON Schema descriptors, the static six-tool spec match, and the `WorkspaceRoot` addressing anchor (owned by [architecture 05](docs/intention-relay/architecture/05-tools-and-workspace.md)). |

### Transport, client, and daemon (active)

| Crate | Responsibility |
| --- | --- |
| [intention-transport](crates/intention-transport) | Private per-user IPC: Unix sockets / Windows named pipes, NDJSON framing of one typed message per line (1 MiB message cap), and the single live wire version carried in the endpoint name. |
| [intention-client](crates/intention-client) | Shared bootstrap, dispatch, subscription, and reconnect client for adapters, with advisory startup lock and daemon launch. |
| [intention-daemon](crates/intention-daemon) | Composition root (`DaemonApplicationFacade`, the only selector of SQLite and concrete drivers), daemon host library, and the thin `intention-daemon` binary. |

### Adapter slots and reserved crates

| Crate | Status | Responsibility |
| --- | --- | --- |
| [intention-tui](crates/intention-tui) | Active adapter (terminal application) | The terminal application over the shared client: a revue-based fullscreen UI, an interactive REPL, and a headless prompt command in one binary. |
| `intention-tauri` | Planned (M6) | Reserved Tauri bridge/UI adapter slot; the crate is created at M6. |
| `intention-vfr`, `intention-headroom`, `intention-plans` | Planned (M7/M8) | Reserved VFR, Headroom/CCR, and Plan/Build artifact crates; created at M7 (plans) and M8 (VFR/Headroom). |
| [intention-test-support](crates/intention-test-support) | Non-production | Durable integration fixtures and contract scenarios used by tests. |

Architecture rules worth knowing: only `intention-daemon`
touches SQLite or selects concrete providers; presentation adapters may only
use `intention-client`, `intention-proto`, and `intention-transport`
boundaries; provider SDKs and Tokio/transport resources stay private to their
owner crates.

## Running the daemon and the terminal client

The workspace has two binaries. `intention-daemon` serves the real, durable
daemon over a private per-user endpoint, and `intention-tui` is the terminal
application over the shared `intention-client`. The daemon is also driven
directly through `intention-client` in the daemon integration tests (for
example
[crates/intention-daemon/tests/client_e2e.rs](crates/intention-daemon/tests/client_e2e.rs),
which spawns the real binary, drives it over real IPC, and executes a real
`read` tool through the production model-tool loop).

Prerequisites for a manual run: a configured provider (see below) and a Rust
toolchain (see [Prerequisites](#prerequisites)).

```text
# Place a valid config file first (see Configuration), then:
cargo run -p intention-daemon                 # default endpoint instance
cargo run -p intention-daemon -- my-instance  # named logical endpoint

# The terminal client launches the daemon itself when none is running:
cargo run -p intention-tui                                # fullscreen TUI (requires a terminal)
cargo run -p intention-tui -- repl                        # interactive line REPL
cargo run -p intention-tui -- run "summarize this repo"   # one headless prompt
cargo run -p intention-tui -- run "..." --format json --timeout 60
```

The daemon resolves its configuration and state from platform-standard
locations, recovers unfinished runs to `Interrupted` before serving, and
prints only safe error codes on startup failure (exit status non-zero). The
endpoint argument is a logical safe instance name; endpoint filesystem paths
never appear in protocol DTOs or errors.

The terminal client shares one command grammar across its three modes
(`tui`, `repl`, `run <PROMPT>`, with `--workspace`, `--session`, `--continue`,
`--mode`, `--timeout`, and `--format`), talks to the daemon only through
`intention-client`, renders the step's answer and reasoning as separate
transient delta channels until the committed row replaces them, and ends with a
closed typed status set: 0 completed, 1 usage, 2 daemon or transport, 3 typed
rejection or failed run, 4 timeout after an interrupt, 5 interrupted run. The
interactive modes open no session at launch: the fullscreen UI starts on its
welcome surface, and a session appears only for an explicit
`--session`/`--continue`, `/new`, a browser row, or the first prompt, which
creates the session it needs. The fullscreen UI carries the whole command
surface in one registry — `/new`, `/sessions`, and `/theme` are the registered
commands — and a leading-slash word opens a hint band above the input block that
filters, ranks, and completes them, with `Tab` or `Enter` committing the
highlighted one and `Esc` closing the band before it means anything else. The
registry also declares each command's arguments: `/theme` takes an optional
`light` or `dark` value, and while the caret is in that argument word the band
lists those values in the same three columns, completing one with a trailing
space. A theme is the terminal's warm off-white light palette or its warm
charcoal dark palette, resolved once per frame so a theme change replays the
transcript exactly as a resize does. The daemon owns the effective theme:
`config.toml` supplies the default through `[tui] theme` (absent means light),
and the stored single-row override in the state database wins over it. `/theme`
with a value selects that theme through the daemon; with no value it opens a
picker panel in the chat panel's band region — so the input block keeps its
rows — where `Up`/`Down` preview the whole window live without persisting
anything, `Enter` commits through the daemon, and `Esc` reverts the preview.

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

[tui]
theme = "light"  # optional; "dark" selects the dark palette
```

Notes:

- Supported provider kinds are `openrouter` and
  `generic-chat-completion-api`; anything else is rejected with a typed safe
  error. `endpoint` is optional for `openrouter` (the driver uses the
  OpenRouter API default).
- The optional `[tui] theme` selects the terminal palette: `light` (the
  default when the section or key is absent) or `dark`. An unknown spelling is
  rejected with the typed `invalid_tui_theme` validation error, so nothing
  silently falls back. The daemon answers the effective theme over the typed
  wire — the state database's single-row override, else this default, else
  light — and a theme selection records no configuration revision and never
  rewrites this credential-bearing file.
- By explicit product decision the credential is open text in this private
  file. Keep the file readable only by your user (e.g. mode `0600` on Unix);
  never commit it. The configuration crate keeps raw text opaque: after
  parsing, credentials are not serialized, displayed, or included in errors,
  DTOs, durable records, diagnostics, or protocol frames. Public
  projections expose only `credential_configured`.
- Configuration is read at daemon startup. Controlled reload with canonical
  config revisions, credential rotation, health checks, provider discovery and
  pricing, raw-TOML configuration editing, and session defaults with per-turn
  overrides are the accepted M5+ Slice 4 direction; that slice is not
  implemented. Fork override commands remain Slice 6 work. Nothing else
  changes configuration after startup.
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
| `make quick` | Fast local loop: tools check, `fmt-check`, lint, tests. |
| `make check` | Complete non-mutating source gate: format, lint, tests/doctests, docs, architecture. |
| `make docs-check` | Rustdoc, then Markdown link/Mermaid/secret-pattern validation. |
| `make coverage` | Line coverage, enforcing each collected crate's declared tier floor (see [quality/coverage.toml](quality/coverage.toml)). |
| `make verify` | Full acceptance gate: `check` plus coverage and dependency gates; removes only generated LLVM coverage artifacts. |
| `make deps` | Supply-chain gates: deny, audit, outdated, machete, udeps, notices check. |
| `make notices` / `make notices-check` | Regenerate / verify [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) against the locked graph. |
| `make ci` | Local alias for the full gate; GitHub Actions runs the per-job aliases (`ci-lint-arch`, `ci-test`, `ci-coverage-default`, `ci-deps`) in parallel. |

`make verify` requires pinned tools, the committed lockfile, and performs no
hidden dependency or tool installation. See
[Quality gates and Makefile](docs/intention-relay/architecture/12-quality-gates-and-makefile.md)
for the detailed contract.

## CI

The blocking workflow is
[.github/workflows/quality.yml](.github/workflows/quality.yml), run on pushes
and pull requests to `main`:

- `lint-arch` and `test` jobs on `ubuntu-24.04` and `windows-2025`;
- `coverage-default` and `deps` jobs on `ubuntu-24.04`;
- pinned toolchains and per-job tool scopes, rust-cache, `mold` on Linux,
  sccache for coverage builds, and uploaded quality reports.

One supporting workflow is not part of the blocking gate: the manual
live-provider end-to-end run
([real-api-e2e.yml](.github/workflows/real-api-e2e.yml)). Dependabot is
enabled for dependency updates
([dependabot.yml](.github/dependabot.yml)).

## Documentation

- [AGENTS.md](AGENTS.md): agent instructions and engineering rules for this
  repository.
- [docs/README.md](docs/README.md): documentation index.
- [docs/intention-relay/README.md](docs/intention-relay/README.md):
    authoritative product reference material and the target architecture.
- [docs/intention-relay/architecture/README.md](docs/intention-relay/architecture/README.md):
  target architecture with reading paths (principles, crate map, DTO policy,
  quality gates, TTD, roadmap, and the instruction channel).
- [Implementation roadmap](docs/intention-relay/architecture/11-implementation-roadmap.md):
  the remaining milestone plan (M6-M12) and the M5+ slice order.

- [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md): generated license notices
  for registry dependencies.

## Roadmap limitations

What `main` does not yet provide (all of it is documented roadmap work):

- No desktop (Tauri/M6) UI yet: the `intention-tauri` bridge is created at M6,
  while the terminal application (`intention-tui`, the one TUI/REPL/headless
  binary) is delivered.
- No Plan/Build artifact policy, physical plans, or Build Autopilot (M7); no
  VFR or Headroom behavior (M8).
- No M5+ work on the Goal domain or session
  branching, and no fork override commands (slices 5-6); no instruction-source, `AGENTS.md`, or effective
  instruction projection behavior
  (slice 3, owned by [architecture 30](docs/intention-relay/architecture/30-instruction-sources-and-system-context.md)).
- No project script library for kernel cells (`.ir/scripts`), which Milestone 12
  delivers as agent-authored modules persisted through the frozen tool
  descriptors ([architecture 20](docs/intention-relay/architecture/20-ipython-kernel-lifecycle.md)).
- Slice 1.5 core simplification is activated: the event log, snapshots,
  cursors, and resync are replaced by the nine current-state tables written in
  one SQLite transaction per state change, and the nine-production-crate
  consolidation and the composition-facade removal are merged on `main`.
- Slice 2 terminal UX is activated: one `intention-tui` binary carries the
  fullscreen revue TUI, the interactive REPL, and the headless prompt command
  over one shared client, with session listing, the answer and reasoning
  channels of transient text deltas, and a closed typed exit-status set. The
  fullscreen UI shows a welcome surface (the product lockup and a
  version/`AGENTS.md`/`MCPs`/`Skills` overview) while no session is open, opens
  no session at launch, draws one window — the chat panel with markdown answers,
  a multi-line input block, and the registry-driven command hint band above it,
  and the sessions panel docked to the window's bottom rows — over one light/dark
  palette system, with `/theme` (`light` or `dark`, or no argument for the
  previewing picker panel) and the daemon-owned effective theme,
  caches the committed transcript layout,
  streams the live tail through the same block functions as the committed rows,
  and layers `Ctrl+C` and `Esc` into consecutive-press arms for interrupting,
  history-pushing, and quitting (a single `Esc` cancels a live run).
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
