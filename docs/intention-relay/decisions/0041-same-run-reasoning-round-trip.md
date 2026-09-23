# ADR 0041: Same-run provider reasoning round-trip

## Status

Accepted as a Milestone 5+ retrospective completion of the ordinary production
model-tool loop ([ADR 0019](0019-production-model-tool-loop.md)) and of the
request-side translation completed by
[ADR 0039](0039-request-side-tool-advertisement.md), within the retrospective
scope of [ADR 0035](0035-m5plus-complete-foundation-activation.md). It makes the
ordinary same-run tool-loop continuation acceptable to a real Chat Completions
gateway in thinking mode by returning the current round's accepted reasoning as
transient request state. It is not a slice activation; the M5+ Slice 3
reservations and all reserved tags remain untouched and reserved.

## Scope and supersession

In scope is the ordinary production model-tool loop's same-run continuation
only:

- the generic Chat Completions adapter receives typed reasoning output from a
  configured thinking-mode model;
- reasoning deltas normalize as `Primary` reasoning fragments;
- the runtime attaches the current round's accepted reasoning to the assistant
  tool-call message of the same-run continuation;
- the generic adapter serializes that attachment on the assistant tool-call
  message of its private typed request;
- the generic adapter's declared `ModelCapabilitiesDto` gains reasoning output.

This record supersedes, in place and only for this scope, the
[ADR 0039](0039-request-side-tool-advertisement.md) Decision 5 sentence "Result
continuation is unchanged: assistant tool-call messages and tool-role result
messages already flow through the runtime loop activated by
[ADR 0019](0019-production-model-tool-loop.md)." Continuation now also carries
the same round's accepted reasoning when the selected model produced it. ADR
0039's body is unchanged, and the supersession is recorded only in this record.
Everything else in ADR 0039 Decision 5 remains true: assistant tool-call
messages and tool-role result messages still flow through the production loop
activated by ADR 0019.

The rest of the reasoning contract surface is unchanged. The normalized
reasoning DTO surface activated for M5+ Slice 2 by
[ADR 0037](0037-m5plus-slice2-control-plane.md) still owns the future
categorization, history, and delivery contracts; the dialect direction is owned
by [ADR 0028](0028-provider-reasoning-and-catalog-detail-directions.md) and the
provider evolution decision by
[ADR 0014](0014-provider-evolution-profiles-and-reasoning.md). The
[ADR 0036](0036-m5plus-slice1-contract-ledger.md) Slice 3 reservations
(`tool-descriptor-revision` `0x0301`, `tool-registry-revision` `0x0302`,
`model-tool-loop-v1` `0x0303`) remain `ReservedForSlice3`, and the
[ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md) single-version
policy is unaffected.

## Context

The live evidence for this decision is a real Chat Completions gateway in
thinking mode: a tool-loop continuation whose assistant tool-call message
lacked `reasoning_content` was rejected with HTTP 400 and the normalized
`request_rejected` failure, "The `reasoning_content` in the thinking mode must
be passed back to the API". Re-sending the exact production request with the
field added, even as an empty string, made the continuation succeed. The live
run executes through the opt-in channel of
[ADR 0040](0040-opt-in-live-provider-e2e.md); the controller records its date,
commit, provider kind, and model when the recorded run is available, together
with the local `quality/reports/real-api-e2e` run report for a local
`make e2e-real-api` run or the workflow run URL for a manual `workflow_dispatch`
run. This record claims no date, commit, or run identifier of its own.

## Decision

1. `intention-provider-generic-chat` enables the pinned `async-openai` 0.41.3
   `byot` feature and drives the streaming Chat Completions call through
   `create_stream_byot` with crate-private typed request and chunk structs. The
   pinned SDK keeps HTTP, SSE, TLS, and error mapping; no custom HTTP or SSE
   parser is introduced, no SDK type leaves the provider crate, and the adapter
   still owns its private wire types. This follows the authorizing clause in
   [architecture 22](../architecture/22-provider-evolution-profiles-and-reasoning.md):
   "The current `async-openai` core Chat Completions adapter is not assumed
   sufficient for every descriptor; a future implementation must choose a
   pinned private SDK or an explicitly specified private typed decoder per
   closed descriptor."
