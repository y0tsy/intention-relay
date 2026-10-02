# ADR 0046: Typed serde JSON contracts

## Status

Accepted 2026-09-30. It removes the binary canonical codec, the numeric tag registry, digests and identity, and the
unconsumed contract families from the workspace, and it fixes the canonicalization policy for the future. It adds no
crate and no dependency: RFC 8785 canonicalization is adopted as policy, and its crate is added only together with a
first real consumer.

## Scope and supersession

In scope: the `intention-domain` canonical codec and execution-meaning records, the `intention-protocol` contract
families and tag descriptors, the SHA-256 dependency, the canonical goldens and their in-file tests, and the
documentation statements that describe them.

Out of scope: the typed serde DTOs themselves, the storage schema, the configuration format, the local transport framing
(ADR 0045), and every behavior of the ordinary runtime. The deletion removed the codec and tag-descriptor DTO families
defined inside `contract_families.rs`; the retained domain, protocol, storage, and configuration DTOs are unaffected.

| Record | Superseded clause | Replaced by |
| --- | --- | --- |
| [ADR 0035](0035-m5plus-complete-foundation-activation.md) | The Slice 1 items `run-execution-meaning-v4` and "canonical tags and digests under the existing `typed-tlv-v1`/SHA-256 policy" | Slice 1 is the typed JSON protocol/DTO contract surface; no canonical codec and no digest identity |
| [ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md) | The "Canonical records" version-ledger row and the Wave 2 plan that repins identity goldens and keeps the V4 record codec | The whole codec, not only the V3 record, is removed |

The historical-compatibility rule that no historical record gains synthetic meaning and that missing meaning is never
reconstructed from current state is not superseded by this record; it remains a project rule owned by [architecture
14](../architecture/14-run-execution-meaning-and-historical-compatibility.md) and [ADR
0038](0038-no-backward-compatibility-and-legacy-removal.md).

## Decision

### Typed JSON only

1. Wire and domain contracts are typed serde JSON. The workspace keeps exactly
one representation: the typed DTOs that the domain, protocol, storage, and configuration already use.
2. Delete the binary canonical codec and its consumers:
`crates/intention-domain/src/canonical.rs`, `crates/intention-domain/src/run_execution_meaning.rs`, and
`crates/intention-protocol/src/contract_families.rs`, together with their `pub mod` and `pub use` exports, all tests
inside them, and the ten canonical goldens under `crates/intention-domain/tests/fixtures/goldens/`.
3. Delete the numeric tag registry (`TagRegistry`, `TagStatus`, the ledger
table, and the tag constants), the `IRCR` and `typed-tlv` codec, canonical digest text and identity codecs,
digest-mismatch errors, `ContractFamilyDescriptor`, `PUBLIC_WIRE_CONTRACT_FAMILIES`, and the wire-family parity tests.
4. Remove `sha2` and its transitive crates from the workspace: the
`intention-domain` dependency, the `quality/architecture.toml` allowlist row, the lockfile, and
`THIRD_PARTY_NOTICES.md`, regenerated with `make notices`.
5. The execution-meaning V4 record and its envelope are removed with the
codec. No canonical meaning bytes, meaning digest, or meaning version is produced, stored, or validated. If a future
consumer needs a serializable meaning record, that record is a typed JSON DTO under the single live version; no
replacement codec is introduced speculatively.
6. Verification receipt: a repository search across `crates/` and `quality/`
finds no `IRCR`, `typed-tlv`, `TagRegistry`, `DigestMismatch`, `execution_meaning`, or `credentials_forbidden` outside
historical ADR text.

### Canonicalization policy

7. When a real consumer needs byte-stable JSON, for example a signed or
hashed artifact, canonicalization is RFC 8785 (JSON Canonicalization Scheme), implemented by `serde_json_canonicalizer`.
The crate is not added now: with no consumer it would be a dead dependency that fails the unused-dependency checks.
8. Until such a consumer exists, no component hashes JSON for identity,
addressing, or comparison; equality is typed structural equality, and record identity is a typed revision identity
rather than a digest.

### Consequences

