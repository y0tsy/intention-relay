# ADR 0032: Post-M5 Accepted Deferred Directions: Activity-Tree Metadata, Semantic Content Inspection, and Per-Call Cancellation

## Status

Accepted 2026-08-30. This decision records three deliberately deferred concept
items as accepted future directions. It does not activate implementation: the
directions are bound to
[Milestone 5+](../architecture/11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment)
and are implemented only through a separately accepted activating specification
at the start of that milestone.

## Decision

The following three items, named in the
[`m4plus_concept2.md`](../m4plus_concept2.md) backlog as "still to decide" or
"deliberately deferred", are adopted as accepted future directions:

- **Tree-level metadata** (concept2 line 7608), owned by
  [architecture 24](../architecture/24-activity-ui-and-adapters.md): a future
  bounded, credential-free metadata surface on activity trees, distinct from
  journal records and notification state, that never becomes activity,
  authority, or a second sequence;
- **Semantic content inspection** (concept2 line 7303), owned by
  [architecture 22](../architecture/22-provider-evolution-profiles-and-reasoning.md):
  a future explicit, code-owned content-inspection policy for reasoning and
  provider content that never substitutes for central redaction, never
  rewrites stored facts, and remains non-authorizing; and
- **Per-call cancellation and owner-specific semantics beyond the selected
  direct-pair communication** (concept2 line 7304), owned by
  [architecture 19](../architecture/19-mandate-gateway-rlm-bridge.md) and
  [architecture 24](../architecture/24-activity-ui-and-adapters.md): a future
  owner-specific cancellation contract beyond the first-scope
  `StopRunCommandDto`/direct-pair boundary.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after
a later activating specification, and is bound to Milestone 5+ in the roadmap.

## Rationale

The authoritative package review of 2026-08-30 confirmed these three items are
named in `m4plus_concept2.md` as deferred or still-to-decide but appear nowhere
in the authoritative documentation, including the deferred/excluded register.
This decision adopts them as accepted future directions so the authoritative
documentation no longer leaves them unmapped, while preserving the project rule
that no feature is documented as implemented without code evidence.

## Normative invariants

1. Tree-level metadata is bounded, credential-free, and non-authorizing; it
   cannot create activity, notification, lifecycle, scheduler, tool, child,
   verifier, MCP, bridge, kernel, or reconciliation authority, and it does not
   replace or filter the activity journal or notification sequences.
2. Semantic content inspection is explicit, code-owned, and non-authorizing; it
   never substitutes for central redaction, never rewrites stored facts, and
   never exposes raw provider content.
3. Per-call cancellation and owner-specific semantics are additive future
   contracts; the first-scope `StopRunCommandDto`/direct-pair boundary remains
   authoritative until a later activating specification defines them.

## Failure semantics

- Each direction fails closed before effect when its future contract is
  unsupported, unnegotiated, or over-limit; no partial projection or partial
  cancellation is delivered.
- Recovery never resumes, retries, reattaches, or reruns work under any of the
  three directions.

## Compatibility and supersession

This decision supersedes the absence of the three items in the authoritative
documentation and records them as accepted future directions rather than
unregistered deferrals. The closed M4 baseline, M3/M4 bytes, and existing
behavior remain unchanged. Activation remains deferred: no code changes are
authorized by this decision.

## Security and residual risk

The directions remain trusted-local and non-authorizing. Tree-level metadata,
content inspection, and per-call cancellation never expose credentials,
provider payloads, prompts, paths, grants, or raw transcripts; redaction stays
central and every activating specification must pass the fake-secret regression
suite.

## Affected documents

- [`architecture/11-implementation-roadmap.md`](../architecture/11-implementation-roadmap.md)
- [`architecture/19-mandate-gateway-rlm-bridge.md`](../architecture/19-mandate-gateway-rlm-bridge.md)
- [`architecture/22-provider-evolution-profiles-and-reasoning.md`](../architecture/22-provider-evolution-profiles-and-reasoning.md)
- [`architecture/24-activity-ui-and-adapters.md`](../architecture/24-activity-ui-and-adapters.md)
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

This decision does not implement the directions; it does not change M3/M4
behavior; it does not renumber M5--M9; it does not activate a crate, schema,
migration, protocol, or feature. Native OS notifications, remote push,
provider-side parser administration, remote continuation, and production
activation remain outside this decision.
