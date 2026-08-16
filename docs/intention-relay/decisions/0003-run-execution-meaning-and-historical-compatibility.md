# 0003: Run Execution Meaning and Historical Compatibility

## Status

Accepted.

## Decision

Future execution uses a closed execution-kind envelope: `Ordinary`, `Mandate`,
or `VerifierMandate`, with an explicit meaning version, payload and canonical
digest. Admission stores immutable, credential-free meaning before dependent
external work. A kind/version/payload mismatch blocks that work.

Historical M3/M4 and existing ordinary meanings remain readable only under their
recorded semantics. They never gain synthetic Mandate, verifier, Skill, MCP,
child, activity, policy, profile, or execution-kind state. Missing historical
meaning is never reconstructed from current configuration, registry, model
name, ancestry, or live resource state.

## Compatibility and non-goals

Future canonical field tables, tags, provider profiles, reasoning, bridge
records, and decoder retention schedules are deferred to their owning packages.
No old byte, UUID, digest, cursor, event, or snapshot is rewritten by this
decision.

## Evidence

Future delivery requires kind/version mismatch rejection, canonical golden
vectors, legacy decode/replay fixtures, no-current-state-reconstruction tests,
and no-external-work-on-incompatibility proof.

## Provenance

`m4plus_concept2.md`, Mandate execution kind, execution meaning, and historical
version compatibility sections.
