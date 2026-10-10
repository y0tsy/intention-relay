# DTO and Contract Policy

**Current policy.**

This document makes the DTO-first principle executable. It applies to all crate, process, persistence, provider, tool,
and presentation boundaries.

## Non-negotiable rule

A boundary communicates through an explicit, strictly typed DTO. Passing an implementation type across a boundary is a
defect, even when both crates currently compile in the same workspace.

A DTO is a stable contract, not merely any serializable struct.

## DTO categories

| Category | Purpose | Examples |
| --- | --- | --- |
| Command DTO | Requested state-changing work. | `SendUserTurnCommandDto`, `InterruptRunCommandDto`. |
| Query DTO | Requested read model or snapshot. | `GetSessionSnapshotQueryDto`. |
| Persistence DTO | Storage-safe representation of a record or projection. | `RunProjectionDto`, `MessageProjectionDto`. |
| Provider DTO | Provider-neutral model request/stream/error contract, including advertised tool definitions and the transient same-run reasoning attachment. | `ModelRequestDto`, `ModelToolDefinitionDto`, `AssistantReasoningDto`, `ModelEventDto`. |
| Runtime execution DTO | Immutable selected execution input, safe terminal outcome, and provider-neutral time and transient text-delta ports over injected provider/storage contracts. | `ModelRunExecutionInputDto`, `ModelRunExecutionOutcomeDto`, `ModelTimePort`, `ModelTextDeltaPort`. |
| Tool DTO | Typed tool invocation, bounded result and interruption outcome, and durable result evidence. | `ToolInput`, `ToolResult`, `ToolResultEvidenceDto`. |
| Config DTO | Parsed, validated, resolved, and revisioned TOML configuration. | `ResolvedConfigDto`, `ConfigSnapshotDto`. |
| Presentation DTO | Explicit adapter projection, if transport DTO is not appropriate for display. | None today; adapters render transport projections directly. |

## Type rules

### UUID roles and non-conversion

UUID is an encoding/value representation, not authority. Every public UUID value uses a domain newtype whose owner
defines generation, scope, validation, serialization, and idempotency behavior. UUID equality never establishes semantic
identity across Session, Run, activity, lineage, graph, operation, or diagnostic domains.

Deterministic UUIDv5 is permitted only where the owner freezes its namespace and name derivation. Daemon-assigned/random
UUIDs and deterministic UUIDv5 values are not interchangeable. Historical UUID bytes and meanings remain unchanged and
cannot be normalized into future records. UUIDs, operation identities, and diagnostic correlation IDs are distinct
classes and must not be converted or used as authority substitutes. There is no durable ordering authority beyond
SQLite row insertion order — `messages.id` for the transcript — and no record family may introduce an ordering
sequence.

### IDs

All IDs are domain newtypes. No public cross-boundary API accepts an identifier as a bare `String`, integer, or UUID
primitive.

```rust
SessionId
RunId
TurnId
ProjectId
WorkspaceId
ToolCallId
ConfigRevisionId
IdempotencyKey
MessageId
```

IDs must have a defined generation owner, parse/validation behavior, serialization representation, and error DTO.
`MessageId` is the transcript row identity and the one non-UUID member: its durable value is the positive SQLite
`messages.id` the single durable writer assigned, and it serializes as a JSON number. The
tool loop addresses model steps and tool groups by plain indices within their containing records rather than by
newtypes; provider-native tool-call identifiers remain private implementation state.

### Envelopes

Transport messages are the typed `intention-proto` wire DTOs: one typed request enum and one typed result enum, a
correlated rejection carrying `ErrorDto`, and uncorrelated committed frames (architecture 03). Persisted state is stored
as plain typed rows; there is no durable event envelope, event identity, or event sequence.

A DTO field may be optional only when its absence has an explicit domain meaning. Optional must not conceal an
unimplemented relationship.

### Errors

Errors crossing a crate/process boundary are structured DTOs:

```text
ErrorDto
  code: stable machine-readable code
  category: validation | policy | not_found | conflict | unavailable | internal
  message: safe human-readable message
  retry: never | immediate | delayed | manual
  correlation_id: optional `CorrelationIdDto` UUID diagnostic reference
```

`CorrelationIdDto` accepts only a canonical UUID string and is an opaque reference, not diagnostic content. An error
carries no dynamic detail payload: user-visible context is code-owned safe guidance, never a path, an OS error, a
command line, a stack trace, or file content.

