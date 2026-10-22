# MCP Connection Records

**Approved future design. Not implemented; activation requires an activating specification.**

Owner: architecture 18.

This document owns future MCP connection administration, discovery, bounded per-connection method lists, direct
invocation, safe projections, disposal, recovery, and compatibility. It applies only to future run execution through
the fixed `mcp` `ToolId`.

## Ownership and non-authorities

The removed execution-meaning envelope, canonical framing, decoders, and compatibility classes
([architecture 02](02-dto-and-contract-policy.md)) leave no
live path. Architecture 15 owns the fixed `mcp` `ToolId`, composition-only activation, direct
admission, `ToolCallId`, generic tool loop, and effect/publication boundary.

An MCP server, discovery response, schema, method, result, or error is evidence/data only: it cannot create or mutate
a `ToolId`, tool spec, lifecycle, `RunId`, Goal, Skill, `ask_user`, child, session, ordinary turn input, or provider
selection.

## One fixed tool and one connection record

`mcp` stays one fixed `ToolId`; the model reaches it as an ordinary architecture-15 tool under the run's frozen
selection, admission, tool loop, and post-commit publication rules. MCP never creates another ToolId, tool spec,
plugin, direct primitive path, daemon, registry, or authority. MCP carries no preloaded catalog and no quota gate, and
it preserves the one gateway, typed boundary, private resources, redaction, publication after commit,
cancellation/disposal, and no-resume law.

This document owns the field semantics of one user-editable, credential-free connection record, mirroring the
provider endpoint rules ([architecture 22](22-provider-evolution-profiles-and-reasoning.md)):

```text
McpConnectionV1
  connection_id
  scope = Project | Session
  transport = Http | Stdio
  endpoint
  private_credential_reference
```

The record is typed serde JSON ([architecture 02](02-dto-and-contract-policy.md)) and is not immutable history: the
user alone creates, edits, archives, and restores a connection, and a model may only prepare a draft under the selected
proposal rules. `endpoint` is the user's own visible, credential-free execution and presentation metadata, validated
exactly as a provider endpoint is ([architecture
22](22-provider-evolution-profiles-and-reasoning.md)): an absolute `HTTPS` URL with a non-root API base, and a raw or
secret-bearing URL is never durable identity. The one opaque credential lives only in the daemon's private loading
boundary ([architecture 09](09-configuration-security-and-observability.md)); the reference is resolved there for a call
and is never a token, key, keychain locator, header, SDK value, socket, process handle, raw frame, raw result, or server
error body, and none of those enters a DTO, log, diagnostic, card, model context, or safe result. The selected transports
are user-created outgoing `HTTP`/`HTTPS` connections and user-created local standard-input/output services. A connection
edit takes effect for later calls and never retargets an already-committed call or an already-sent model step.

## Discovery and the frozen method set

The daemon discovers a connection's methods at connect or run start into one bounded per-connection method list: each
entry names one remote method and its input JSON Schema text. The server's schema is
parsed as JSON, bounded in size, and passed through as text; there is no closed typed schema family, no normalization
into typed families, and no JSON Schema validator beyond the parse. A malformed or over-bound schema makes that method
unavailable and fails with `mcp_schema_mismatch`; every other schema is carried unchanged.

The run's frozen architecture-15 tool selection records the method set the model saw, as safe connection references,
method names, and schema text. A missing or unknown selection is never rebuilt from a later discovery, current
configuration, server state, or UI state ([architecture
15](15-tool-registry-and-model-tool-loop.md)); a method discovered later cannot change an already-sent model step or an
already-committed call.

## Direct invocation, results, and safe projection

An invocation is one direct architecture-15 call: the connection reference, the method name, and typed arguments. It
validates the run's frozen selection, the current connection record, the typed input, intrinsic bounds, cancellation
state, and live availability, and it commits the binding of that call's `ToolCallId` to its connection and method in
the same transaction as the tool-call row, before external work. `Started` is recorded immediately before irreversible
dispatch; no external work occurs inside a transaction. A method absent from the connection's
current method list or from the run's frozen selection fails `mcp_method_unavailable` before effect; an archived,
removed, or unavailable connection fails `mcp_connection_unavailable` before effect. The remote server's own
idempotency support is evidence only: it cannot authorize automatic retry, status lookup, replay, or a claim of safe
repetition.

Only descriptor-owned safe projections cross the boundary:

```text
McpMethodSummaryV1
  safe_connection_identity
  connection_reference
  bounded_method_name
  schema_text

McpResultProjectionV1
  connection_and_method_reference
  known_terminal_class
  bounded_validated_typed_result
```

Raw JSON/maps, headers, protocol frames, server errors, credentials, SDK values, sockets, process resources,
provider-native IDs, and unsafe resource details never leave the MCP boundary; the endpoint is the user's own visible
configuration and carries no credential. A validated result, known protocol failure, known remote failure, known
connection refusal, or unrepresentable bounded projection with durably proven remote terminal effect is known; an
unrepresentable result after dispatch without that proof is not pre-effect incompatibility but a started effect without
terminal proof.

## Cancellation and recovery

Availability is checked at call time against the current connection record and the run's frozen selection; an
unavailable connection or method is the typed failure below, and no separate readiness evidence type, probe, or
pre-flight state exists. Invocation is tool-loop work after admission.

