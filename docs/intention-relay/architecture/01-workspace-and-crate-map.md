# Workspace and Crate Map

**Current policy.**

This document assigns ownership within the Rust workspace and defines allowed dependency directions. It implements the
workspace principle in [00 Principles and Scope](00-principles-and-scope.md) and the DTO-first rule in [02 DTO and
Contract Policy](02-dto-and-contract-policy.md).

## Rules

- Each crate owns one cohesive responsibility and exposes DTO-based public contracts.
- Dependency cycles are forbidden.
-  An adapter may depend on `intention-client` and presentation-only crates, never on application, runtime, storage
implementation, tools, or provider implementations.
- Concrete drivers are selected only by the composition root.
- Feature flags may choose implementations, but may not make the public contract type-unstable.
-  M1 establishes the `ConfigRevisionId` and credential-free `ConfigRevisionDto` contract foundation. M3 makes
`ConfigRevisionDto` the canonical credential-free persisted configuration selection: the composition root supplies one
startup revision, storage records it by `ConfigRevisionId`, and each accepted run retains its immutable revision. TOML
is applied only at daemon startup; live reload remains deferred.
-  M3 activates the engine and storage layers, merged since Slice 1.5 into `intention-engine` and `intention-storage`. The
active graph adds the intentional `storage -> config`, `storage-sqlite -> config`, `application -> config`, and `runtime
-> config` edges required to persist and attach canonical configuration revisions without exposing credentials or
filesystem paths.
-  M4 activates the provider layer, carried since the Slice 1.5 collapse by `intention-providers`. The model
crate remains provider-neutral and depends only on `intention-proto`; provider crates depend only on model/config/types
plus their private SDK. `intention-proto` owns the provider-neutral `UsageDto`, `FinishReasonDto`, `ToolCallDto`, and
`ProviderErrorDto` shared by model and durable domain facts; `intention-providers` re-exports them for its consumers. Only
`intention` may select either concrete provider.
-  M4 model and provider evidence is domain-owned and current-state: domain, storage, and protocol never depend on
`intention-providers`, and SQLite stores assistant content, reasoning text, tool calls, and tool results on the transcript
and tool-result rows rather than in typed envelopes or per-run cursors. `intention-engine` depends on the
provider-neutral `intention-providers` contract only for its injected base execution service; it neither selects a concrete
provider nor exposes an async runtime resource.
-  M4 activates the daemon host as a private composition consumer. `intention-daemon` may depend on the composition
facade plus the DTO/application/runtime/model/protocol/transport/type crates needed to host selected execution and
streaming, and on private Tokio/future support. It never depends directly on a concrete provider or storage
implementation, selects no provider, and exposes no provider SDK, credential, Tokio, or storage resource in its public
contract.
-  M5 activates the typed tool/workspace/hook path. The composition root (`intention`) owns assembly of the six active
tools (`read`, `write`, `edit`, `execute`, `glob`, and `grep`) and the workspace/hook services; `intention-daemon` hosts
the application path but does not select implementations. The remaining registry slots are reserved and unavailable.

The M1-M5 activation notes are historical records: the coverage policy is now the per-crate tiers declared in
`quality/coverage.toml` ([12 Quality Gates and Makefile](12-quality-gates-and-makefile.md)).

## Planned crates

| Crate | Owns | May depend on |
| --- | --- | --- |
| `intention-proto` | ID newtypes, schema versions, common errors, time, envelopes. | Minimal shared dependencies only. |
| `intention-domain` | Domain DTOs, value validation, invariants. | `intention-proto`. |
| `intention-engine` | Commands, queries, semantic use-case workflows, and protocol-result mapping plus deterministic run execution, interruption handling, context-window accounting, and recovery-before-ready, over DTO-only storage. | Domain, storage, tools, providers, configuration revisions, proto. |
| `intention-storage` | DTO-only semantic repository methods, committed-change evidence, transcript and tool-result reads, and persisted configuration-revision inputs. | Config, domain, types. |
| `intention-config` | TOML parsing, validation, resolved configuration and revision DTOs. | Types, domain as needed. |
| `intention-providers` | Provider-neutral model DTOs and driver trait plus both SDK translation adapters. | Config, proto, and shared value types. |
| `intention-tools` | Registry and core tool contracts with JSON Schema descriptors, the typed hook phases and dispatcher, and the WorkspaceRoot addressing anchor. | Proto. |
| `intention-vfr` | VFR hook, mapping, expansion/raw-read tools. | Hooks, tools, domain, types. |
| `intention-headroom` | Headroom hook, CCR contracts, retrieve tool. | Hooks, tools, storage contracts, domain, types. |
| `intention-plans` | Plan artifacts, hidden frontmatter, Plan/Build policy hooks. | Hooks, tools, storage contracts, domain, types. |
| `intention-transport` | Socket/pipe framing, server/client protocol, subscriptions. | Protocol, types. |
| `intention-client` | Bootstrap, connection, dispatch, subscription, reconnect. | Protocol, transport, types. |
| `intention-daemon` | Daemon host binary: process lifecycle, private model-task registry, and typed connection/stream hosting. | Composition facade; application/runtime/model/domain/protocol/transport/type DTO contracts; private Tokio/future support. Never concrete provider or storage implementations. |
| `intention` | Composition root library: factories, dependency wiring, and daemon application facade. | All selected concrete implementations. |
| `intention-tauri` | Tauri bootstrap and native bridge. | Client, protocol, presentation DTO mapping. |
| `intention-tui` | TUI and REPL presentation adapters. | Client, protocol, presentation crates. |