2. Reasoning deltas from the typed chunk stream normalize to
   `ModelEventDto::ReasoningDelta` fragments with category `Primary`. An empty
   channel is a documented presence marker that never becomes a durable fact:
   zero-length reasoning text creates no durable reasoning fragment, no
   snapshot content, and no message text.
3. `intention-model` gains a transient `AssistantReasoningDto` carrying the
   current round's tool-call ids and reasoning text, which may be empty. The DTO
   validates at least one unique tool-call identity and bounded,
   control-character-safe text. It travels on `ModelRequestDto` as
   `assistant_reasoning`; the runtime attaches each tool round's accepted
   reasoning to that round's assistant tool-call message, and later same-run
   continuation requests preserve the ordered per-round attachments. It is
   transient request state with no durable representation: it never becomes
   message text, durable history, a run snapshot, a canonical record, or
   adapter-visible state, and it never enters a different run.
4. The generic adapter's `ModelCapabilitiesDto` declares reasoning output
   support because reasoning output is consumed and preserved. Multimodal and
   vendor extensions still fail preflight before any outbound request is
   prepared; the reasoning declaration does not widen any other capability.
5. `intention-provider-openrouter` ignores the attachment by design. Its pinned
   SDK assistant-message type has no reasoning echo field, and its request
   exposes reasoning only through request-scoped options (`reasoning`,
   `include_reasoning`), so this record leaves that adapter's request shape
   unchanged; whether a routed upstream model would need its own reasoning echo
   remains out of scope for this record. The no-op is documented and tested.
6. Cross-turn and durable reasoning transfer remains future work (architecture
   22 / RSN-011..014). The runtime never sends prior-turn or fork reasoning to a
   provider, and a later turn's text-only context is accepted by the gateway
   (verified live alongside the 400 evidence); only the same round's own
   reasoning returns to its own continuation.

## Invariants

1. Only typed DTOs cross the provider boundary. The attachment is a validated
   `AssistantReasoningDto`; no SDK type, raw JSON value, or loosely typed map
   crosses it.
2. Transient only. The attachment creates no durable fact, digest, snapshot,
   storage row, event, or canonical record, and no adapter or client reads it
   as history.
3. Same-run only. An attachment is bound to its own round's tool-call
   identities and travels only within that run; prior-turn, fork, sibling, and
   current-state reasoning remain forbidden.
4. Presence is not content. Empty reasoning text is a presence marker only; it
   never becomes a durable reasoning fact, message text, or summary.
5. Transport ownership is unchanged. The pinned SDK owns HTTP, SSE, TLS, and
   error mapping; the adapter adds no custom HTTP or SSE parser, and the `byot`
   typed seam keeps all wire structs private to the provider crate.
6. Capability truthfulness. The generic driver declares reasoning support only
   because it consumes and preserves reasoning output; unsupported multimodal
   and vendor-extension requests still fail closed at preflight.
7. No slice advance. No frozen descriptor/registry revision, digest, execution
   meaning, or Mandate tool-loop contract is introduced, and the ADR 0036
   Slice 3 reservations remain reserved.

## Compatibility

- Local protocol 1.1, public DTO schema 1.1, TOML configuration schema 1, and
  the single live SQLite schema are unchanged. Reasoning attachment is
  provider-neutral request state, not a public wire family; no migration,
  version change, or durable representation is added.
- M3/M4/M5 recorded history, replay, durable facts, snapshots, and evidence
  remain unchanged. Historical runs gain no reasoning category, echo, or
  synthetic reasoning state.
- The OpenRouter adapter's behavior, request shape, and capability declaration
  are unchanged; drivers that do not consume reasoning remain valid for
  ordinary requests.
- The ADR 0038 single-version policy is unaffected: nothing opens an older
  schema or adds a compatibility branch.

## Security and failure behavior

- The attachment carries provider reasoning text only. Existing central
  redaction and the credential, provider-payload, SDK-resource, and diagnostic
  exclusion rules remain in force; no credential, path, or raw native payload
  is serialized.
- A gateway that requires the echo rejects a continuation without it; the
  runtime supplies the current round's accepted reasoning, and empty text may
  be carried only as the documented presence marker. There is no silent
  fallback, no cross-run substitution, and no invented text.
- The typed `byot` seam keeps transport failures in the pinned SDK's existing
  normalized error mapping; malformed, out-of-order, or unknown reasoning
  chunks follow the adapter's existing safe failure behavior.