Before `Started`, cancellation or restart records a known before-start outcome and no MCP effect occurs; after
`Started`, only durable terminal proof makes the result known, and otherwise the interrupted invocation commits a
bounded `Partial` result with its notice. A partial result pauses nothing: the next model step proceeds, and the
interrupted call is never repeated. Cancellation prevents later calls/model steps, suppresses late facts, and disposes
private resources without asserting rollback.

A local stdio service is a private daemon-owned process per connection, started lazily on that connection's first call
and terminated on connection archival, idle disposal, failure, or daemon shutdown. It is never shared across
connections, never adopted or reattached after restart, and never treated as durable execution authority. HTTP connections and local
process epochs are private daemon resources, never durable execution authority.

MCP recovery validates committed MCP records, classifies unfinished calls, disposes private resources, establishes
fresh availability without dispatch, and never reconnects, polls, reattaches, respawns, retries, resumes, rediscovers, or
repeats old discovery/invocation work. A fresh run imports no live process, connection handle, credential, or method
list; it discovers again against the current connection record, and earlier discovery remains audit evidence only.

## Child, protocol, and compatibility boundaries

A child's MCP calls use the same connection records through ordinary architecture-15 admission, and a child run has its
own frozen method selection. A child run may receive only explicit safe connection/provenance references, never a live
process, connection handle, credential, method list, invocation, or unfinished effect; parent controls cannot invoke,
widen, or inspect private resources, and an interrupted child MCP call's partial result remains child-local.

Future MCP projections use typed requests ([architecture 03](03-daemon-transport-and-adapters.md))
layered with the `model_tool_loop_v1` descriptor/model
capability: live notifications through one post-commit gate; reconnect re-reads current state, and there is no event
tail, cursor, or resynchronization. No caller receives a partial ordinary snapshot. No MCP record carries state it did
not record, and no current state reconstructs meaning a record never had: a database without the current storage stamp
is discarded whole rather than opened, and no record, server, endpoint, credential, schema, registry, configuration,
ancestry, Goal, Skill, UI, log, or remote continuation state supplies missing MCP meaning.

## MCP detail: bounded gateway, bounds, and safe failures

The bounded MCP gateway is the canonical `mcp` gateway `ToolId`, owned by a future MCP boundary and assembled only
through the composition root's tool path and gateway. Its descriptor exposes only the run's frozen method selection:
the methods the current connection record named at admission, each naming exactly one connection and remote method,
with its bounded schema text. It is never an arbitrary method
name outside that selection, raw JSON transport, arbitrary header map, or automatic exposure of a method discovered
after admission, and a method whose schema is malformed or over-bound is unavailable.

There is no per-method approval step: a method the connection record exposes is available to the run's frozen
selection without a separate user decision. The connection record's scope, its user-owned create/edit/archive/restore
lifecycle, its credential privacy, and its transport selection are stated once in "One fixed tool and one connection
record"; the local service lifecycle once in "Cancellation and recovery".

Every MCP call passes the same frozen tool selection, daemon-bound authority, typed admission, durable outcome,
cancellation, redaction, and post-commit publication rules as another model-visible tool. The run's selection records
the method set the model saw, and the call resolves the current connection record; an archived or unlisted connection
or method fails before effect. An already started ambiguous call is never repeated and commits a bounded `Partial`
result with its notice when appropriate. A service cannot invoke `ask_user`, and it cannot create a Goal,
proposal, session, run, connection, tool registration, child agent, or another authority context. There is no
MCP listener, remote attachment to the daemon's ingress, plug-in system, Skill/MCP
installation, dynamic tool registration, server-driven child control, autonomous continuation, or claim that a local
service is isolated from the user's ordinary OS authority.

The MCP-related closed safe failures through `ErrorDto` are:

```text
mcp_connection_unavailable
mcp_method_unavailable
mcp_schema_mismatch
mcp_local_process_unavailable
```

They disclose no credential, path, raw external response, private process resource, or implementation detail. The
package continues to exclude autonomous continuation, work after client disconnection,
attachments/images/binary/rich-MIME input, dynamic extensions and installation, dynamic tool registration, physical
deletion, and administration of long-lived workers, leases, attach/detach, force-kill, or supervisor recovery.
Work/continuation/requeue after client disconnection is an accepted post-M5 future direction, to be executed in
Milestone 5+ as an explicit durable contract that never silently resumes old external work; it is not activated here.

## Dependencies and non-goals

This document depends on architecture 15. It defines no MCP server implementation, inbound listener, raw string-method
transport outside a selected method, arbitrary maps/headers, plugins/installations, dynamic ToolIds, long-lived
workers/supervision, provider evolution, bridge/IPython, Skills/Goals/context semantics, session forks, activity/UI,
schema, migrations, crates, Cargo, Makefile/CI, or production implementation. It introduces no product
depth/count/calendar/lifetime/concurrency ceiling: intrinsic representation/protocol bounds and actual finite resource
capacity remain separately typed, with no truncation, hidden retry counter, or quota.

Architecture 19 may carry safe MCP projections through its spawned-child ingress and delivery path but cannot discover,
invoke, reattach, or recreate MCP work independently. Architecture 20 kernel-originated MCP work still uses this
document's fixed `mcp` lifecycle through the daemon's typed client and tool loop, checkpoints contain no live MCP
state, and later runs discover again against the current connection record. Architectures 03 and 21-23 own
Goal/Skill/context, provider, fork, and activity semantics: context is safe non-authorizing projection only, provider
and MCP private credentials/resources remain separate and non-authorizing, no live MCP state crosses a fork, and
activity projections expose only safe MCP provenance without discovering, invoking, reconnecting, or recreating MCP
work.