## Dependency direction

```mermaid
flowchart BT
  TY[types] --> DO[domain]
  TY --> CF[config]
  DO --> ST[storage contracts]
  CF --> ST
  DO --> MO[model contracts]
  DO --> TL[tool contracts]
  DO --> PR[protocol]
  CF --> AP[application]
  CF --> RT[runtime]
  ST --> AP
  ST --> RT
  MO --> RT
  TL --> RT
  ST --> SQ[SQLite storage]
  CF --> SQ
  DO --> SQ
  AP --> IN[intention composition]
  RT --> IN
  SQ --> IN
  IN --> DH[daemon host]
  AP --> DH
  RT --> DH
  MO --> DH
  DO --> DH
  PR --> TR[transport]
  TR --> DH
  TR --> CL[client]
  CL --> TA[tauri adapter]
  CL --> TU[tui repl]
```

<!-- Arrows show permitted lower-level dependencies toward a consumer. Composition is the sole wiring location. -->

## Slice 1.5 target crate map (not activated)

Slice 1.5 collapses the workspace to ten production crates. This section freezes the target map; the table above stays
current policy until the slice activates, and the exact permitted-edge table plus per-crate test targets and coverage
tiers are declared by its activating specification (roadmap, slice 1.5).

| Target crate | Absorbs | Owns |
| --- | --- | --- |
| `intention-proto` | `intention-proto`, `intention-protocol` | Identity newtypes, shared value types, schema versions, the versioned public protocol DTOs, and their typed serde payloads. |
| `intention-domain` | `intention-domain` | Domain records, value validation, and invariants. |
| `intention-config` | `intention-config` | TOML parsing, validation, resolved configuration, and credential-free snapshots. |
| `intention-engine` | `intention-application`, `intention-runtime` | Commands, queries, semantic use-case workflows, deterministic lifecycle decisions, interruption handling, and recovery. |
| `intention-tools` | `intention-tools`, `intention-workspace`, `intention-hooks` | Tool registry and contracts, WorkspaceRoot policy, and the typed hook phases, contexts, and dispatcher. |
| `intention-providers` | `intention-model`, `intention-provider-openrouter`, `intention-provider-generic-chat` | The provider-neutral model contract and both concrete SDK translation adapters. |
| `intention-storage` | `intention-storage`, `intention-storage-sqlite` | Repository contracts and the bundled SQLite single-schema implementation. |
| `intention-transport` | `intention-transport` | Socket/pipe framing, the JSON-RPC server and client protocol, and subscriptions. |
| `intention-daemon` | `intention`, `intention-daemon` | Composition root, dependency wiring, process lifecycle, and typed connection and stream hosting in one binary crate. |
| `intention-client` | `intention-client` | Bootstrap, connection, dispatch, subscriptions, and reconnect, fully asynchronous and covering every protocol command and query. |

Retained outside the ten: the adapter crates `intention-tui` and `intention-tauri`, the future skeleton crates
`intention-vfr`, `intention-headroom`, and `intention-plans`, the non-production `intention-test-support` crate, and the
`quality/harness` tooling.

Rules the target map fixes:

- One composition location. The composition facade library disappears; `intention-daemon` is the only crate that selects
  concrete implementations and the only place that wires them, and the daemon host calls the engine directly.
- Boundaries own serialization. Only types crossing the three physical boundaries carry wire attributes: the IPC wire
  (`intention-proto` payloads served by `intention-transport`), SQLite rows (`intention-storage`), and provider SDK
  calls (`intention-providers`). Every other cross-crate call passes domain types.
- Tools exchange JSON only with the model. Tool inputs and outputs are schema-validated JSON; nothing else in the system
  carries an untyped JSON payload.
- Acyclic direction. Protocol and domain types are the lowest layer; configuration, storage, tools, and providers
  build on them; the engine builds on all of those; the daemon composes them; and no lower crate depends on
  `intention-engine`, `intention-daemon`, or `intention-client`.

