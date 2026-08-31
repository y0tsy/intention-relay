# 0006: Mandate Lifecycle and Admission Boundary

## Status

Accepted.

## Scope

This record promotes the Foundation Mandate authority rules into one detailed
lifecycle owner, without authorizing implementation.

## Decision

`13-mandate-domain-and-durable-lifecycle.md` is the sole detailed owner for
Mandate aggregate identity, revisions, lifecycle, trigger reasons, fresh-run
admission, conflict precedence, uncertainty pausing, recovery, and Mandate
protocol/persistence boundaries.

Mandate triggers are distinct from M3 queue tickets. Admission is one atomic,
idempotent transition that freezes a new RunId, revision, reason, selection, and
execution meaning before an external effect. User lifecycle/revision mutations
win conflicts. Unknown terminal effects pause the Mandate until exact
reconciliation permits only a later fresh Active path or Stopped.

## Compatibility and non-goals

M3/M4 remain ordinary historical behavior. This decision adds no crate, schema,
migration, protocol wire family, direct tool policy, WorkspaceRoot rule, child
or verifier implementation, provider evolution, MCP, Skill, UI, or roadmap
renumbering.

## Evidence

Later work requires exhaustive lifecycle, race, atomicity, phase/recovery,
compatibility, redaction, replay, and outcome fixtures described by the owner
document.

## Provenance

`m4plus_concept.md`, Mandate identity/lifecycle, transition linearization,
trigger/recovery, limit classification, and historical compatibility sections.
