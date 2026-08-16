# Run Execution Meaning and Historical Compatibility

## Status and scope

**Approved future architecture, documentation-only.** This document is the sole
detailed owner for immutable execution meaning, canonical semantic identity,
decoding, and execution/replay/audit compatibility. It does not authorize a
crate, schema migration, protocol frame, provider driver, scheduler, or runtime
implementation.

It preserves M3/M4 bytes and behavior. Existing records remain ordinary under
their recorded semantics and do not acquire a future envelope.

## Ownership

| Owner | Responsibility | Not responsible for |
| --- | --- | --- |
| Domain | Envelope/payload DTOs, tags, canonicalization, digest validation, semantic decoders and compatibility classes. | Storage resources or concrete drivers. |
| Storage | Atomic binding/read contracts, additive bridge/migration behavior and byte preservation. | Meaning selection or canonical policy. |
| Protocol | Later negotiated safe projections only. | Raw canonical bytes or semantic ownership. |
| Provider/tool/context owners | Their nested selection versions and validation. | Execution-kind selection or fallback. |
| Application/runtime | Admission selection and pre-effect compatibility enforcement. | Concrete implementation selection. |
| Composition | Resolve validated concrete implementations. | Rebuilding stored meaning. |
| Daemon | IDs, tasks, reread and publication. | Semantic selection discretion. |
| Adapters | Typed compatibility outcome display. | Local inference or repair. |

The Mandate aggregate owns immutable meaning reference/binding. The admitted Run
owns execution evidence and references that binding, but cannot alter or
reinterpret it. Mandate sequence, Session sequence and Run cursor remain
separate.

## Closed execution envelope

```text
RunExecutionKindDto
  Ordinary
  Mandate
  VerifierMandate

RunExecutionMeaningEnvelopeV1
  1 execution_kind
  2 meaning_record_tag
  3 meaning_record_version
  4 canonicalization_version
  5 canonical_meaning_bytes
  6 canonical_meaning_digest
```

The tuple `(envelope version, execution kind, record tag, record version)` is
closed. The digest is SHA-256 of exact canonical meaning bytes. Its lower-case
textual form is `<namespace>:sha256:<64 lowercase hex>`; the digest field is not
included in its own digest input.

| Kind | Valid payload |
| --- | --- |
| Ordinary | Exact supported ordinary meaning version or explicit later ordinary bridge. |
| Mandate | `MandateRunExecutionMeaningV1`. |
| VerifierMandate | Mandate meaning plus immutable verifier authority, target set, operation, baseline, audit/evidence and reconciliation selections. |

Unknown kind/version, wrong tag, kind/payload mismatch, malformed/noncanonical
bytes, digest mismatch, unavailable nested selection or unsupported executable
semantics block dependent work before provider, tool, process, kernel, MCP,
network, child, bridge, or scheduler effects. They leave unrelated replay and
audit history readable where its own records remain supported. No state may
infer a kind from a model name, provider, current configuration, registry,
ancestry, Goal, Skill, MCP source, bridge, kernel, prompt or adapter.

## Mandate execution meaning

`MandateRunExecutionMeaningV1` is an independent credential-free canonical
record with an owner-defined tag, V1 fixed field table, SHA-256 digest, and
explicit disabled variants for optional selections.

| Field | Semantic selection | Owner or state |
| ---: | --- | --- |
| 1 | Mandate selection | Mandate lifecycle |
| 2 | Provider selection | Provider evolution |
| 3 | Model capability selection | Provider evolution |
| 4 | Mandate activity selection | Later activity package |
| 5 | Context projection selection | Later context package |
| 6 | Direct tool selection | Tool registry and Mandate tool-loop package |
| 7 | Goal context selection | Later Goal package |
| 8 | MCP selection | MCP lifecycle package |
| 9 | Verifier selection presence | Architecture 17 |
| 10 | Verifier selection when present | Architecture 17 |
| 11 | Child-link selection presence | Architecture 17 |
| 12 | Child-link selection when present | Architecture 17 |
| 13 | Terminal provenance references | Mandate/terminal owner |
| 14 | Skill selection | Skill package |

`MandateSelectionV1` freezes Mandate identity/revision, trigger reason,
service-session/activity context where defined, verified checkpoints and
continuation configuration. Goal, Skill, MCP and child references are immutable
non-authorizing context/provenance. Every optional nested selection is exactly
`Disabled` or `Selected`; absence is not an ambiguous default.

Ordered collections preserve declared semantic order and that order is
digest-significant. Set-like fields define a canonical sort key and reject
duplicate semantic keys. Nested records bind their typed identity/revision and
canonical digest, not mutable current owner state.

