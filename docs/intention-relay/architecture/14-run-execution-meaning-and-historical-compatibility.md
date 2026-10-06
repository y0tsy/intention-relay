# Run Execution Meaning and Historical Compatibility

**Superseded historical record. No implementation work is authorized here; the removed machinery has no implementation
path.**

Owner: architecture 14 (historical record). Decisions: ADR 0005, ADR 0012. Research: m4plus_concept.md.

This document was the detailed owner for immutable execution meaning, canonical semantic identity, decoding, and
execution/replay/audit compatibility. That machinery is removed: the binary canonical codec, the semantic decoders, the
digest/identity helpers, and the compatibility classes are deleted by ADR 0012, and historical compatibility is banned
by ADR
0005. The document is retained as the historical record of what existed, what was removed, and what remains.

## What existed

Until the removal, future execution work was specified around one closed execution-meaning envelope and one canonical
record codec. `RunExecutionMeaningEnvelopeV1` bound a closed execution kind (`Ordinary`, `Mandate`, `VerifierMandate`),
a meaning record tag, a record version, a canonicalization version, the canonical meaning bytes, and a SHA-256 canonical
meaning digest. The digest's lower-case textual form was `<namespace>:sha256:<64 lowercase hex>`, and the digest field
was excluded from its own digest input; the tuple `(envelope version, execution kind, record tag, record version)` was
closed. Unknown kind/version, wrong tag, kind/payload mismatch, malformed or noncanonical bytes, digest mismatch, an
unavailable nested selection, or unsupported executable semantics blocked dependent work before any provider, tool,
process, kernel, MCP, network, child, bridge, or scheduler effect.

`CanonicalRecordV1` specified `IRCR` framing with canonicalization version `typed-tlv-v1`: four ASCII `IRCR` bytes,
big-endian `u32` canonicalization version, record tag, record version, then a strictly increasing field stream of field
number, one-byte wire type, `u32` value length, and exact value bytes. The initial wire types were `u64`, `bool`,
`utf8`, `uuid`, `digest`, `bytes`, `record`, `list`, and `optional`; optional values had an explicit presence marker,
lists a counted ordered element sequence, and nested records carried their full framing rather than an untyped blob.
Field tables identified required and optional fields, types, order, owner, intrinsic encoded-size and nesting bounds,
and digest namespace; tags were domain-owned, globally stable, and never reused.

`MandateRunExecutionMeaningV1` was a credential-free canonical record with a fourteen-field table: Mandate selection,
provider selection, model capability selection, activity selection, context projection selection, direct tool selection,
Goal context selection, MCP selection, verifier selection presence and value, child-link selection presence and value,
terminal provenance references, and Skill selection. `MandateSelectionV1` froze Mandate identity/revision, trigger
reason, service-session/activity context, verified checkpoints, and continuation configuration. Every optional nested
selection was exactly `Disabled` or `Selected`, ordered collections were digest-significant, and set-like fields defined
a canonical sort key and rejected duplicate semantic keys.

Decoding produced one closed compatibility outcome: `Supported`, `ReadableNotExecutable`, `UnknownTag`,
`UnknownVersion`, `Corrupt`, `DigestMismatch`, or `KindPayloadMismatch`. Execution required a valid canonical record,
executable nested versions, a supported exact driver contract, required references, and present capacity/readiness;
availability could defer work but never mutated or rebuilt meaning, readable history could replay while execution was
unavailable, and an unavailable audit record was isolated without synthesizing a replacement fact. Every claimed
executable decoder retained exact golden fixtures. The earlier V3 record codec and its golden had already been removed
under ADR 0005, leaving V4 as the single live record version with no legacy bridge.

## What was removed

The removal (ADR 0012) deleted the binary canonical codec and the unconsumed contract families in one change:

- `crates/intention-domain/src/canonical.rs`: the codec, tag registry, and canonical record framing;
-  `crates/intention-domain/src/run_execution_meaning.rs`: the execution-kind envelope, `MandateRunExecutionMeaningV1`,
its field tables and validation, and the digest/identity helpers;
- `crates/intention-protocol/src/contract_families.rs`: the contract families and their public descriptors;
-  the `sha2` dependency and its transitive crates, the architecture policy allowlist entry, and the regenerated
lockfile and third-party notices;
- the semantic decoders and compatibility classes;
- the golden fixtures under `crates/intention-domain/tests/fixtures/goldens/`; and
-  the 256/512-character checks, the frozen activity/run/child limit records (`Fixed*Limits`), and other speculative
contract limits that lived in those files.

