# Daemon, Transport, and Adapters

## Scope

This document defines the local single-user daemon, the typed local process protocol, bootstrap/reconnect behavior, and the responsibilities of Tauri, TUI, and REPL adapters.

It does not define Web, Telegram, HTTP, WebSocket, remote authentication, or multi-user operation. Those are explicit v1 exclusions.

## Ownership

The daemon is the sole owner of:

- application use cases and runtime actors;
- SQLite connections and persistence transactions;
- active sessions and runs;
- provider drivers and model streams;
- tool registry, workspace policy, hooks, VFR, Headroom, and plan policies;
- event publication and subscription source.

Tauri, TUI, and REPL own only presentation, user input adaptation, local display state, and reconnect UX.

## Local transport

v1 uses:

- Unix domain sockets on supported Unix platforms;
- named pipes on Windows;
- a framed, versioned protocol carrying only `intention-protocol` DTOs;
- OS-user filesystem permissions as the local access boundary.

No TCP listener is opened in v1. This avoids treating localhost as an authentication boundary and keeps remote API design out of the initial product.

## Shared client

`intention-client` is the only supported client-side integration path for local adapters. It owns:

- daemon discovery and bootstrap coordination;
- protocol version negotiation;
- typed command dispatch and query execution;
- typed subscriptions;
- reconnect behavior;
- snapshot-plus-event-tail recovery;
- local transport error classification.

It must not contain Svelte, terminal, or domain workflow behavior.

## Bootstrap sequence

```mermaid
sequenceDiagram
  participant AD as Adapter
  participant CL as Client
  participant LK as Startup lock
  participant DM as Daemon

  AD->>CL: Connect or bootstrap
  CL->>DM: Try local socket
  alt Daemon is ready
    DM-->>CL: Protocol hello
    CL-->>AD: Connected client
  else Socket unavailable
    CL->>LK: Acquire startup lock
    CL->>DM: Retry local socket
    alt Another client started daemon
      DM-->>CL: Protocol hello
    else No daemon exists
      CL->>DM: Start process
      CL->>DM: Wait for readiness
      DM-->>CL: Protocol hello
    end
    CL->>LK: Release lock
    CL-->>AD: Connected client
  end
```

### Bootstrap requirements

1. The adapter first attempts a connection without spawning anything.
2. A process-wide startup lock prevents duplicate daemons.
3. The lock holder rechecks availability before creating a daemon.
4. Readiness includes successful protocol negotiation, not merely process existence.
5. Startup errors are typed and safe to render.
6. Closing one adapter never terminates a healthy shared daemon.
7. The exact idle-shutdown policy is deferred. The default assumption is daemon persistence until explicit stop or OS shutdown.

## Protocol lifecycle

At connection time, client and daemon exchange:

- protocol major/minor version;
- supported feature/capability DTOs;
- client identity limited to local adapter metadata, not an application account;
- last observed session event sequence for subscriptions, when available.

An incompatible major protocol version fails closed with `ErrorDto { category: unavailable }`. The adapter should offer a safe reconnect/restart action, never silently reinterpret mismatched payloads.

## Subscription and reconnect

A subscription is associated with a session and optional run scope. The client records the latest applied event sequence.

```mermaid
sequenceDiagram
  participant AD as Adapter
  participant CL as Client
  participant DM as Daemon
  participant ST as Storage

  AD->>CL: Subscribe from sequence N
  CL->>DM: Subscription DTO
  DM->>ST: Load snapshot and event tail
  ST-->>DM: Snapshot plus events
  DM-->>CL: Snapshot DTO
  DM-->>CL: Events after N
  CL-->>AD: Reconcile view
  Note over AD,DM: Connection is lost
  AD->>CL: Reconnect
  CL->>DM: Subscribe from last sequence M
  DM->>ST: Load events after M
  ST-->>DM: Tail or resync required
  DM-->>CL: Event tail or snapshot
  CL-->>AD: Consistent state
```

### Reconciliation rules

- Adapters render in sequence order per session.
- A gap requires recovery through a snapshot and event tail, not guessed UI state.
- Duplicate delivery is tolerated through `event_id` and sequence-aware reducers.
- A stale live event may not mutate a projection if its sequence is already applied.
- The daemon has authoritative ordering; adapters do not synthesize a server sequence.

## Tauri bridge

Tauri is a bootstrap/native bridge, not a domain host.

```text
Svelte UI → Tauri invoke/event bridge → intention-client → local daemon
```

The Rust bridge may:

- initialize `intention-client`;
- dispatch protocol command/query DTOs;
- forward typed event DTOs;
- map explicit presentation DTOs when necessary for JavaScript ergonomics;
- manage window, native dialog, notification, and app lifecycle details.

The bridge must not:

- import application use-case services directly;
- create a competing SQLite connection or run actor;
- invoke provider SDKs or tools;
- reimplement persistence, permission, plan, workspace, VFR, or Headroom policy;
- introduce a parallel Tauri-only command contract.

## TUI and REPL

TUI and REPL connect directly through `intention-client`. They are equal presentation adapters, not special daemon modes.

They must use the same command, query, snapshot, and event DTOs as the Tauri bridge. This is an intentional architectural proof that presentation logic is isolated.

## Daemon restart semantics

On daemon startup:

1. load current projections and durable records;
2. locate runs with an unfinished persisted status;
3. transition each to `interrupted` with a durable event;
4. do not automatically retry model calls, tool calls, or shell processes;
5. publish resulting state when an adapter subscribes.

This policy is honest about unknown external side effects. A user may initiate a new retry or a manually reconciled follow-up run.

## Required tests and observable outcomes

| Requirement | Test evidence | Observable result |
| --- | --- | --- |
| Shared daemon | Tauri bridge test and TUI client test connect to one fixture daemon. | Both observe the same session snapshot and event sequence. |
| Startup race | Multi-client bootstrap integration test. | Exactly one daemon host is created. |
| Permission boundary | Socket/pipe permission integration test. | A different OS user cannot connect. |
| Protocol mismatch | Client/server compatibility test. | Connection fails with typed incompatibility error. |
| Reconnect | Disconnect/reconnect sequence test. | Adapter recovers identical state via snapshot plus tail. |
| Restart | Persisted active-run recovery test. | Run becomes `interrupted`; no provider/tool call resumes. |
| Adapter isolation | Dependency/contract test. | Tauri and TUI have no direct runtime/storage implementation dependency. |

## Quality-gate integration

The daemon, transport, client, and adapter tests in this document are blocking `make verify` inputs. Their crates use the coverage tier, feature-profile matrix, strict lint policy, and architecture boundaries defined in [12 Quality Gates and Makefile](12-quality-gates-and-makefile.md). Tauri and TUI are accepted through mapping contracts and fixture-daemon outcome scenarios, not a misleading aggregate UI line-coverage target.

## Open implementation decisions

- exact framing codec and backpressure behavior;
- maximum subscription buffer and slow-client policy;
- local socket path conventions per platform;
- how daemon upgrades coordinate with a connected adapter;
- explicit daemon stop command and future idle shutdown policy.