Messages are code-owned safe guidance. Runtime data must never be interpolated into `message` or placed in a
map/`serde_json::Value`. `Display` remains exactly `code: message` and never renders correlation data.

Provider secrets, filesystem content not intended for display, raw stack traces, and SDK objects never appear in
`ErrorDto`.

## Prohibited contract leaks

The following must not cross a boundary as public inputs or outputs:

- `serde_json::Value` or loosely typed maps as a replacement for a typed schema;
-  raw SQL rows, connection pools, transactions, file handles, `PathBuf` with unvalidated semantic meaning, Tokio tasks,
channels, mutex guards, or closures;
- provider SDK request/response/stream types;
- Tauri commands, window handles, Svelte stores, terminal widgets, or presentation state;
- bare strings for domain IDs, modes, risks, statuses, tool names, or tool kinds;
- implementation error types that reveal secrets or topology.

Provider SDK request/response/stream types and raw `serde_json::Value` cannot cross a provider boundary. M4
model/provider contracts use validated text context, requested/declared capability DTOs, ordered stream facts, usage,
finish reasons, safe provider errors, and typed JSON-object tool-call text. A request advertises tool definitions as a
validated JSON Schema descriptor (`ModelToolDefinitionDto`), never as raw `serde_json::Value`; the advertised set is
transient request state with no durable representation ([architecture 08](08-model-protocol-and-providers.md)). A
request may additionally carry the current round's
accepted provider reasoning as transient same-run attachment state with no durable representation: the runtime may
attach it to the assistant tool-call message of the same-run continuation ([architecture
08](08-model-protocol-and-providers.md)), and it is never durable history,
message text, or cross-turn transfer. `intention-proto` owns the shared safe usage, finish-reason, tool-call, and
provider-error values; `intention-providers` re-exports them for its consumers. Native SDK decoding and JSON values
may exist only inside the owning provider implementation before being normalized to those DTOs.

Run-scoped delivery is a current-state stream: `SubscribeRunCommandDto` receives its correlated result as the current
run state, and subsequent uncorrelated `RunStreamFrameDto` values carry `kind` `content` (one committed transcript row)
or `kind` `status` (the committed `RunProjectionDto`, never a delta) with no positions or cursors. A re-subscribing
client re-reads current state and continues live; there is no event tail, resync, or catch-up position.

## Validation ownership

Contracts are typed serde JSON DTOs. Public DTO deserialization is the structural validation boundary: required fields,
field types, and closed enum variants are established on decode and cannot be bypassed by a decoder. Semantic invariants
are enforced at the boundary that owns them and again at admission before any effect, because an admitting authority
cannot assume the producer decoded through the same boundary. The in-process model DTOs of `intention-providers` are the
recorded exception: they declare no decoder, and their typed constructors are their validation boundary.

There is no binary canonical form: the wire and domain contracts are the serde JSON DTOs themselves, and no tag
registry, canonical digest, or identity layer exists. Canonicalization is introduced only when a first real
consumer needs canonical bytes, and then only as RFC 8785 JSON Canonicalization (`serde_json_canonicalizer`); no
canonicalization dependency is added before that consumer exists. The public DTOs that declare invariants beyond their
field types are validated at admission; extending decode-time enforcement to them remains a recorded follow-up card.
Until a real canonicalization consumer exists, no component hashes JSON for identity, addressing, or comparison:
equality is typed structural equality, and record identity is a typed revision identity rather than a digest.

Validation occurs at the boundary that has the necessary context.

