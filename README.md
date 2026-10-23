# Intention Relay

Intention Relay is a local-first, single-user coding-agent system built as a
Rust workspace. A standalone daemon process owns the application runtime and
all durable state; the terminal application (fullscreen TUI, interactive REPL,
and headless prompt command) is a delivered adapter over one typed local
protocol and one shared Rust client, and desktop (Tauri) presentation is a
planned adapter over the same client.

The project is under active development: the daemon-side backend is implemented
through the closed M0-M5 milestones, the post-M5 foundation is in progress with
slices 1, 1.5, and 2 merged and slices 3-6 defined, and no released product
exists yet. Everything here is development-machine software: there are no
deployed users, no externally persisted data, and no third-party consumers, so
backward compatibility is neither required nor in demand (see `AGENTS.md`).

## What it does

- **Daemon-owned sessions and runs.** A user project maps to durable sessions;
  an accepted user turn can start one agent-execution run with a tracked
  lifecycle (`Starting`, `Running`, `Completed`, `Failed`, `Interrupted`). A
  reconnecting client re-reads current state.
- **One typed protocol, one shared client.** All adapters reach the daemon
  through `intention-client` over a private, per-user local transport (Unix
  domain sockets on Unix, named pipes on Windows).
- **Model-driven runs with workspace tools.** Provider-neutral model drivers
  (OpenRouter and generic Chat-Completions-compatible endpoints) stream into a
  run loop that can call typed workspace tools (`read`, `write`, `edit`,
  `execute`, `glob`, `grep`) in one deterministic typed tool sequence, with
  durable, redacted tool-result evidence. Workspace addressing and tool policy
  are owned by [architecture 05](docs/intention-relay/architecture/05-tools-and-workspace.md).
- **SQLite-first durable state.** One composition crate selects the SQLite
  adapter; the durable store is six current-state tables in a single-version
  schema, `messages` rows are the transcript, and every state change commits in
  one SQLite transaction before publication, with recovery-before-readiness on
  daemon start.
- **Security posture.** Single-user, endpoint and state kept under the current
  user's platform directories; fail-closed path, symlink, and configuration
  policies; credentials stay in the private config layer and never appear in
  public DTOs, errors, durable records, or logs. These are logical product
  controls in a trusted-local process, not an OS sandbox.

## Repository layout

The workspace is nine production crates plus the `intention-tui` terminal
adapter and the non-production `intention-test-support` fixture crate:
`intention-proto` (identity, shared value types, and the typed protocol),
`intention-config`, `intention-storage` (repository contract plus the bundled
SQLite implementation), `intention-engine` (commands, queries, run execution,
recovery), `intention-providers` (provider-neutral model contract plus the
OpenRouter and generic Chat Completions adapters), `intention-tools` (tool
descriptors, dispatch, and workspace addressing), `intention-transport`
(per-user local IPC), `intention-client` (bootstrap, dispatch, subscriptions,
reconnect), and `intention-daemon` (the one composition root and the daemon
binary). Each crate's responsibility, permitted dependency edges, and test
targets are stated in the
[crate map](docs/intention-relay/architecture/01-workspace-and-crate-map.md);
only `intention-daemon` selects concrete implementations, and only the IPC
wire, SQLite rows, and provider SDK calls carry DTOs.

## Running the daemon and the terminal client

The workspace has two binaries. `intention-daemon` serves the durable daemon
over a private per-user endpoint, and `intention-tui` is the terminal
application over the shared `intention-client`, launching the daemon itself
when none is running.

