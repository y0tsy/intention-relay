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
name, ancestry, or live resource state. The run-execution-meaning V3 record is
removed; V4 is the single live record version
([ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md)).

## Canonical envelope and outcomes

The accepted envelope is credential-free and binds closed execution kind,
payload tag/version, canonicalization version, canonical bytes and lowercase
SHA-256 digest. It is selected atomically before dependent external work.
Unknown/malformed/mismatched meaning blocks that work before effect while
preserving unrelated readable replay/audit history. Meaning is never rebuilt
from current state.

## Common historical-version policy

The following cross-direction historical-version rules are adopted as future
detail owned by [architecture 14](../architecture/14-run-execution-meaning-and-historical-compatibility.md):

- `intention-domain` owns versions for domain facts, semantic snapshots, and
  canonical record tags; `intention-storage` owns the single current-schema
  DDL and its projections (migrations are removed by ADR 0038);
  `intention-protocol` owns public command, query, and frame schemas;
  an owning provider driver owns its driver-contract compatibility; a future
  direction owns the values of its own records but cannot change a
  cross-direction semantic record without an explicit new version;
- Factory-aligned Skill records, frontmatter, cards, supplements, origins,
  resolution, and selections use new tags and retained Skill decoders; no M3/M4
  record or legacy run acquires a synthetic Skill state; and
- materialized context projections follow the selected `fork-model-context-v1`
  rule: a later implementation uses the stored compatible schema unchanged,
  defines a separately versioned compatible projection, or blocks the dependent
  operation (owned by [architecture 23](../architecture/23-non-destructive-session-branching-and-regeneration.md)).

## Ownership and deferrals

[Architecture 14](../architecture/14-run-execution-meaning-and-historical-compatibility.md)
owns canonical field tables, tags, digest/decoder rules and compatibility
outcomes. Later packages own nested provider, tool, MCP, Skill, Goal, child,
verifier, bridge and UI payload semantics. Provider profiles/Responses and reasoning are owned by architecture 22 and
decision 0014. Legacy bridge definition and decoder removal are governed by
[ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md): the
run-execution-meaning V3 codec and golden are removed and no legacy bridge is
defined. No old
byte, UUID, digest, cursor, event or snapshot is rewritten by this decision.

## Evidence

Future delivery requires kind/version mismatch rejection, canonical golden
vectors for the current record version, current-record decode/replay fixtures,
no-current-state-reconstruction tests, and no-external-work-on-incompatibility
proof.

## Provenance

`m4plus_concept.md`, Mandate execution kind, execution meaning, and historical
version compatibility sections.
