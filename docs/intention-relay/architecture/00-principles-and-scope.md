# Principles and Scope

**Current policy.** Principles 1–12 govern v1, delivered through M5. The post-M4+ extension packages state no
principles here; each is governed by its owner document and implemented only where its owning milestone has landed
([implementation roadmap](11-implementation-roadmap.md)).

This document governs the application shape, ownership boundaries, v1 exclusions, and delivery mindset. Detailed
contracts belong to [DTO and contract policy](02-dto-and-contract-policy.md); crate ownership to [Workspace and crate
map](01-workspace-and-crate-map.md).

## Decided principles

### 1. Workspace, not a monolithic core

Intention Relay is a Rust workspace of small, logically grouped crates. `intention-daemon` is a composition root that creates
factories and connects implementations, not a catch-all core crate.

### 2. DTO-first at the three physical boundaries

A boundary communicates through an explicit, strictly typed DTO, and exactly three boundaries force a serialized shape:
the IPC wire, SQLite persistence, and provider SDK calls. Untyped maps, raw JSON, implementation records, SDK objects,
and stringly typed identifiers are prohibited as contracts anywhere. Crates inside the process call each other with
domain types: a DTO is not introduced between internal crates, and a type acquires wire attributes only where it
crosses one of the three boundaries ([architecture 02](02-dto-and-contract-policy.md)).

### 3. One daemon owns application runtime

A local single-user daemon is the only owner of active session actors, run actors, SQLite connections, model streams,
tools, and subscriptions. Adapters never own business runtime state.

### 4. Adapters share one contract

Tauri, TUI, and REPL use one typed local protocol through the shared `intention-client` crate. Tauri's Rust side is a
minimal native bridge because a WebView cannot directly use the local typed socket protocol.

### 5. One active run per session

A session may have one active run only. A user turn accepted while that run is active is recorded as a durable pending
turn and joins the run's live context in order at the next model boundary; it never starts a second run and never
silently merges into an in-flight request.

### 6. Local trusted workspace

A session has an obligatory `WorkspaceRoot`. All filesystem tools resolve paths against it, reject absolute and parent
(`..`) paths at the typed input, and never fall back to process `pwd`. Process execution receives it as CWD. The root is
an addressing anchor, not a containment boundary: a path may leave it through a symbolic link, and v1 is trusted local
execution, not a sandbox ([architecture 05](05-tools-and-workspace.md)).

### 7. Cross-cutting features attach as ordinary calls

Workspace enforcement, VFR, Headroom/CCR, and plan-mode restrictions attach as ordinary calls in the direct tool
sequence; VFR and Headroom arrive as modules inside `intention-tools`, and the typed hook plane was removed in Slice
1.5. Base tools stay focused on their primitive work and do not hard-code extension behavior.

### 8. Automatic durable state

Sessions, turns, run transitions, artifacts, plans, todos, tool calls, and configuration revisions are automatically
persisted under the one-transaction state-change law of [architecture 04](04-sessions-runs-events-and-storage.md).

### 9. Plan and Build are separate policies

Plan and Build share an agent loop but have distinct tool policies. Plan denies the project-mutating `write`/`edit`
tools and mutates only plan files in the daemon-defined plans directory through the plan owner. `execute` cannot be
fully constrained and therefore has an explicit prompt policy, risk policy, and audit trail.

### 10. Test-driven delivery is architectural

A crate is not considered complete because it compiles or has unit tests. Each delivery slice begins with failing
contract and outcome tests. Architecture rules are encoded in crate boundaries, dependency manifests, and the lint
configuration wherever a mechanism can decide them; the remainder is a review responsibility. See [Test-driven
delivery and verification](10-test-driven-delivery-and-verification.md).

### 11. Reproducible quality gates are architectural

Before production functionality is accepted, the workspace must have a pinned toolchain, strict pragmatic linting, the
per-crate line-coverage floors, rustdoc verification, and supply-chain verification. The root
Makefile orchestrates these non-mutating gates. GitHub Actions runs the per-job aliases (`ci-lint-arch`, `ci-test`,
`ci-coverage-default`, `ci-deps`) as parallel matrix jobs in `.github/workflows/quality.yml`, and
`make ci` is the local single-pass alias for the same full gate. See [Quality gates and
Makefile](12-quality-gates-and-makefile.md).

### 12. Unattended execution, no interactive gates

The user hands over a task and leaves; a run may last hours. No execution path waits for user input: admission, tools,
MCP, providers, children, bridge, and kernel work proceed without confirmation, approval, or a blocking question.
`ask_user` is asynchronous: the question is recorded durably and delivered to the user, the call returns immediately,
and the model continues the work the question does not directly block, instructed that an answer may never arrive
([architecture 15](15-tool-registry-and-model-tool-loop.md)).

Security is structural, never a prompt: `WorkspaceRoot` addressing (principle 6), credential isolation at the daemon's
private loading boundary ([architecture 09](09-configuration-security-and-observability.md)), typed DTO boundaries, safe
projections that keep credentials, raw frames, and implementation resources out of DTOs, logs, cards, and model
context, post-commit publication, and one local user who owns the machine. No phase may add a per-action confirmation
barrier.

