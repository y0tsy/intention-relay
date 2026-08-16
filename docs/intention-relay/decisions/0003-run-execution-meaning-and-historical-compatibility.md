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

## Canonical envelope and outcomes

The accepted envelope is credential-free and binds closed execution kind,
payload tag/version, canonicalization version, canonical bytes and lowercase
SHA-256 digest. It is selected atomically before dependent external work.
Unknown/malformed/mismatched meaning blocks that work before effect while
preserving unrelated readable replay/audit history. Meaning is never rebuilt
from current state.

## Ownership and deferrals

[Architecture 14](../architecture/14-run-execution-meaning-and-historical-compatibility.md)
owns canonical field tables, tags, digest/decoder rules and compatibility
outcomes. Later packages own nested provider, tool, MCP, Skill, Goal, child,
verifier, bridge and UI payload semantics. Provider profiles/Responses,
reasoning, bridge implementation and decoder removal remain deferred. No old
byte, UUID, digest, cursor, event or snapshot is rewritten by this decision.

## Evidence

Future delivery requires kind/version mismatch rejection, canonical golden
vectors, legacy decode/replay fixtures, no-current-state-reconstruction tests,
and no-external-work-on-incompatibility proof.

## Provenance

`m4plus_concept2.md`, Mandate execution kind, execution meaning, and historical
version compatibility sections.