- The live evidence is opt-in and manual under ADR 0040 and is never a blocking
  gate; hermetic tests remain the acceptance evidence.

## Non-goals

This decision does not add:

- cross-turn or durable reasoning transfer and manifests (RSN-011..014);
- other reasoning dialects, or any descriptor/profile/user-kind reasoning work;
- thinking activation fields (`thinking`, `enable_thinking`, `think`,
  `reasoning_effort`, `thinking_budget`, and related controls);
- the Responses API, a Responses driver, or remote continuation;
- reasoning summaries, semantic content inspection, or a new durable reasoning
  representation;
- changes to multimodal or vendor-extension preflight.

## Affected documents

- `decisions/README.md`
- `architecture/02-dto-and-contract-policy.md`
- `architecture/08-model-protocol-and-providers.md`
- `architecture/10-test-driven-delivery-and-verification.md`
- `architecture/11-implementation-roadmap.md`
- `architecture/22-provider-evolution-profiles-and-reasoning.md`
- `reconciliation/README.md`
- `reconciliation/source-of-truth-matrix.md`
- `reconciliation/contradiction-register.md`
- `reconciliation/concept-supersession-index.md`
- `reconciliation/compatibility-register.md`
- `reconciliation/evidence-register.md`
- `quality/architecture.toml`
- `quality/self_test.py`
- `crates/intention-model`
- `crates/intention-runtime`
- `crates/intention-provider-generic-chat`
- `crates/intention-provider-openrouter`
- `crates/intention-daemon/tests/real_api_e2e.rs`

## Evidence

Hermetic tests remain the acceptance evidence; the ADR 0040 live channel adds
the opt-in manual run.

| Requirement | Evidence anchor |
| --- | --- |
| Transient model contract round trip | `crates/intention-model/tests/model_contracts.rs`: `assistant_reasoning_validates_tool_call_identities_and_bounded_text`, `assistant_reasoning_round_trips_and_rejects_invalid_wire_values`, `model_request_assistant_reasoning_round_trips_and_survives_rebuilds` |
| Typed adapter wire | `crates/intention-provider-generic-chat/tests/generic_chat_contracts.rs` plus crate unit tests in `crates/intention-provider-generic-chat/src/lib.rs`: `reasoning_content` delta normalization to `Primary` `ModelEventDto::ReasoningDelta`, assistant tool-call echo serialization, and empty-channel presence-only handling, with no custom HTTP/SSE parser |
| Runtime same-run attachment | `crates/intention-runtime/tests/m5_tool_loop.rs`: `tool_round_reasoning_is_attached_to_later_requests_in_round_order`, `empty_reasoning_channel_round_attaches_presence_without_blank_facts`; crate unit test `round_reasoning_attachment_keeps_presence_without_text` |
| Capability declaration and fail-closed preflight | generic adapter capability tests; multimodal and vendor extensions still fail before outbound work |
| OpenRouter no-op | `crates/intention-provider-openrouter/src/lib.rs` unit tests: the attachment is ignored and the request shape is unchanged |
| Live 400 evidence and acceptance | the opt-in live run through `crates/intention-daemon/tests/real_api_e2e.rs` under ADR 0040; the controller records date, commit, provider kind, model, and the local run report or the workflow run URL |
| Reconciliation evidence | EVD-064 (`reconciliation/evidence-register.md`, `Planned` until the hermetic gates and the controller-recorded live run); EVD-062 is supplemented because the ADR 0039 continuation now also carries reasoning |

Required gates remain `make quick`, `make verify`, `docs-check`, and the
Linux/Windows CI matrix; the ADR 0040 live channel remains manual and
non-blocking.

## Research provenance

The reasoning directions in [`m4plus_concept.md`](../m4plus_concept.md) were
filtered through [ADR 0014](0014-provider-evolution-profiles-and-reasoning.md),
[ADR 0028](0028-provider-reasoning-and-catalog-detail-directions.md), and the
M5+ Slice 2 activation of [ADR 0037](0037-m5plus-slice2-control-plane.md),
whose normalized reasoning DTO surface this record only reaches through the
ordinary same-run request path. The ADR 0040 live channel supplied the
delivery-integrity provenance: the thinking-mode gateway rejection and the
empty-string acceptance were observed with the exact production request. This
record activates only the same-run echo the ordinary loop requires; it adds no
new research direction, no descriptor/profile revision, and no Slice 3
contract.