User-facing workflow steps outside a run, such as Plan approval and Goal proposal acceptance, are not gates on a run:
they apply when the user returns and never pause execution.

## Scope boundaries

Adapters — the terminal application today, the desktop bridge at M6 — reach the daemon through `intention-client` and
the typed wire. They hold no business logic, no direct storage access, and no direct model-driver access. The live
crate graph and every dependency edge are the crate map in
[architecture 01](01-workspace-and-crate-map.md).

## Required v1 outcomes

The implementation must demonstrably provide:

1. Tauri and TUI/REPL can operate the same daemon-owned session without duplicating application logic.
2. A daemon restart marks an unfinished run as interrupted and exposes that result through the same contract.
3.  A filesystem tool cannot escape its session `WorkspaceRoot` through relative paths, absolute paths, or process CWD
fallback.
4.  A Plan-mode agent can create and refine a physical plan in the daemon-defined plans directory, while normal
write/edit tools cannot mutate the project or that directory.
5.  VFR and Headroom can be enabled independently as ordinary calls, preserve typed observability, and expose their
supporting tools.
6.  State visible to an adapter was committed before its corresponding live frame was published; provisional text
deltas are transient, never authoritative, and are replaced by the committed row.
7.  Provider secrets cannot appear in transport frames, persisted state, diagnostics, or normal UI output.
8.  `make verify` proves the applicable formatting, lint, feature, test, documentation, coverage, and supply-chain
gates before an implementation slice is accepted.

## Explicit v1 non-goals

Web interface, Telegram bot, remote API, HTTP/WebSocket transport, cross-device access, multiple
users/accounts/roles/profiles/remote authorization, parallel runs in one session, container/sandbox workspace isolation,
automatic continuation of interrupted model, tool, or shell processes after daemon restart, a general source editor or
proven LSP integration or Files panel, and direct user administration of MCP servers or direct manual sub-agent
spawning.

## Implementation-required decisions

The following are required before their owning implementation slice begins. The first table is settled by delivered
work; the second stays owned by the named slice.

### Settled by delivered work

| Topic | Settled decision and source |
| --- | --- |
| Turn input | A user turn accepted during an active run is recorded as a durable pending turn and joins that run's live context in FIFO order at the next model boundary; removal of a not-yet-seen pending turn stays explicit, and no automatic retry or resume exists ([architecture 04](04-sessions-runs-events-and-storage.md), "Pending turns"). |
| Risk policy | Build runs without a per-action confirmation barrier for configured active capabilities, while Plan keeps hard-denied project writes and an advisory-guided `execute` ([architecture 07](07-plan-and-build-modes.md)); the exact capability taxonomy and audit policy for `execute`, network, and destructive file actions remains listed as open in [architecture 05](05-tools-and-workspace.md). |
| AppData location | Production SQLite state lives in the platform AppData/state location with no process-CWD fallback (roadmap M3), and the migration half of the question is closed by the single-live-schema rule. |
| Schema history | The existence of databases is not an argument for keeping DB migrations or old-schema compatibility; every versioned system keeps exactly one live version and evolves in place. |
| Plan revision mechanics | Each edit rewrites the single body-only plan file, and the plan number, revision, and status live on the SQLite plan record ([architecture 07](07-plan-and-build-modes.md)); no frontmatter and no patch-record family exist, and Plan mode itself is M5+ Slice 3 scope. |
| Provider retries | Runtime owns at most one retry for a delayed or retryable provider failure before any durable fact, with a fixed 250 ms delay and `max_attempts` 1..=2 / `attempt_timeout_seconds` 1..=60, the attempt window being an idle window re-armed by every delivered provider event ([architecture 08](08-model-protocol-and-providers.md), [architecture 09](09-configuration-security-and-observability.md)). |

### Still open

| Topic | Decision required |
| --- | --- |
| Transcript retention | Retention and compaction policy for transcript rows and stream deltas. Closing it also needs one owner: [architecture 04](04-sessions-runs-events-and-storage.md) defers the policy, [architecture 09](09-configuration-security-and-observability.md) records it as its own open item, and the roadmap assigns closure to Milestone 8. |
| CCR backend | The read-through view's storage backend ([architecture 06](06-vfr-and-headroom.md), "Open decisions"). The durable tool result is the record, so the view carries no retention, rotation, expiry, or capacity policy. |

## Deferred decisions

Remote adapter security and authentication; multi-daemon or cloud synchronization; a plugin API for externally supplied
tools; ordinary-v1 background scheduled jobs, including recurring schedule syntax and worker topology; sandboxing,
worktrees, or per-run filesystem isolation.

## Verification

Before implementation starts, the team must create the architectural test matrix described in
[10-test-driven-delivery-and-verification.md](10-test-driven-delivery-and-verification.md) and the reproducible quality
foundation described in [12-quality-gates-and-makefile.md](12-quality-gates-and-makefile.md). Every v1 principle above
requires at least one executable test or a justified documented exception.