The error category follows the boundary that detected the mismatch: a peer that does not speak the live local wire
fails as the typed `stale_daemon_protocol` error with `ErrorDto { category: unavailable }` (architecture 03, "Protocol
lifecycle"), while a malformed request line is a `validation` failure answered with a typed identity-less rejection. The
difference is intentional.

Validation cannot be delegated only to UI. Tauri and TUI may provide ergonomic pre-validation, but daemon validation is
authoritative.

## Command lifecycle

```mermaid
sequenceDiagram
  participant A as Adapter
  participant C as Client
  participant D as Daemon
  participant U as Use case
  participant S as Storage

  A->>C: Typed operation
  C->>D: Typed request
  D->>U: Application DTO
  U->>S: State-change DTO
  S-->>U: Committed values
  U-->>D: Domain result
  D-->>C: Typed result and committed frames
  C-->>A: Typed result and frames
```

A live frame is published only after the storage commit succeeds. The command result and frames must include enough
typed identity for an adapter to reconcile state.

## Versioning and compatibility

- Every transport schema and persisted row schema has exactly one live version.
- Changes are additive by default. Current payloads may omit additive fields
such as `ErrorDto.correlation_id`; omitted fields decode as `None`.
- Public DTOs tolerate unknown additive JSON fields unless a closed
configuration schema explicitly documents `deny_unknown_fields`. Required fields, invalid types, invalid IDs, unknown
closed variants, and any stored schema other than the current one always fail safely.
- The typed local wire has exactly one live version, carried by the platform-default endpoint name byte
([architecture 03](03-daemon-transport-and-adapters.md)): there is no negotiated protocol version, no handshake, and no
per-payload `schema_version`. Configuration and storage keep their own explicit schema versions.
- SQLite storage is the single live schema (logical version 1) created
directly on open under one integer schema stamp; a database that does not carry the current stamp is discarded and
recreated from scratch, that discard is the only version gate, and no migration chain or older-schema open exists, while
persisted rows keep their recorded meaning.
- Provider DTOs are versioned independently from provider SDK models.

## Contract tests required before implementation

-  typed wire fixtures for every request and result family, including malformed and foreign-byte cases;
- invalid-shape and invalid-ID tests at every input boundary;
- explicit wire-validation tests for non-blank, path, timestamp, and schema invariants;
- schema compatibility fixtures for transport and persisted records;
- compile-time tests proving forbidden implementation types do not appear in public signatures where tooling permits;
- consumer-driven contract tests for `intention-client` against daemon transport;
- redaction tests for all error and stream DTOs carrying configuration or provider context.

## Quality-gate integration

DTO compatibility, validation, redaction, and public-API boundary tests are blocking `make verify` inputs. Every
DTO-owning crate is subject to its declared coverage tier, and contract fixtures run in the single feature
configuration.
See [12 Quality Gates and Makefile](12-quality-gates-and-makefile.md).

## Outcome criteria

A feature is not ready unless a Tauri bridge and TUI/REPL can invoke its same public command/query DTOs and interpret
the same resulting snapshot and frame DTOs without adapter-specific business rules.

See [03 Daemon, Transport, and Adapters](03-daemon-transport-and-adapters.md) for the process boundary, [10 Test-Driven
Delivery and Verification](10-test-driven-delivery-and-verification.md) for mandatory test layers, and [12 Quality Gates
and Makefile](12-quality-gates-and-makefile.md) for the blocking orchestration contract.

## Slice 1.5 boundary model (activated)

Slice 1.5 collapsed the DTO surface to the three places where a process, a file, or a foreign SDK forces a serialized
shape. This section records the landed model; the rules above are the live policy.

- Three physical boundaries. DTOs exist at the IPC wire (protocol commands, queries, notifications, and their typed
  payloads), at SQLite persistence (rows and projections), and at provider SDK calls (requests, stream facts, and errors
  normalized into the provider-neutral contract). Every DTO category above belongs to exactly one of the three.
- Domain types inside the process. Crates call each other with domain types. A DTO is not introduced between internal
  crates, and a type acquires wire attributes only when it crosses one of the three boundaries.
- JSON only for tool payloads. Tool inputs and outputs are JSON objects validated at runtime against the tool's
  declared JSON Schema, and they are the only schemaless JSON values in the system. `serde_json::Value` stays
  prohibited everywhere else, including errors, configuration, and storage.
- Nine identifiers. `SessionId`, `RunId`, `TurnId`, `WorkspaceId`, `ProjectId`, `ToolCallId`, `ConfigRevisionId`,
  `IdempotencyKey`, and `MessageId` are the complete identity newtype set. `MessageId` is the durable transcript row
  identity, assigned by the storage boundary from the SQLite `messages.id` row id. `EventId`, `AssistantTurnId`,
  `PlanId`, `PlanRevisionId`, `ModelStepId`, and `ToolGroupId` are removed: model steps and tool groups are addressed
  by plain indices within their containing records, and mutating operations carry an `IdempotencyKey` instead of an
  operational ID.
- Publication follows the commit. A live frame is emitted once the durable commit succeeds, carrying the values the
  commit recorded; the scoped durable reread proof is removed.
