# 0001: Mandate Authority and Fresh-Run Lifecycle

## Status

Accepted.

## Decision

A Mandate is durable user-issued work authority, not a Goal, prompt, tool permission, provider continuation, daemon, or
second runtime. User commands own product lifecycle and revision decisions. The daemon may only record explicitly
defined operational facts, including trigger capture, admission, known terminal disposition, and required uncertainty
pausing.

A continuation always admits a fresh run with a new `RunId`. Revision changes affect only future admission. No provider
request, tool call, process, kernel, child work, MCP operation, or external effect resumes after restart.

The conceptual durable Mandate family (`MandateDto`, `MandateRevisionDto`, `MandateTriggerReasonDto`,
`MandateRunDispositionDto`) is future detail owned by [architecture
13](../architecture/13-mandate-domain-and-durable-lifecycle.md). Its records are credential-free, typed, immutable at
their selected revision, and represented through the typed JSON record policy (ADR 0046). They contain no raw prompt
transcript, provider resource, live kernel namespace, process handle, MCP connection, bridge grant, credential, or
unfinished external operation.

`ProductCeiling` is a product counter, reservation, or quota and is forbidden for new Mandate admission.
`IntrinsicBound` is a correctness boundary that remains mandatory and rejects without truncation. `CapacityAvailability`
is temporary finite runtime, storage, provider, registry, process, kernel, or scheduler availability that never becomes
a quota or a successful result. An `Unavailable` outcome atomically preserves committed history, the applicable pending
trigger, and its projections without dropping, truncating, or inventing work; a later durable readiness/capacity
observation or explicit user lifecycle action may make that trigger eligible for a fresh run only.

## Invariants

- exactly one non-terminal Mandate run exists at a time;
- triggers are durable causal evidence, not legacy queued turns;
- Goals, Skills, parentage, activity, MCP source, model output, bridge grants,
kernel state, and adapter state grant no lifecycle authority;
- intrinsic bounds, typed capacity availability, and forbidden product ceilings
remain separate concepts.

## Compatibility and non-goals

M3/M4 runs retain recorded ordinary semantics. This record does not define scheduler ordering, direct tool admission,
WorkspaceRoot behavior, child graphs, provider profiles, or implementation crates.

Owner: architecture 13. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).

Provenance: `m4plus_concept.md`, selected semi-autonomous Mandate overlay and transition linearization sections.