Prerequisites for a manual run: a configured provider (see
[Configuration](#configuration)) and a Rust toolchain (see
[Prerequisites](#prerequisites)).

```text
# Place a valid config file first (see Configuration), then:
cargo run -p intention-daemon                 # default endpoint instance
cargo run -p intention-daemon -- my-instance  # named logical endpoint

cargo run -p intention-tui                                # fullscreen TUI (requires a terminal)
cargo run -p intention-tui -- repl                        # interactive line REPL
cargo run -p intention-tui -- run "summarize this repo"   # one headless prompt
cargo run -p intention-tui -- run "..." --format json --timeout 60
```

The daemon resolves its configuration and state from platform-standard
locations, recovers unfinished runs to `Interrupted` before serving, and prints
only safe error codes on startup failure (exit status non-zero). The endpoint
argument is a logical safe instance name; endpoint filesystem paths never appear
in protocol DTOs or errors. The terminal client addresses the endpoint instance
the `INTENTION_ENDPOINT` identifier names, falling back to the platform-default
endpoint when the variable is absent, and launches the `intention-daemon`
program installed beside its own executable.

All three modes share one command grammar (`tui`, `repl`, `run <PROMPT>`, with
`--workspace`, `--session`, `--continue`, `--mode`, `--timeout`, `--format`,
and the fullscreen UI's `--theme auto|light|dark`) and end with a closed typed
status set: 0 completed, 1 usage, 2 daemon or transport, 3 typed rejection or
failed run, 4 timeout with no observed terminal status, 5 interrupted run. A run
stops through the daemon's `run.cancel`: `Esc` stops a live run in one press and
`Ctrl+C` arms the same stop. The fullscreen UI, its command surface, and its
locally stored theme are documented in
[crates/intention-tui/README.md](crates/intention-tui/README.md) and
[architecture 03](docs/intention-relay/architecture/03-daemon-transport-and-adapters.md).

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
- The credential is open text in this private file. Keep the file readable only
  by your user (e.g. mode `0600` on Unix); never commit it. The configuration
  crate keeps raw text opaque: after parsing, credentials are not serialized,
  displayed, or included in errors, DTOs, durable records, diagnostics, or
  protocol frames. Public projections expose only `credential_configured`.
- Configuration is read at daemon startup, and nothing changes it after
  startup. Malformed TOML or a future schema version fails typed validation,
  and the configuration file content is omitted from error output.

## Prerequisites

- **Platforms.** Linux and Windows are the CI-verified platforms
  (`ubuntu-24.04`, `windows-2025`); the codebase also carries macOS path
  mapping. Code, tests, and fixtures must not hard-code POSIX-only paths.
- **Rust.** The pinned stable toolchain is `1.97.1`
  ([rust-toolchain.toml](rust-toolchain.toml)) with `rustfmt`, `clippy`,
  `llvm-tools-preview`, and the `x86_64-pc-windows-gnu` target; all quality
  tools are pinned in [quality/tools.toml](quality/tools.toml).
- **MinGW** only for `make lint-windows`, which lints the Windows target from
  Linux; CI installs it for the lint job, and no other local target needs it.
- **Python 3** for the quality scripts behind the Makefile.
- **Network** on first setup: `make bootstrap-tools` installs the exact pinned
  toolchains and quality tools (this target is mutating and networked;
  everything else is offline once installed).
- Everything builds from the committed `Cargo.lock`; the workspace uses
  edition 2024, denies `unsafe_code`, and applies a pragmatic Clippy policy
  (see [Cargo.toml](Cargo.toml)).

## Build, test, and quality commands

Use the root [Makefile](Makefile) for all quality work. Start with `make quick`
while iterating and run `make verify` before acceptance; the full gate contract
is [architecture 12](docs/intention-relay/architecture/12-quality-gates-and-makefile.md).

| Command | Purpose |
| --- | --- |
| `make bootstrap-tools` | Mutating/networked: install exact pinned toolchains and quality tools. |
| `make quick` | Fast local loop: tools check, `fmt-check`, lint, tests. |
| `make check` | Complete non-mutating source gate: format, lint, tests, doctests/rustdoc. |
| `make lint-windows` | Lint the Windows target from Linux; needs the pinned `x86_64-pc-windows-gnu` target and a MinGW toolchain. |
| `make coverage` | One instrumented workspace run; enforces each measured crate's line floor from `COVERAGE_FLOORS` in the [Makefile](Makefile). |
| `make verify` | Full acceptance gate: `check` plus the coverage and dependency gates. |
| `make deps` | Supply-chain gates: lockfile anchor, license and advisory policy, manifest hygiene. |
| `make ci` | Local alias for the full gate; GitHub Actions runs the per-job aliases (`ci-lint-arch`, `ci-test`, `ci-coverage-default`, `ci-deps`) in parallel. |

`make verify` requires pinned tools and the committed lockfile, and performs no
hidden dependency or tool installation. Machine-readable policy lives in
[quality/tools.toml](quality/tools.toml) (pinned toolchain and tools, validated
by `quality/check_tools.py`), [deny.toml](deny.toml) (licenses and advisories),
and [clippy.toml](clippy.toml) (the two compiler-enforced source rules). Crate
and external dependency edges are enforced by the crate manifests and rustc.

## CI

The blocking workflow is
[.github/workflows/quality.yml](.github/workflows/quality.yml), run on pushes
and pull requests to `main`:

- `lint-arch` (formatting, native lint, Windows-target lint, doctests,
  rustdoc), `coverage-default`, and `deps` on `ubuntu-24.04`;
- `test` on `windows-2025`, which runs the platform-dependent suite
  (`PLATFORM_TESTS`) and the doctests; Linux test execution happens inside
  `coverage-default`, which runs the full suite instrumented;
- pinned toolchains and per-job tool installs, a per-phase build cache that
  keeps each compiling job's target tree warm, `mold` from its own pinned action
  on Linux, LLD instead of `link.exe` on Windows, MinGW for the Windows-target
  lint, and uploaded quality reports.

The live-provider end-to-end run is a local target, `make e2e-real-api`, and
never part of the blocking gate. Dependabot is enabled for dependency updates
([dependabot.yml](.github/dependabot.yml)).

## Documentation

- [AGENTS.md](AGENTS.md): agent instructions and engineering rules for this
  repository.
- [docs/README.md](docs/README.md): documentation index.
- [docs/intention-relay/README.md](docs/intention-relay/README.md):
  authoritative product reference material and the target architecture.
- [docs/intention-relay/architecture/README.md](docs/intention-relay/architecture/README.md):
  target architecture with reading paths.
- [Implementation roadmap](docs/intention-relay/architecture/11-implementation-roadmap.md):
  the milestone plan and the M5+ slice order.

## Roadmap limitations

What `main` does not yet provide (all of it is roadmap work):

- No desktop (Tauri/M6) UI: the `intention-tauri` bridge is created at M6.
- No VFR or Headroom behavior (M7).
- No agent-operation behavior (slice 3), Goal domain or session branching
  (slices 5-6).
- No project script library for kernel cells (`.ir/scripts`), which Milestone
  11 delivers ([architecture 20](docs/intention-relay/architecture/20-ipython-kernel-lifecycle.md)).
- Out of scope for v1: web/remote transport, multi-user access, sandboxed
  execution, and automatic run resumption.

The v1 boundary and non-goals are stated in
[architecture 00](docs/intention-relay/architecture/00-principles-and-scope.md).

## Development and contribution

- Read [AGENTS.md](AGENTS.md) first. The authoritative product, architecture,
  and roadmap context lives under
  [docs/intention-relay/](docs/intention-relay/README.md).
- Follow test-driven delivery: every milestone starts from failing contract,
  architecture, and outcome tests ([TTD policy](docs/intention-relay/architecture/10-test-driven-delivery-and-verification.md)).
- Use the quality gates: `make quick` while iterating, `make verify` before
  handoff; CI must stay green on pull requests to `main`.
- A new production crate states its responsibility in the crate map and gets a
  line floor in the Makefile's `COVERAGE_FLOORS` before production code is
  accepted.
- Keep one live version of every versioned system: when an execution path
  becomes outdated, remove it rather than adding compatibility layers, fallback
  branches, or migration fixtures.
- Use only pinned dependencies and tools; keep the lockfile committed; never
  run hidden installs inside the gates.
- Never commit or expose secrets (API keys, tokens, passwords) in code,
  configuration, logs, or examples. Use placeholders only.
- All code, comments, docs, and commits are in English. Use Conventional
  Commit messages (`type(scope): description`) and keep changes surgical.
- The workspace is Apache-2.0 licensed.
