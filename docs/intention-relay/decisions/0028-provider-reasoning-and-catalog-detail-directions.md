# ADR 0028: Post-M5 Provider Reasoning and Catalog Detail Directions

## Status

Accepted 2026-08-30. This decision records the provider reasoning and catalog
detail layer as an accepted future direction. It does not activate
implementation: the layer is bound to
[Milestone 5+](../architecture/11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment)
and is implemented only through a separately accepted activating specification
at the start of that milestone.

## Decision

The following detail from
[`m4plus_concept2.md`](../m4plus_concept2.md) is adopted and owned by
[architecture 22](../architecture/22-provider-evolution-profiles-and-reasoning.md):

- typed cross-turn reasoning history (`ReasoningHistoryTransferDto`,
  `TextualHistoryV1 { compatibility_id }`, `ReasoningHistoryManifestDto`,
  `ReasoningHistoryBound`, 4-MiB aggregate bound,
  `reasoning_history_unavailable`/`incompatible`/`too_large`);
- reasoning usage accounting (`ReasoningUsageDto`, absent-never-zero, no
  double-count, no price/currency);
- `normalized_reasoning_stream_v1` paged initial delivery
  (`RunReasoningHistoryPageDto`/`RunReasoningHistoryCompletedDto`, 256 facts /
  512 KiB per page, `normalized_reasoning_stream_required`);
- the typed stateless reasoning dialect catalog
  (`reasoning_content`/`reasoning`/`reasoning_details[].text`/
  `message.thinking`, thinking activation, `reasoning_effort`/
  `thinking_budget`/`thinking_token_budget`);
- reasoning in branches (`inherited_reasoning_history_references`);
- catalog lifecycle limits (63-char IDs, 128 profiles, 32 kinds, 512 KiB raw
  candidate, 32 issues, 30-minute removal, 8 promotions, 32 reconciliation),
  tombstones, and the closed audit taxonomy; and
- the legacy M4 selection bridge (`LegacyM4SelectionBindingDto`,
  `historical_selection_corrupt`).

The session-selection layer (session defaults/overrides, `provider_profiles_v1`,
promotion/reconciliation, held recovered-run admission) is owned by
[architecture 29](../architecture/29-provider-session-and-profiles-protocol.md) and adopted by
[ADR 0024](../decisions/0024-provider-session-and-profiles-protocol-directions.md);
this decision removes the "session defaults/overrides" exclusion from
architecture 22's non-goals.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after
a later activating specification, and is bound to Milestone 5+ in the roadmap.

## Rationale

The authoritative package review of 2026-08-30 confirmed the reasoning history,
usage, paged delivery, dialect catalog, catalog limits, and legacy bridge are
present in `m4plus_concept2.md` but absent from architecture 22, and that the
session-selection layer was explicitly excluded. This decision adopts the detail
so the authoritative documentation fully covers the features, while preserving
the project rule that no feature is documented as implemented without code
evidence.

## Normative invariants

1. `compatibility_id` is code-owned and never inferred from a model name,
   endpoint, or equal text.
2. A run is never silently sent without required history; the complete required
   history must transfer as a whole (4 MiB) or the dependent run is rejected
   before provider work.
3. A missing reasoning usage component is never zero; the same source `RunId`
   is never charged or counted twice.
4. `normalized_reasoning_stream_v1` is additive and negotiated; a client never
   receives live reasoning before the initial history completes; unnegotiated
   peers fail closed with `normalized_reasoning_stream_required`.
5. The dialect catalog is closed and typed; no encrypted/opaque payloads, raw
   provider JSON, or generic request templates.
6. Catalog/profile/kind rows are immutable append-only history; tombstones are
   permanent; a tombstoned ID cannot be reintroduced.
7. The legacy M4 bridge is additive: original IDs, snapshot JSON, and UUIDs are
   never replaced; a missing/corrupt binding is `historical_selection_corrupt`.

## Failure semantics

- Missing/corrupt/incompatible/over-limit reasoning references block only the
  dependent run before any provider call.
- An over-budget page or catalog candidate is rejected before parsing or
  persistence; no content is truncated or partly committed.
- A degraded daemon rejects provider state changes/admission/promotion/default
  changes with `execution_not_ready`, except accept/reject of the one pending
  candidate.

## Compatibility and supersession

This decision supersedes the "session defaults/overrides" exclusion in
architecture 22's non-goals and the absence of the reasoning/catalog detail.
The closed M4 baseline, M3/M4 bytes, and existing behavior remain unchanged.
Activation remains deferred: no code changes are authorized by this decision.

## Security and residual risk

The layer remains trusted-local. Manifests, pages, tombstones, and failures are
bounded and credential-free; redaction stays central and every activating
specification must pass the fake-secret regression suite.

## Affected documents

- [`architecture/11-implementation-roadmap.md`](../architecture/11-implementation-roadmap.md)
- [`architecture/22-provider-evolution-profiles-and-reasoning.md`](../architecture/22-provider-evolution-profiles-and-reasoning.md)
- [`architecture/29-provider-session-and-profiles-protocol.md`](../architecture/29-provider-session-and-profiles-protocol.md)
- [`reconciliation/README.md`](../reconciliation/README.md)
- [`reconciliation/source-of-truth-matrix.md`](../reconciliation/source-of-truth-matrix.md)
- [`reconciliation/contradiction-register.md`](../reconciliation/contradiction-register.md)
- [`reconciliation/concept-supersession-index.md`](../reconciliation/concept-supersession-index.md)
- [`reconciliation/deferred-excluded-register.md`](../reconciliation/deferred-excluded-register.md)
- [`reconciliation/evidence-register.md`](../reconciliation/evidence-register.md)
- [`reconciliation/compatibility-register.md`](../reconciliation/compatibility-register.md)
- [`decisions/README.md`](README.md)

## Required evidence

No implementation evidence is claimed. The activating specification must declare
exact crates, DTO/wire/storage versions, feature profiles, coverage tiers,
fixtures, and outcome evidence, and pass `make quick`, `make verify`, and
Linux/Windows CI before acceptance.

## Non-goals

This decision does not implement the layer; it does not change M3/M4 behavior;
it does not renumber M5--M9; it does not activate a crate, schema, migration,
protocol, or feature. A Responses SDK/driver, user-kind parser, catalog
database, wire tags, migrations, profile picker/editor, credential
entry/keychain/rotation, health test, discovery, pricing, telemetry, live
reload, multimodal or structured output, arbitrary headers, plugin drivers,
remote continuation, provider-side parser administration, and production
activation remain outside this decision.