A VerifierMandate payload cannot downgrade to Mandate/Ordinary when any required
verifier selection is missing, corrupt or unsupported. Architecture 17 owns
verifier and child-link nested field semantics; it binds immutable authority,
target/baseline, edge, and delegation references without changing this document's
canonical/decode ownership.

## Canonical record and digest policy

```text
CanonicalRecordV1
  magic = IRCR
  canonicalization_version = typed-tlv-v1
  record_tag
  record_version
  strictly ordered field stream
```

Canonical framing is exact: `IRCR` is four ASCII bytes;
`canonicalization_version`, `record_tag`, and `record_version` are unsigned
big-endian `u32`; each field is unsigned big-endian `u32` field number,
one-byte wire type, unsigned big-endian `u32` value length, then exact value
bytes. Field numbers are positive and strictly increasing. The initial wire
types are `u64`, `bool`, `utf8`, `uuid`, `digest`, `bytes`, `record`, `list`,
and `optional`; their individual scalar/list encodings are fixed by the owning
field table. Unsigned values use minimal big-endian value bytes with zero encoded
as one `0x00` byte. Booleans are exactly `0x00`/`0x01`; UTF-8 text is validated
without trimming, locale folding, or normalization unless that field explicitly
requires normalization; UUIDs use canonical sixteen-byte form; and closed enums
use stable numeric discriminants.

Optional values have an explicit one-byte presence marker followed by their
value when present. Lists have an unsigned big-endian `u32` count followed by
ordered typed elements. Nested records carry their full framing, never an
untyped blob. Field tables identify required/optional fields, types, order,
owner, intrinsic encoded-size/nesting bounds and digest namespace.

Tags are domain-owned, globally stable and never reused. A tag identifies one
record family forever; a version identifies one exact field table and semantic
interpretation. Semantic changes require a new version or tag. Unknown fields,
zero/duplicate/descending fields, noncanonical list/set order, invalid scalar
forms, trailing bytes, or over-limit bytes are invalid for execution.

Canonical bytes must never derive from JSON/maps, debug output, Rust declaration
order, hash iteration, platform paths, native provider data, credentials,
handles, raw content or mutable current state. A digest validates framing,
tag/version, field table, nested records, recomputation and envelope agreement.
Digest equality alone never authorizes replacing different canonical bytes.

## Admission binding and no fallback

Admission atomically binds a new RunId to selected Mandate revision/reason,
`MandateSelectionV1`, complete envelope/digest, projections, events, snapshots,
Mandate sequence and idempotency evidence. It happens before dependent external
work. Equal operation identity and semantic digest return the existing binding;
changed reuse conflicts before another reason is consumed or another run exists.

The only legal sources of meaning are persisted canonical records, their typed
immutable nested references, and an explicit versioned bridge. No admission,
replay, recovery, fork, bridge, child, verifier, provider or audit path may fill
missing meaning from current TOML/config snapshot, registry/descriptors, catalog,
model name, endpoint/credential, provider availability, ancestry/activity/Goal,
filesystem/process/kernel/bridge/network state, logs, UI state or remote
continuation state.

## Compatibility classes and decode outcomes

Decode structural canonical bytes, then typed tag/version, semantic validation,
and operation compatibility. The closed result is one of `Supported`,
`ReadableNotExecutable`, `UnknownTag`, `UnknownVersion`, `Corrupt`,
`DigestMismatch`, or `KindPayloadMismatch`.

### Execution compatibility

Execution requires valid canonical/digest/kind/payload, executable nested
versions, supported exact driver contract, required references and present
capacity/readiness. Availability may refuse/defer pre-effect work but never
mutates, reroutes or rebuilds meaning. Incompatible/corrupt meaning blocks
before effect.

Scheduler readiness is live operational evidence outside canonical execution
meaning. It may defer fresh admission but cannot repair, reroute, replace, or
reconstruct frozen meaning from a current scheduler, time zone, configuration,
registry, provider, model, or resource state.

### Replay compatibility

Readable history may replay while execution is unavailable. M3 session replay
and M4 run streaming remain unchanged. Future Mandate replay is separately
negotiated and Mandate-sequence ordered. Unknown future replay facts produce
typed resync/history-unavailable, never partial reduction. Unnegotiated clients
fail closed rather than receiving a partial ordinary snapshot.

### Audit compatibility

An unavailable audit record is isolated: unrelated projections/history remain
usable, no replacement fact is synthesized, no other sequence advances, and raw
corrupt bytes never appear in errors, protocol, logs or diagnostics.

