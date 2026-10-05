# ADR 0045: Local JSON-RPC 2.0 transport

## Status

Accepted 2026-09-30. It replaces the bespoke length-prefixed local protocol with JSON-RPC 2.0 over the existing local
socket, raises the protocol version to 2.0 with exact-match negotiation, and removes the negotiated capability plane and
the connection-role split. It activates no remote transport, no authentication layer, no second protocol version, and no
capability substitute.

## Scope and supersession

In scope is the local transport between the client and the daemon: `intention-protocol` message envelopes and the new
JSON-RPC module, `intention-transport` framing and handshake, the client and daemon serve loops, and the tests that
assert them.

Out of scope: the typed command and query payload DTOs themselves, which keep their current shapes; the public DTO
schema version; the TOML configuration and SQLite storage schemas; and every non-local transport.

| Record | Superseded clause | Replaced by |
| --- | --- | --- |
| [ADR 0035](0035-m5plus-complete-foundation-activation.md) | The Slice 1 negotiated capability families (`provider_profiles_v1`, `session_fork_v1`, `normalized_reasoning_stream_v1`, `agent_activity_v1`, `user_notifications_v1`, `daemon_tool_gateway_v1`, `model_tool_loop_v1`) | No capability plane exists; one protocol version and typed methods only |
| [ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md) | The local-protocol row of the version ledger, the "Protocol same-major compatibility" superseded commitment, and the Wave 3 instruction to keep the 1.1 negotiation gates | Protocol 2.0 exact equality; the single-version policy is unchanged |
| [ADR 0039](0039-request-side-tool-advertisement.md), [ADR 0040](0040-opt-in-live-provider-e2e.md), [ADR 0041](0041-same-run-reasoning-round-trip.md) | Their compatibility lines "Local protocol 1.1 ... unchanged" | Protocol 2.0 exact equality; the feature records otherwise stand |

The protocol-version clauses of ADRs 0039, 0040, and 0041 are read through this record. The M5+ Slice 2 control plane
was activated and then reverted (Slice 2 revert); the capability and connection-role surfaces this record removes were
the remaining live pieces of that negotiated plane.

## Decision

### Wire and framing

1. The local wire is JSON-RPC 2.0, serialized with `serde_json`, framed as
NDJSON: one JSON message per line, UTF-8, terminated by `\n`. A serialized JSON string never contains a raw line break,
so line framing is unambiguous.
2. The socket is unchanged: a local Unix domain socket or Windows named pipe
created through `interprocess`, with the existing `0700` directory and `0600` socket permissions, the stale-socket
reclaim probe, and the connect and synchronous IO timeouts. No network listener is added.
3. `MAX_FRAME_BYTES` (1 MiB) is retained under the name `MAX_MESSAGE_BYTES`
as a transport liveness cap: an over-size message fails closed instead of allowing a self-inflicted out-of-memory or
hang. It is not a contract limit on message content ([ADR 0048](0048-limits-by-precedent-and-no-content-scanning.md)).
4. The envelope DTOs are removed: `ProtocolMessageDto`,
`ProtocolRequestEnvelopeDto`, `ProtocolResponseEnvelopeDto`, `RunSubscriptionRequestEnvelopeDto`, and
`ProtocolDaemonFrameDto`. A new `intention-protocol/src/jsonrpc.rs` module defines the typed conforming envelopes
`JsonRpcRequestDto<T>`, `JsonRpcResponseDto<T>`, `JsonRpcErrorDto`, and `JsonRpcNotificationDto<T>`, with the required
`"jsonrpc": "2.0"` member, a `u64` correlation `id`, and `method` plus `params`/`result`/`error`.

### Handshake and version

5. The handshake carries exactly the protocol version and the adapter name.
There is no capability list in the handshake, no capability intersection between peers, and no per-feature negotiation.
6. The protocol version is 2.0 and negotiation is exact equality. A
mismatched peer receives a typed JSON-RPC error `-32001` with the typed `ErrorDto` in `error.data` before the connection
closes. This fixes the recorded diagnostics defect where an incompatible version closed without a typed reason.
7. The public DTO schema version, the TOML configuration schema, and the
single SQLite storage schema are unchanged. No payload DTO changes shape because of this record.
8. The capability plane is removed in full: `ProtocolCapabilityDto`,
`POST_M5_CAPABILITIES`, the family gates (`*_capability_required`, `model_tool_loop_required`,
`execution_meaning_capability_required`, and their peers), the duplicate-capability rejection, and every dependent
fail-closed branch. A feature is implemented or it is not; it is never gated on a handshake string.
9. The connection-role split is removed: `AsyncDaemonConnectionRoles`, the
`Ordinary`/`RunStream` roles, and `negotiate_by_capability`. One connection is one client with one method surface; a
subscription is an ordinary method call, not a connection mode.

### Methods, notifications, and errors

10. Each existing command and query variant maps to exactly one method:

    | Method | Payload |
    | --- | --- |
    | `session.create` | `CreateSessionCommandDto` |
    | `turn.send` | `SendUserTurnCommandDto` |
    | `turn.remove` | `RemoveQueuedTurnCommandDto` |
    | `run.stop` | `StopRunCommandDto` |
    | `session.subscribe` | `SubscribeSessionCommandDto` |
    | `run.subscribe` | `SubscribeRunCommandDto` |
    | `daemon.health` | `GetDaemonHealth` (no params) |
    | `session.snapshot` | `GetSessionSnapshotQueryDto` |