9. The removed files carried the 256 and 512 character caps, the quantity and
aggregate limits, the digest-format checks, and the frozen activity, run, and child limit records. Those validators are
deleted rather than relocated; the surviving limits are only the liveness safeguards recorded by [ADR
0048](0048-limits-by-precedent-and-no-content-scanning.md).
10. Coverage denominators shrink by more than eight thousand lines; coverage
tiers are re-verified by `make verify` rather than adjusted by exclusion.
11. The Slice 1 "contract ledger" as a canonical-codec and registry artifact
no longer exists; the typed protocol, DTO, configuration, and storage schema versions remain the only ledger surface.

## Invariants

1. One representation. Typed serde JSON is the only wire and contract
representation; there is no binary canonical codec, no numeric field tag, and no typed-TLV.
2. No identity digests. The workspace produces no SHA-256 identity, namespace
digest, or digest text, and nothing addresses a record by digest.
3. No speculative canonicalizer. RFC 8785 is the policy; the dependency
appears only together with a real consumer.
4. Typed validation only. Contract validation is typed deserialization plus
the semantic checks that remain in the typed DTOs.
5. History untouched. M3/M4 bytes, runs, events, snapshots, and cursors keep
their recorded meaning and are never re-encoded, rewritten, or synthesized.
6. Single version. The single-version policy of
[ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md) is unchanged; this record removes a representation, not
a version.

## Compatibility

M3/M4 recorded state and replay are unaffected: the removed surfaces had no consumer outside their own codecs, tests,
and goldens. No compatibility fixture, golden, or decoder is kept for the removed representation, per [ADR
0038](0038-no-backward-compatibility-and-legacy-removal.md). Typed DTOs evolve in place under the current public schema
version; no second DTO schema version and no migration are introduced.

## Security and failure behavior

Removing the digest and identity machinery removes the digest-mismatch failure family and the tag-parity failure family;
dependent work no longer fails on a canonical-encoding mismatch because no canonical encoding exists. Typed DTO
validation remains the only contract rejection surface and keeps its credential-free typed errors. The runtime
credential-shaped content validators that lived in the deleted files are removed with them, and the content-scanning ban
is recorded by [ADR 0048](0048-limits-by-precedent-and-no-content-scanning.md); the surviving secret hygiene is the CI
documentation secret scan and the fake-secret absence tests.

## Non-goals

No replacement binary codec; no canonicalization dependency now; no new identity or digest scheme; no migration or
conversion of persisted data; no change to typed DTO shapes or schema versions; no change to the local transport framing
of [ADR 0045](0045-local-json-rpc-2-0-transport.md); no re-introduction of the deleted validation limits.

## Affected documents

[Architecture 02](../architecture/02-dto-and-contract-policy.md) owns the typed DTO and contract policy that replaces
the canonical codec; [architecture 14](../architecture/14-run-execution-meaning-and-historical-compatibility.md) keeps
the historical-compatibility rules that do not depend on the codec; [architecture
10](../architecture/10-test-driven-delivery-and-verification.md) and [architecture
12](../architecture/12-quality-gates-and-makefile.md) own the re-verified coverage tiers and the removal of the golden
and parity tests from the gate surface; [architecture 11](../architecture/11-implementation-roadmap.md) and the
[architecture README](../architecture/README.md) record the removed Slice 1 ledger artifacts; and
`quality/architecture.toml`, `Cargo.lock`, and `THIRD_PARTY_NOTICES.md` are updated in the same change with the
dependency removal.

## Evidence

The removal is accepted only together with: the deleted files, goldens, and in-file tests absent from the tree, with no
remaining reference to a removed symbol; the repository search receipt for `IRCR`, `typed-tlv`, `TagRegistry`,
`DigestMismatch`, `execution_meaning`, and `credentials_forbidden`; `make notices` regenerating the notices and lockfile
without `sha2` and its transitive crates; re-verified coverage tiers; and unchanged M3/M4 current-schema round trips
with an unchanged live `make e2e-real-api` path. Gates: `make quick`, `make verify`, `docs-check`, Linux/Windows CI.

## Research provenance

The canonical codec's original purpose of typed-TLV record identity across M3/M4; the audit finding that no consumer
outside the codec, its tests, and its goldens existed; the single-version and no-bureaucracy policy of [ADR
0038](0038-no-backward-compatibility-and-legacy-removal.md); and the deferral of RFC 8785 until a real consumer appears.