```mermaid
flowchart BT
  PR[intention-proto] --> DO[intention-domain]
  PR --> CF[intention-config]
  DO --> CF
  PR --> ST[intention-storage]
  DO --> ST
  CF --> ST
  PR --> TL[intention-tools]
  DO --> TL
  PR --> PV[intention-providers]
  DO --> PV
  CF --> PV
  PR --> EN[intention-engine]
  DO --> EN
  CF --> EN
  ST --> EN
  TL --> EN
  PV --> EN
  EN --> DH[intention-daemon]
  ST --> DH
  TR --> DH
  PR --> TR[intention-transport]
  TR --> CL[intention-client]
  PR --> CL
  CL --> AD[adapter crates]
  DH --> BIN[daemon binary]
```

## M3 ownership decisions

-  `intention-storage` defines semantic, DTO-only operations such as create session, accept a user turn as started or
pending, remove a not-yet-seen pending turn, append pending turns to a run context, transition a run, recover
unfinished runs, read the transcript and tool results, and accept configuration revisions. It does not expose a
transaction closure, SQL connection, filesystem path, or backend resource.
-  `intention-storage` owns bundled SQLite opening and direct creation of the single current storage schema,
single-transaction state writes, and SQLite-only fault injection. It persists one canonical `WorkspaceId
-> WorkspaceRootDto` association; the workspace addressing policy — the root as an anchor, not a containment boundary
([architecture 05](05-tools-workspace-and-hooks.md)) — remains M5 policy ownership.
-  `intention-engine` decides valid run state edges and owns interruption handling: a stopped provider stream or tool
call records a notice, resets its signal, and the run continues with its next model step. The repository owns run
creation, pending-turn context joins, and recovery. It has no provider, tool, timer, or stream dependency in
M3.
-  `intention-test-support` is a non-production workspace crate. It owns credential-free fixture configuration, native
temporary roots under `std::env::temp_dir()`, `TempDir`-backed durable databases, deterministic sessions, and bounded
fixture listener orchestration. `intention` exposes only hidden `test-support` facade seams for an injected
database and current-state inspection; `intention-daemon` exposes only a hidden one-connection dispatch seam.
Release production APIs and the daemon binary expose no fixture mode.

## Composition rules

`intention` creates and connects:

- resolved TOML configuration;
- SQLite storage implementation;
- selected provider drivers;
- core tool registry;
- workspace, plan, VFR, and Headroom hook registrations;
- application facade and runtime actor factories;
- a daemon application facade that `intention-daemon` hosts over transport.

For M5, this is the ownership boundary for the active six-tool registry, `WorkspaceRoot`, and the typed hook dispatcher.
Base tools remain primitive implementations; application owns lifecycle/result persistence and publication, while hooks
transform or reject typed context without committing storage or publishing independently.

`intention-daemon` depends on this composition facade, never the reverse. Its M4 private host may consume
DTO/application/runtime/model contracts to own the task registry, cancellation, and streaming transport loop, but it
never imports or selects concrete provider/storage implementations. Composition selects concrete implementations; the
binary owns process lifecycle and typed connection hosting.

No other crate chooses a concrete SQLite driver, OpenRouter client, or adapter implementation by global construction.

## Binaries

Planned binaries are thin:

- `intention-daemon`: starts a configured daemon host.
- `intention-tui`: starts a terminal client and invokes shared bootstrap.
- a future administrative CLI may use `intention-client`, not daemon internals.

`intention-tauri` is a desktop integration crate/binary host, not a second daemon implementation.

## Architectural test requirements

The workspace must have tests that fail when these rules are broken:

1. no cyclic crate dependencies;
2. the required v1 crate set exists, and every crate has a single declared responsibility;
3. adapters do not import forbidden implementation crates or declare forbidden workspace/external dependencies;
4. only `intention` selects concrete storage/provider/tool extension implementations;
5. `intention-proto` has no dependency on Tauri, SQLite, provider SDKs, or UI crates;
6. public cross-crate methods accept/return DTOs, not implementation resources or provider SDK types;
7. every planned crate has a stated test target before implementation begins;
8. every declared boundary is enforced by the architecture checker against the machine-readable policy.

The exact test strategy and minimum test portfolio are defined in [10 Test-Driven Delivery and
Verification](10-test-driven-delivery-and-verification.md); the pinned tooling, coverage policy (per-crate tiers),
feature profiles, lint policy, and
Makefile/CI contract in [12 Quality Gates and Makefile](12-quality-gates-and-makefile.md).

## Non-goals

This map does not freeze individual module names, file names, Cargo feature syntax, or exact trait method signatures;
those are implementation choices that must preserve these ownership and dependency rules.