On the wire, `params` carries the existing typed `ProtocolRequestPayloadDto` and `result` carries
`ProtocolResponsePayloadDto`, each naming the payload variant listed above; no DTO changes shape. A table-driven
conformance test enforces the one-to-one coverage of every command and query variant.
11. After a `run.subscribe`, the server publishes `run.frame` notifications
carrying `RunStreamFrameDto`. Session subscriptions have no push channel: `session.subscribe` and `session.snapshot`
return their snapshot result, and a client that needs newer events subscribes again from its cursor. No
`session.frame` notification, push registry, or second connection purpose exists. Notifications carry no `id` and expect
no response.
12. The initial replay or resynchronization of a run subscription is the
ordinary correlated response result of `run.subscribe` (`RunSubscriptionResponseDto::Replay`, `Resync`, or `Error`).
There is no separate replay frame, replay method, or second connection purpose.
13. JSON-RPC errors are typed and conforming: `-32700` parse error, `-32600`
invalid request, `-32601` method not found, `-32602` invalid params, and `-32001` protocol-version mismatch. The typed
`ErrorDto` is carried in `error.data`, and its stable `data.code` stays the client-visible vocabulary for daemon
rejections.
14. The daemon sends notifications only and never server-initiated requests.
Batch arrays are not part of this transport.

## Invariants

1. One wire. JSON-RPC 2.0 over NDJSON is the only local protocol; no envelope
DTO, length prefix, alternative framing, or parallel path remains.
2. Exact version. Protocol 2.0 is the single live version, negotiated by
exact equality; there is no minor tolerance and no dual-protocol support.
3. No capability plane. No capability list, capability intersection, feature
gate, or capability-required error code exists, and none is replaced by an equivalent.
4. Typed payloads. Params and results are the existing typed serde DTOs; no
raw JSON passthrough or stringly typed dispatch bypasses them.
5. Typed errors. Every rejection is a JSON-RPC error carrying the typed
`ErrorDto` in `error.data`; the daemon never closes silently and never panics on malformed input.
6. One role. A connection has one role; subscriptions are methods whose later
frames are notifications.
7. Liveness kept. The message-size cap, connect and IO timeouts, socket
permissions, and stale-socket reclaim remain.
8. Local only. The transport remains the local socket; no network listener,
remote client, or authentication layer is introduced.

## Compatibility

The protocol version rises from 1.1 to 2.0 because the wire changed. Old daemons and clients fail closed with the typed
`-32001` mismatch error rather than misparsing a new message; no compatibility branch, fallback decoder, or protocol-1.1
path is kept, per the single-version policy of [ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md).
M3/M4/M5 durable runs, sessions, events, snapshots, cursors, and storage bytes are untouched by the wire change, and
replay is served through the same typed payloads as before. The live-provider end-to-end channel of [ADR
0040](0040-opt-in-live-provider-e2e.md) is re-pointed at the new wire and remains the manual, non-blocking live anchor.

## Security and failure behavior

The socket stays the only local IPC surface: no remote endpoint, no authentication layer, and no change to the file
permissions that restrict the daemon. Malformed, unknown, or over-size messages fail closed with a typed error or a
bounded transport failure; they never panic, hang, or allocate without bound. The version-mismatch error discloses only
the two version values and the adapter name; no path, credential, or daemon internals are echoed. Removing the
capability plane removes the capability-gated branches and the risk that a handshake string is treated as authority.

## Non-goals

No remote, TCP, HTTP, or WebSocket transport; no JSON-RPC batch support; no Content-Length (LSP-style) framing; no
authentication, authorization, or encryption layer inside the protocol; no transport-level compression; no capability
substitute such as feature flags, method probing, or version ranges; no payload DTO, public DTO schema, storage, or
configuration change; no second protocol version and no compatibility shim.

## Affected documents

[Architecture 02](../architecture/02-dto-and-contract-policy.md) owns the DTO and contract policy the typed payloads
follow; [architecture 03](../architecture/03-daemon-transport-and-adapters.md) the daemon transport, handshake, methods,
and error mapping; [architecture 10](../architecture/10-test-driven-delivery-and-verification.md) and [architecture
12](../architecture/12-quality-gates-and-makefile.md) the conformance tests and gate policy; [architecture
11](../architecture/11-implementation-roadmap.md) and the [architecture README](../architecture/README.md) the protocol
version and the removal of the capability plane.

## Evidence

The transport is accepted only together with: conformance tests for the JSON-RPC envelope, the four standard error
codes, the version-mismatch `-32001` error, notification framing, and the one-to-one command and query method table;
rewritten `transport_integration`, client, daemon, facade, and run-streaming tests that speak JSON-RPC 2.0 over the real
socket, including the Windows named-pipe path; updated or removed hello goldens that no longer carry capabilities; and
one live `make e2e-real-api` run (2/2) after the change, proving the protocol rewrite did not break the ordinary
production path. Gates: `make quick`, `make verify`, `docs-check`, Linux/Windows CI.

## Research provenance

The JSON-RPC 2.0 specification; the existing local socket, permissions, stale-socket probe, and timeout behavior; the
recorded diagnostics defect where an incompatible hello closed without a typed reason; and the cleanup-branch audit that
found the hand-written envelope protocol, the capability plane, and the connection-role split to be the source of the
removed complexity.