No replacement codec was introduced: domain and wire records are typed serde JSON (ADR 0012), and the
`IRCR`/`typed-tlv-v1` framing, a tag registry, canonical digests, and canonical semantic identity no longer exist
anywhere in the project. RFC 8785 canonicalization is adopted only together with a first real consumer, and no
canonicalization crate is added before then. Historical compatibility remains banned (ADR 0005): no legacy bridge,
old-version decoder, migration or byte-preservation fixture exists, and no historical record gains synthetic future
state, and missing meaning is never reconstructed from current state.

## What remains

- The local protocol is JSON-RPC 2.0 over NDJSON (ADR 0011); domain and wire records are typed serde JSON DTOs.
-  M3/M4 sessions, runs, provider kinds, tool-call denial, replay, and recovery keep their ordinary
current semantics under the single live schema and protocol version (ADR 0005). Transport and storage liveness
safeguards are the limits kept by ADR 0014, not semantic limits.
-  Provider selection for future work stays credential-free and non-authorizing: a model ID never selects provider kind,
driver, endpoint, protocol, capability, credential transport, or execution kind, and the frozen capability intersection
remains kind descriptor maximum intersect explicitly declared model subset intersect driver support.
`ProviderDriverContractRevisionDto` remains code-owned family plus `major.minor`; incompatible request, normalization,
order, capability, or credential-transport changes require a new major. [Architecture
22](22-provider-evolution-profiles-and-reasoning.md) owns `responses`, parse-time `openai` aliasing, profiles/catalogs,
and reasoning semantics.
-  Future nested selections are typed JSON fields owned by their domain documents: tool selection by [architecture
15](15-tool-registry-and-model-tool-loop.md), MCP by [architecture
18](18-mcp-capability-lifecycle.md), bridge by [architecture 19](19-gateway-rlm-bridge.md), kernel by
[architecture 20](20-ipython-kernel-lifecycle.md), and Goal/Skill/context by [architecture
21](21-goals-skills-context-memory-and-compaction.md). None of them revives a canonical envelope, digest, or decoder.
-  `WorkspaceRoot` is an addressing anchor: the default base for relative paths, the initial CWD for `execute`, and the
default scope root for glob/grep. It is not a security boundary, and no lexical symlink parser or containment check
gates it (ADR 0013).
-  [Architecture 30](30-instruction-sources-and-system-context.md) ([ADR
0010](../decisions/0010-instruction-sources-and-system-context.md)) keeps the effective instruction projection as
configuration and project content; no instruction digest exists, and instruction text is never execution meaning.

## Historical ownership

The removed machinery was owned as follows; the live owners now implement typed JSON equivalents or own nothing at all.

| Historical owner | Removed responsibility |
| --- | --- |
| Domain | Envelope/payload DTOs, tags, canonicalization, digest validation, semantic decoders, and compatibility classes. |
| Storage | Atomic binding/read contracts and canonical byte preservation. |
| Protocol | Separately negotiated safe projections of canonical records. |
| Provider/tool/context owners | Nested selection versions and validation inside the canonical record. |
| Application/runtime | Admission selection and pre-effect canonical compatibility enforcement. |
| Composition | Resolving validated concrete implementations without rebuilding stored meaning. |
| Daemon | IDs, tasks, durable reread, and publication. |
| Adapters | Typed compatibility outcome display. |

## Dependencies and non-goals

This document defines no live encoding, SQL/wire implementation, provider runtime activation, MCP, Skills/Goals,
bridge/kernel, forks, UI, crates, Cargo, feature/coverage policy, Makefile/CI, or M4 behavior. The admitted run owns
its execution evidence; Session sequence and Run cursor remain separate, as owned by [architecture
04](04-sessions-runs-events-and-storage.md).

## Required evidence before implementation

None for the removed machinery; it has no implementation path, and no activation specification can revive it. A future
change to the record policy (for example, adopting RFC 8785 canonicalization with its first real consumer) requires its
own approved decision and the repository's ordinary evidence, and must not reintroduce a canonical identity or a
compatibility layer for a removed version.
