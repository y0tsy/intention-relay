# 0001: Mandate Authority and Fresh-Run Lifecycle

## Status

Accepted.

## Decision

A Mandate is durable user-issued work authority, not a Goal, prompt, tool
permission, provider continuation, daemon, or second runtime. User commands own
product lifecycle and revision decisions. The daemon may only record explicitly
defined operational facts, including trigger capture, admission, known terminal
disposition, and required uncertainty pausing.

A continuation always admits a fresh run with a new `RunId`. Revision changes
affect only future admission. No provider request, tool call, process, kernel,
child work, MCP operation, or external effect resumes after restart.

### Mandate DTO family and limit classification

The conceptual durable Mandate family is adopted as future detail owned by
[architecture 13](../architecture/13-mandate-domain-and-durable-lifecycle.md):

```text
MandateDto
  mandate_id
  active_revision
  lifecycle_state
  service_session_id
  work_state_references
  verified_checkpoint_references
  child_work_graph_reference
  activity_identity

MandateRevisionDto
  mandate_id
  revision
  objective
  scope
  mode
  trigger_configuration
  goal_context_references
  continuation_configuration
  stop_conditions
  canonical_revision_digest

MandateTriggerReasonDto
  reason_id
  source_kind
  first_observed_at
  last_observed_at
  coalesced_count
  typed_references
  triggering_revision

MandateRunDispositionDto
  run_id
  terminal_kind
  next_action = Continue | AwaitUserDecision | None
  checkpoint_reference_when_verified
  external_effect_reference_when_unknown

MandateLimitClassDto
  ProductCeiling
  IntrinsicBound
  CapacityAvailability

MandateCapacityOutcomeDto
  outcome = Available | Unavailable
  resource_kind
  reason
  retry_disposition
  trigger_reason_reference
  observed_at
```

All records are credential-free, typed, immutable at their selected revision,
and represented through the repository's later canonical record/version policy.
They contain no raw prompt transcript, provider resource, live kernel
namespace, process handle, MCP connection, bridge grant, credential, or
unfinished external operation. `ProductCeiling` is a product counter,
reservation, or quota and is forbidden for new Mandate admission;
`IntrinsicBound` is a correctness boundary that remains mandatory and rejects
without truncation; `CapacityAvailability` is temporary finite runtime,
storage, provider, registry, process, kernel, or scheduler availability that
never becomes a quota or a successful result. An `Unavailable` outcome
atomically preserves committed history, the applicable pending trigger, and its
projections without dropping, truncating, or inventing work; a later durable
readiness/capacity observation or explicit user lifecycle action may make that
trigger eligible for a fresh run only.

## Invariants

- exactly one non-terminal Mandate run exists at a time;
- triggers are durable causal evidence, not legacy queued turns;
- Goals, Skills, parentage, activity, MCP source, model output, bridge grants,
  kernel state, and adapter state grant no lifecycle authority;
- intrinsic bounds, typed capacity availability, and forbidden product ceilings
  remain separate concepts.

## Compatibility and non-goals

M3/M4 runs retain recorded ordinary semantics. This record does not define
scheduler ordering, direct tool admission, WorkspaceRoot behavior, child
graphs, provider profiles, or implementation crates. The Mandate DTO family
and limit-class DTOs above are future detail owned by architecture 13 and do
not activate a crate, schema, migration, or wire implementation.

## Evidence

Future delivery requires lifecycle/race/fresh-run/recovery/typed-capacity
fixtures, no-synthetic-history fixtures, and outcome evidence through the
standard quality gate.

## Provenance

`m4plus_concept2.md`, selected semi-autonomous Mandate overlay and transition
linearization sections.