Every claimed executable decoder retains exact golden fixtures. Decoder removal
is an explicit future compatibility decision. A readable record is never
implicitly executable.

## Historical M3/M4 and additive bridges

M3/M4 `EventEnvelopeDto`, projections, snapshots, `ConfigSnapshotDto`, UUID
`ConfigRevisionId`, queue tickets, cursors, facts, replay and provider selection
remain byte/meaning compatible. M4 `ToolCallRecorded` remains evidence followed
by `tool_execution_unavailable`, never historical tool execution. M4 provider
kinds remain `openrouter` and `generic-chat-completion-api`; an opaque model ID,
including `gpt-*`, `o*` or `codex*`, never changes that fact.

A later concrete consumer may define `LegacyOrdinaryRunBridgeV1` referencing
legacy identity/source bytes and schema class. It cannot copy, normalize,
replace or digest-reidentify those bytes; create Mandate/verifier/Skill/MCP/child/activity/profile/policy/tool-loop state; make a legacy run Mandate-executable; or be synthesized from current configuration/registry. This package creates no bridge or migration.

## Provider selection compatibility boundary

For future meaning, provider selection is a credential-free non-authorizing
nested record that freezes descriptor kind/revision, byte-exact model ID,
normalized endpoint where applicable, safe credential-transport metadata,
validated model-capability intersection, execution policy, driver-contract
revision and immutable provenance.

A model ID never selects provider kind, driver, endpoint, protocol, capability,
credential transport or execution kind. The frozen capability intersection is:
kind descriptor maximum ∩ explicitly declared model subset ∩ driver support.
Unknown taxonomy, invalid intersection or unsupported driver preflights before
outbound work.

`ProviderDriverContractRevisionDto` is code-owned family plus `major.minor`.
Incompatible request/normalization/order/capability/credential-transport changes
require a new major. Older minors require explicit support and fixtures. Live
credential/resource/health/catalog availability is distinct from compatibility:
it may refuse/defer work but cannot rewrite meaning, reroute a run, substitute a
default, or consume a trigger as if execution happened.

`responses`, parse-time `openai` aliasing, profiles/catalogs, reasoning,
discovery, credentials and live reload remain deferred. No M4 provider behavior
changes here.

## Persistence and protocol boundary

Future storage binds canonical bytes/digest and run/meaning reference additively
and verifies them on read. It preserves source bytes, never uses JSON as
canonical form, never updates canonical bytes in place, rejects future schemas
before readiness, and exposes DTO-only outcomes rather than rows/byte buffers.
Exact tables, migrations, field tags on wire, pages and retention remain
deferred.

A future separately negotiated execution-meaning/Mandate protocol sends typed
safe projections only, never raw canonical bytes. It preserves correlated
initial replay and sequence-owned later frames; Session, Run and Mandate link
only through typed IDs. Existing M3/M4 protocol DTOs are not widened here.

## Dependencies and non-goals

This document does not define SQL/wire implementation, provider
profiles/Responses, scheduler topology,
child graph, verifier workflow, MCP, Skills/Goals, bridge/kernel, forks, UI,
crates, Cargo, feature/coverage policy, Makefile/CI or M4 changes. CON-003 and
provider evolution remain later-owner work. Tool-loop/registry, WorkspaceRoot,
and direct-admission policy are owned by architecture 15; scheduler/readiness
semantics are owned by architecture 16.

## Required evidence before implementation

A later implementation specification must define:

- canonical golden bytes/digests and negative kind/tag/version/field/order/
  digest/nesting/verifier vectors;
- deterministic decode/re-encode and Linux/Windows equality;
- M3/M4 schema-v1/v2 byte-preservation, replay and tool-denial fixtures;
- no-current-state-reconstruction cases across config, registry, provider/model,
  credentials, ancestry, MCP, process/kernel/bridge state;
- capability-intersection, model-name non-routing and driver-major/minor tests;
- admission fault rollback at record/binding/projection/event/snapshot/idempotency
  stages and no external work on incompatibility;
- recovery/no-resume, negotiated/unnegotiated replay/resync and audit-isolation
  fixtures;
- fake-secret absence from canonical bytes/digests/persistence/protocol/errors/
  logs/diagnostics/adapters; and
- end-to-end compatible admission, incompatibility, historical startup, known
  continuation, uncertainty/reconciliation and user-precedence outcomes.

Before code, declare exact crate/test-target/coverage/feature/architecture
fixtures, then require `make quick`, `make verify` and Linux/Windows CI.
