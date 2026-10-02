# 0028: Post-M5 Provider Reasoning and Catalog Detail Directions

## Status

Accepted 2026-08-30. Not implemented; activation requires an activating specification.

## Decision

The provider reasoning and catalog detail from `m4plus_concept.md` is adopted and owned by architecture 22 (provider
evolution, profiles and reasoning):

- typed cross-turn reasoning history (`ReasoningHistoryTransferDto`,
`TextualHistoryV1 { compatibility_id }`, `ReasoningHistoryManifestDto`, `ReasoningHistoryBound`, 4-MiB aggregate bound,
`reasoning_history_unavailable`/`incompatible`/`too_large`);
- reported-usage accounting with no double-count and no price or currency (the
typed `ReasoningUsageDto` was removed by the unconsumed-surface audit, 2026-09);
- `normalized_reasoning_stream_v1` paged initial delivery
(`RunReasoningHistoryPageDto`/`RunReasoningHistoryCompletedDto`, 256 facts / 512 KiB per page,
`normalized_reasoning_stream_required`);
- the typed stateless reasoning dialect catalog
(`reasoning_content`/`reasoning`/`reasoning_details[].text`/ `message.thinking`, thinking activation,
`reasoning_effort`/ `thinking_budget`/`thinking_token_budget`);
- reasoning in branches (`inherited_reasoning_history_references`);
- catalog lifecycle limits (256-character IDs counted as characters, 128
profiles, 32 kinds, 512 KiB raw candidate, 32 issues, 30-minute removal, 8 promotions, 32 reconciliation), tombstones,
and the closed audit taxonomy;
- the normalized-reasoning-stream detail: the closed provider/model/domain/
durable correspondence (`ModelEventDto::ReasoningDelta { category, content }`, `ModelEventDto::ReasoningSummaryDelta {
content }`, `ModelRunFactInputDto::ReasoningDeltaRecorded { category, content }`,
`ModelRunFactInputDto::ReasoningSummaryDeltaRecorded { content }`, and the domain
`ReasoningDeltaRecorded`/`ReasoningSummaryDeltaRecorded` variants), the closed `provider_reasoning_stream_invalid`
failure, the fixed 4-MiB combined per-run reasoning-output bound with `reasoning_output_limit_exceeded`, `category`
required on the wire with no defaulting to `Primary` per [ADR
0038](0038-no-backward-compatibility-and-legacy-removal.md), and the descriptor owning the fixed field-path and
array-index order for values originating in one native response;
- the unified model-capability taxonomy value `model-capability-taxonomy-v1`,
and the note that the concept2 `reasoning_history_transfer` field name is research-only (architecture 22 owns
`LocalDurableHistoryV1 { reasoning_input_contract }`);
- the initial capability-slice detail: the closed supported set of
`reasoning_effort` and the resolved-reasoning-policy contents (closed fragment-category and summary support, the
`ReasoningHistoryTransferDto` mode, `compatibility_id` when transfer is enabled, and the fixed 4-MiB output and history
limits);
- the bounded `responses` v1 detail: the closed `ReasoningEffortLevel` values
(`none`, `minimal`, `low`, `medium`, `high`, `xhigh`, `max`) and the automatic-provider-reasoning-summary default
request for a summary-supporting `responses` profile. The typed Responses reasoning-mode projection and the
protocol-side effort copy were removed as unconsumed by the unconsumed-surface audit (2026-09).

The session-selection layer (session defaults and overrides, the profiles protocol surface) is owned by [architecture
29](../architecture/29-provider-session-and-profiles-protocol.md) and adopted by [ADR
0024](0024-provider-session-and-profiles-protocol-directions.md); this decision removes the "session defaults/overrides"
exclusion from architecture 22's non-goals.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after its own activating specification, and
remains bound to Milestone 5+.

## Normative invariants

1. `compatibility_id` is code-owned and never inferred from a model name,
endpoint, or equal text.
2. A run is never silently sent without required history; the complete
required history must transfer as a whole (4 MiB) or the dependent run is rejected before provider work.
3. Reported usage is never double-counted: the same source `RunId` is never
charged or counted twice.
4. `normalized_reasoning_stream_v1` is additive and negotiated; a client
never receives live reasoning before the initial history completes; unnegotiated peers fail closed with
`normalized_reasoning_stream_required`.
5. The dialect catalog is closed and typed; no encrypted or opaque payloads,
raw provider JSON, or generic request templates.
6. Catalog, profile, and kind rows are immutable append-only history;
tombstones are append-only removal history, and a removed ID is admitted again only when a later accepted catalog
reintroduces it.
7. The legacy M4 selection bridge is removed by
[ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md): no legacy selection binding is materialized for
historical runs, and no synthetic provider selection or binding is ever created.
8. The normalized reasoning stream has one shared `RunEventCursorDto`; a
malformed, duplicate-where-forbidden, out-of-order, or post-terminal value fails closed with
`provider_reasoning_stream_invalid`, and the combined reasoning fragments and summaries of one run are bounded at 4 MiB
with `reasoning_output_limit_exceeded`.
9. `ReasoningEffortLevel` is closed; a profile may select only values declared
in its model subset, and an unsupported effort fails preflight before outbound work.
10. For a summary-supporting `responses` profile, the default request asks for
an automatic provider reasoning summary; a returned summary becomes a distinct tail-only `ReasoningSummaryDelta`, never
raw chain-of-thought and never model context.

## Failure semantics

- Missing, corrupt, incompatible, or over-limit reasoning references block
only the dependent run before any provider call.
- An over-budget page or catalog candidate is rejected before parsing or
persistence; no content is truncated or partly committed.
- A degraded daemon rejects provider state changes, admission, promotion, and
default changes with `execution_not_ready`, except accept/reject of the one pending candidate.

## Rationale

The reasoning history, usage, paged delivery, dialect catalog, and catalog limits were present in `m4plus_concept.md`
but absent from architecture 22, and the session-selection layer was explicitly excluded. Adopting the detail makes the
authoritative documentation cover the features without documenting any part of them as implemented.

## Compatibility and non-goals

This decision supersedes the "session defaults/overrides" exclusion in architecture 22's non-goals and the absence of
the reasoning and catalog detail. The closed M4 baseline, M3/M4 bytes, and existing behavior remain unchanged, and no
code changes are authorized by this decision.

A Responses SDK or driver, user-kind parser, catalog database, wire tags, migrations, profile picker or editor,
credential entry, keychain, or rotation, health test, discovery, pricing, telemetry, live reload, multimodal or
structured output, arbitrary headers, plugin drivers, remote continuation, provider-side parser administration, and
production activation remain outside this decision. M5-M9 are not renumbered.

Owner: architecture 22. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).
