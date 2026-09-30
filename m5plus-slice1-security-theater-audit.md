# M5+ Slice 1: security-theater and unconsumed-surface audit

**Status.** Read-only audit record and working document for the cleanup branch
`cleanup/m5plus-slice1-theater-and-dead-code`.

**Baseline.** `main` @ `4bce50e` (2026-09-30), clean tree.

**Object.** The M5+ Slice 1 delivery, "Contracts and versions"
([ADR 0036](docs/intention-relay/decisions/0036-m5plus-slice1-contract-ledger.md)),
merged as PR #21 (squash `5d81639` plus follow-up `37dc501`), as it exists on
`main` after the Slice 2 revert
([ADR 0044](docs/intention-relay/decisions/0044-revert-of-m5plus-slice2-control-plane.md),
`4bce50e`).

**Method.** Seven parallel read-only auditor zones:

1. negotiation, versions, and capability families;
2. canonical codec, digests, identities, tag registry;
3. `run-execution-meaning-v4`, selections, fixed limits;
4. public wire-family DTOs and domain/wire parity;
5. storage single schema, config versioning, redaction;
6. quality-gate and test burden attributable to Slice 1;
7. ADR/register claims vs code, and the project's own unconsumed-surface bar.

Every finding carries `file:line` evidence. Nothing in the repository was
modified by the audit. The controller spot-checked the highest-impact claims
(see "Verification notes").

## 1. Criteria

**Security theater (THEATER).** A mechanism whose cost, ceremony, or rhetoric
exceeds any realistic benefit for this product, typically because (a) there is
no concrete threat or adversary (single user, one machine, one build, no
network service, no deployed users, no third-party consumers), (b) there is no
production consumer (tests written specifically for the mechanism do not
count), (c) the justification is self-referential, or (d) fail-closed friction
exists without protecting anything.

**Justified.** Prevents realistic self-inflicted harm (unbounded data causing
OOM or hangs, silent data corruption, accidental credential leakage into
logs/events/snapshots, bad model output or hand-edited config) or serves an
active production function.

**Mixed.** A real function wrapped in disproportionate ceremony, or a defense
that is computed but never reaches a real boundary.

**The project's own bar.** The 2026-09 unconsumed-surface audit states the
operational criterion verbatim: *"no producer, no consumer, and no M6-M9
deliverable claims it"*
([deferred-excluded-register.md](docs/intention-relay/reconciliation/deferred-excluded-register.md)
EXC-035/EXC-036; governing sentence in
[architecture/11-implementation-roadmap.md](docs/intention-relay/architecture/11-implementation-roadmap.md)
lines 640-655). A surface claimed by a committed downstream slice may survive
without a current producer/consumer; a surface whose only consumer is its own
tests does not.

## 2. Executive summary

- The load-bearing Slice 1 additions are small: the hello handshake with
  `Ordinary`/`RunStream` role routing, the exact-version gate, the fail-closed
  TOML schema check, and the single-SQLite-schema policy. Together these are
  under roughly 200 production lines.
- About **3,646 production lines** across the four Slice 1 files
  (`negotiation.rs`, `canonical.rs`, `run_execution_meaning.rs`,
  `contract_families.rs`) have **zero non-test consumers** anywhere in the
  workspace. About **4,698 test lines**, 91 inline tests, and 13 fixture files
  surround them; those tests exist substantially because the coverage policy
  pays for them, not because a consumer demands them.
- Estimable **clean theater** (self-referential, self-refuting, or
  enforcement-looking machinery with no reachable effect): on the order of
  **1.5-2k production lines**, plus roughly 3k lines of tests that serve only
  those mechanisms. The remainder of the unconsumed code is contract-ahead-of-
  consumer material that the project's own strategy (ADR 0035) deliberately
  pre-authors for slices 3/4 and the Mandate track.
- A second pattern is **documentary overstatement**: "fail closed", "no partial
  contract or partial effect", "authenticated input", "frozen limits",
  "credential-shaped". Several of these statements are vacuous or false against
  the current code (section 6).
- What actually protects secrets today is older and structural
  (`StartupProviderMaterial` has no `Debug`/`Display`/serde; durable payload
  assertions predate Slice 1). Slice 1 added little real protection and a large
  amount of enforcement-looking bookkeeping.

## 3. Zone verdicts

| Zone | Verdict |
| --- | --- |
| 1. Negotiation, versions, capabilities | Hello handshake justified; exact-version gate mixed; `negotiation.rs` gates, 11 error codes, 7 capability variants, and `ProtocolNegotiationResultDto` are theater (zero callers) |
| 2. Canonical codec, digests, identity, tags | Closed island with 0 consumers outside `intention-domain`; identity-exclusion setters never read; 8 exported helpers with zero callers; envelope digest verified only by tests; 21 reserved tag rows mirrored in four hand-maintained copies |
| 3. `run-execution-meaning-v4` | 0 non-test consumers, 0% reachable from non-test roots; `FixedActivityLimits` frozen-value validation duplicated three times and asserting itself; claimed `execution_meaning_capability_required` gate does not exist; "authenticated" digest claim false for an unkeyed hash |
| 4. `contract_families.rs` | 3,757 lines; all 53 public types have zero Rust consumers outside the file; 30 `validate()` methods with zero call sites; credential detector pins `"api-key"` as forbidden although the modeled `SafeHeader` feature needs exactly that; `TryFrom` compatibility bridges in a repository that bans compatibility; ~16 orphan DTOs |
| 5. Storage, config, redaction | Slice 1 footprint small (444 lines) and almost entirely reversed within a day by the ADR 0038 wave-4 work (landed via the PR #36 squash `37bad4e`; standalone `c7c0c78` is not on `main`); remaining storage/config behavior justified; real redaction value is structural and predates Slice 1 |
| 6. Quality-gate and test burden | 8,344 Slice 1 source lines, 91 tests, 13 fixtures, tier-A 95% coverage obligations on unconsumed code; tag inventory in >=6 hand-maintained copies; the parity test named after ADR 0036 never reads ADR 0036 and real ADR-vs-registry drift passes every gate; CI cache housekeeping (~250 lines) is not security theater |
| 7. Claims vs reality | Version/schema policy claims enforced; capability and canonical layers self-referential; four verified contradictions in ADR text vs code (section 6); `provider_profiles_v1` is the cleanest removal candidate under the project's own bar |

## 4. Findings by zone

### Zone 1. Negotiation, versions, capabilities

Justified:

- The `ProtocolHelloDto` handshake is shipped behavior.
  `crates/intention-protocol/src/lib.rs:94-160`; `crates/intention-transport/src/lib.rs:647-671`
  (`negotiate_client`/`negotiate_daemon`); daemon side
  `crates/intention-daemon/src/lib.rs:1197-1211`; client side
  `crates/intention-client/src/lib.rs:116-124`. The peer's declared
  `RunStreamSubscriptions` selects the response role
  (`crates/intention-transport/src/lib.rs:447-459`). Real-process E2E covers it.
- `require_exact_protocol_version` (`crates/intention-transport/src/lib.rs:686-700`)
  is cheap fail-closed protection against a stale daemon from an earlier build,
  a scenario the project explicitly defers to M9 (architecture 03:184, 348-350;
  roadmap lines 201 and 779). Mixed, because the diagnostic half does not
  reach the client: the daemon rejects before replying and swallows the error
  (`crates/intention-daemon/src/lib.rs:1024-1027`), so the client sees
  `local_daemon_connection_unavailable` (EOF mapped at
  `crates/intention-transport/src/lib.rs:762`; classification at
  `crates/intention-client/src/lib.rs:799-804`), retries for three seconds
  (`STARTUP_TIMEOUT`, `crates/intention-client/src/lib.rs:36`), and attempts a
  doomed extra daemon spawn. The integration suite admits this asymmetry
  (`crates/intention-transport/tests/transport_integration.rs:559-563`).

Theater:

- `intersect_capabilities` and duplicate rejection
  (`crates/intention-protocol/src/negotiation.rs:13-55`): zero call sites
  outside the file's own tests; duplicates are structurally impossible in the
  result (a filter over a fixed unique list) and production ignores duplicate
  entries via `contains`; no typed API can submit a duplicate.
- `require_capability` with its 11-entry error table and
  `require_gateway_tool_loop` (`negotiation.rs:58-116`, table at `:72-91`):
  zero call sites; no code path can emit any listed
  `*_capability_required` code; the gateway/bridge dependency rule protects a
  component with no production code at all.
- `ProtocolNegotiationResultDto` (`negotiation.rs:5-11`, re-export
  `crates/intention-protocol/src/lib.rs:92`): zero consumers.
- The seven post-M5 capability variants (`crates/intention-protocol/src/lib.rs:45-64,79-88`):
  advertised by nobody. The daemon advertises only the four M3 baseline
  capabilities (`intention-daemon/src/lib.rs:1197-1211`); clients require three
  baseline plus run-stream (`intention-client/src/lib.rs:35-41`). The live M4
  reasoning stream negotiates under `run_stream_subscriptions`, not
  `normalized_reasoning_stream_v1`, and the live M5 tool loop is not gated by
  `model_tool_loop_v1`.
- Two of the three hello goldens are self-referential evidence
  (`crates/intention-protocol/tests/fixtures/goldens/hello-*.json`);
  `hello-current-version-v1.json` asserts the capability list equals the same
  constant list, and `hello-incompatible-major-v2.json` backs an assertion that
  `2.0 != 1.1` while the actual rejection is tested in transport.

Cost: `negotiation.rs` is 117 production + 285 test lines, 10 unit tests, 100%
tier-A line coverage (263/263 per `quality/coverage.toml:54` and the committed
coverage report) for a module nothing calls, plus ~200 lines of post-
introduction churn (`37bad4e` +120/-21, `4bce50e` -89) that added four further
dead error codes and removed one.

### Zone 2. Canonical codec, digests, identities, tag registry

The whole layer terminates in its own tests: `canonical.rs` ->
`run_execution_meaning.rs` -> their tests; the only cross-crate references are
`contract_families.rs` importing `TagRegistry` and `credential_shaped_identifier`
from a module that is itself unconsumed.

Theater:

- Identity-exclusion setters (`crates/intention-domain/src/canonical.rs:276-360`):
  `with_credentials`, `with_filesystem_path`, `with_display_data`,
  `with_readiness`, `with_current_state` write fields that `encode` never reads
  (it iterates only `self.fields`, `:375-389`). The advertised guarantee in
  ADR 0036 ("Digest inputs exclude credentials, paths, display data, readiness,
  and current state") is the absence of a call, not a mechanism. The only
  confirming artifact, `tests/fixtures/goldens/identity-exclusion-v1.txt`, is
  byte-identical to `identity-v1.txt` except for the `record=` line (verified
  by `diff`).
- Eight exported helpers with zero callers (`canonical.rs:723-880`), including
  `contains_control_or_nul` (`:878`) whose doc comment claims rejection
  "before canonical encoding" that no code performs; `canonical.rs` has no test
  module, so those lines never execute (corroborated by the local coverage
  artifact at 77.3% line coverage).
- `NamespacedDigest` / `IdentityV1` / `for_namespace` (`canonical.rs:105-274`):
  zero production uses; runs are identified by `Uuid::new_v4()` newtypes, not
  by this scheme.
- Envelope digest verification (`crates/intention-domain/src/run_execution_meaning.rs:758-768`,
  `DigestMismatch`): the only digest comparison in the repository, reachable
  only from tests; the project's own PR24-028 ledger already recorded its
  impact as "currently test-only". The digest is unkeyed and travels inside the
  bytes it covers, so a writer can alter metadata and recompute.
- Reserved tag rows: 21 of 24 ledger rows carry no codec; the same list is
  hand-maintained in the registry (`canonical.rs:941-1054`), a domain parity
  test (`run_execution_meaning.rs:2935-3120`), the protocol descriptor table
  (`contract_families.rs:1519-1688`), and a doc-bytes pin in
  `quality/self_test.py:2124-2153`.

Justified or mixed:

- Byte-deterministic encoding and the decoder size caps are the right
  discipline and would be justified the moment a durable or IPC consumer
  exists; none exists today (storage idempotency uses unique identities, not
  digests).
- `credential_shaped_identifier` implements a real concern but its only caller
  guards DTOs nothing constructs (see zone 4).

Cost: `canonical.rs` 1,063 lines; `run_execution_meaning.rs` 778 production +
2,344 test lines; 10 goldens; `sha2 = "0.11"` plus six transitive lock crates
added in `5d81639` (`crates/intention-domain/Cargo.toml:15`).

### Zone 3. run-execution-meaning-v4, selections, fixed limits

`run_execution_meaning.rs` is 3,122 lines (778 production, 2,344 test, 44
tests). It has zero non-test consumers: no storage table, column, IPC frame,
runtime, daemon, or application code reads an execution meaning.

Theater, ranked:

- `FixedActivityLimits` frozen-value validation (`:65-190`): eight constants in
  three copies (constants, `frozen()`, and `AgentActivityLimitsV1` in
  `contract_families.rs:778-806`); `validate()` asserts equality with its own
  values; architecture 24 (lines 3-13) states these numbers are *not*
  implementation limits yet and still need classification. Nothing enforces or
  reads them. Roughly 110 production + 230 test lines.
- The claimed `execution_meaning_capability_required` fail-closed gate
  (ADR 0036 line 32): no such code exists anywhere in `crates/`; PR24 removed
  its wildcard predecessor. A documented gate that cannot be raised.
- `policy_selection_digest` (`:59`): written and re-serialized, never computed
  from or compared against the referenced snapshot; the snapshot has no
  producer.
- The envelope "authenticated input" framing (ADR 0036 Appendix A, envelope
  0x0102 field 6; code comment `:232-237` "cannot be altered without producing
  `CanonicalError::DigestMismatch`"): false for an unkeyed SHA-256 over fields
  1-5; the only test (`:1183-1230`) mutates values without recomputing.

Mixed or reserved:

- `ExecutionKind` closure (`:10-33`) is cheap and justified.
- The v4 record container is deliberately opaque to the ledger codec
  (ADR 0036 lines 206-219) but cannot validate the "immutable meaning" it is
  named for; `encode` can produce an 11-field record the decoder then rejects
  (no test covers that path).
- `FixedRunLimits` (`:44-53,277-337`): "fixed" with no frozen values, no
  validation, no reader; arbitrary values including zero round-trip.
- `AgentActivitySelectionV1` (`:193-212,339-462`): legitimate reserved material
  for the unstarted Slice 4, coupled to the unclassified constants above.
- Ten goldens pin the byte layout; five of the ten embedded payload records
  belong to families reverted by ADR 0044, so any future re-activation that
  changes those encodings forces re-pinning all of them. The v3 golden was
  removed by `37bad4e`.

### Zone 4. Public wire-family DTOs and domain/wire parity

`contract_families.rs` is 3,757 lines (1,688 production; 2,068 test; 37 tests).
All 53 public types have zero Rust consumers outside the file (two appear only
as string literals inside a foreign test; one name collides with an unrelated
enum variant). All 144 `.validate()` call sites are inside the file's own test
module; there are 30 validators.

Theater, ranked:

- The credential-shape rejection (`:43-48`, helper in `canonical.rs:844-871`):
  no credential field exists in these DTOs, and the detector pins `"api-key"`
  as credential-shaped (`:2442`), which makes the realistic header name for the
  modeled `SafeHeader` transport mode a guaranteed `credentials_forbidden`
  (fixtures avoid it by inventing `"x-safe-header"`). The single caller guards
  DTOs nothing constructs.
- `TryFrom<ForkPreviewV1/V2> for ForkPreviewDto` lossless bridges (`:533-590`):
  compatibility machinery between DTO generations in a repository whose policy
  explicitly bans compatibility (AGENTS.md; ADR 0038), consumed only by their
  own test.
- ~16 orphan DTOs outside the ledger inventory (`:247,:252,:280,:285,:290,:306,:311,:316,:320,:593,:598,:623,:668,:1012,:1255,:1480,:1504`):
  no descriptor, no parity coverage, no consumer.
- The `+1,249` lines added by `37dc501` (13 structs, 13 validators, 13 tests)
  are fully unconsumed.

Mixed:

- The 25-entry descriptor table and parity tests (`:1519-1688`,
  `:3595-3745`) are a copy-paste guard between in-workspace hand-maintained
  tables, not a wire contract; the family-name list exists in four code copies
  plus one doc pin.
- The bound validators are correct in spirit; they are inert because there is
  no producer. Deleting the bounds would be the wrong fix; wiring producers is
  the right one.

Fairness: under the project's own bar, the reserved Slice 3/4 families survive
because the roadmap claims them (architecture 11 lines 537-541, 1626, 1689;
ADR 0044 preserves them). The problem is the as-built shape: specification
transcribed into constructors and validators nobody will consume, at tier-A
coverage cost (`quality/coverage.toml:3`; architecture policy 282-287).

### Zone 5. Storage, config, redaction

Storage carries no version mechanism at all: `open()` runs one idempotent
`execute_batch` of 14 `CREATE TABLE IF NOT EXISTS` statements and one index
(`crates/intention-storage-sqlite/src/lib.rs:42-140,152-165`); no
`user_version`, no schema constant, no migration chain. A mismatched file is
neither rewritten nor silently corrupted; failures surface lazily as a generic
`storage_unavailable` (`:1968-1996`). This is simplification, not theater.

Slice 1's own footprint here (444 lines) is mostly ceremonial and was reversed
within about a day:

- `slice1_schema_three_reopen_preserves_all_m3_m4_bytes` (+332 lines, typed
  cell-by-cell byte preservation for a schema version the policy forbids
  supporting) and `slice1_storage_schema_three_remains_authoritative` (+73)
  were deleted in full by the ADR 0038 wave-4 work, which reached `main` via
  the PR #36 squash `37bad4e` (-1135/+145 in the same files). The standalone
  `c7c0c78` is not an ancestor of `main` (verified).
- A `#[doc(hidden)] pub const TEST_SCHEMA_3_SQL` was exported from production
  source for fixtures; removed with the same wave.

Justified:

- Fail-closed TOML `schema_version` (`crates/intention-config/src/lib.rs:24-33,366-397`):
  typed, secret-free errors for a realistic hand-editing mistake.
- Structural credential absence: the secret lives only in
  `StartupProviderMaterial` with no `Debug`/`Display`/serde/accessor
  (`:652-657`); snapshots carry `credential_configured: bool` only (`:238-243`).
  `safe_debug_projection` (`:483-498`) has only test consumers, so the
  protection is the DTO shape, not the projection.
- Storage secret assertions and per-fact caps predate Slice 1; Slice 1 added
  nothing to fake-secret enforcement in this zone (the Slice 1 fake-secret test
  lives in `contract_families.rs:2864-2895`).

Not Slice 1, noted for the cleanup inventory: `crates/intention-storage/tests/opaque_json_guard.rs`
(528 lines) is a hand-rolled Rust source scanner enforcing a naming/style rule
against future code, with no current violation and three self-tests; it came
with the PR #36 review remediation.

### Zone 6. Quality-gate and test burden

- Attribution: `5d81639` = 10,490 insertions across 48 files; `37dc501` = +1,249
  in one file. Current on `main`: **8,344 source lines** across four files
  (3,646 production + 4,698 test), 91 inline tests, 13 fixtures (108 lines),
  plus ~250 lines of CI cache housekeeping (198-line script, 32-line workflow,
  26-line self-test case).
- Test/production ratio is roughly 1.3:1 overall, and for `negotiation.rs` and
  `run_execution_meaning.rs` about 2.4:1 and 3:1 respectively.
- The tests are load-bearing for the coverage gate, not for product behavior:
  deleting the mechanisms would delete their only consumers, break no product
  scenario, and fail tier-A coverage unless the policy or exclusions also
  change.
- Tag inventory exists in at least six hand-maintained copies across two crates
  plus a doc pin; any tag change forces lockstep edits. The test named
  `tag_registry_parity_with_the_adr_0036_ledger` never reads ADR 0036, and the
  real ADR-vs-registry drift (ADR 0036 still shows `Wired (Slice 2)` for
  0x0206-0x020B; the registry says `ReservedForSlice2`) passes every gate.
- `quality/self_test.py` gained exactly one Slice 1 case; it belongs to cache
  housekeeping. The six `test_slice2_*` doc-pinning cases are PR #36 work that
  outlived the Slice 2 revert (they pin text of the superseded ADR 0037).
- CI cache housekeeping is expressly not security theater; it manages the
  GitHub Actions cache limit.

### Zone 7. Claims vs reality

Enforced claims: version ledger exact equality (`transport/lib.rs:686-700`,
called from the live handshake), TOML fail-closed, single SQLite schema, no
synthetic post-M5 records. Wording overstated: "runtime version assertions"
overstates constant references in `intention`/daemon.

Self-referential claims: the capability layer, the canonical codec, the parity
tests (both tables live in the same workspace and are edited together; the
joint consumer does not exist).

Removal candidates under the project's own bar:

- `provider_profiles_v1`: no producer, no consumer, and no authorized slice
  (ADR 0044 lines 187-195: Slice 2 can return only through a new activating
  specification). PR24 had already recorded the identical gap (`pr24-code-review-ledger.md:45`).
- The six Slice 2 wire DTOs still public in `contract_families.rs` despite
  ADR 0044 line 170-171 claiming "no canonical record, public DTO, or serving
  surface exists for them": `ProviderProfileRevisionV1` (`:138`),
  `ResolvedRunProviderSelectionDto` (`:57`), `ReasoningHistoryManifestDto`
  (`:628`), `ContextSourceManifestV1` (`:698`), `ModelContextProjectionV1`
  (`:732`), and their descriptors in the parity list.
- The dead gate helpers and codes in `negotiation.rs` (zone 1).

Fairness: no register cites removed test names for Slice 1 rows; the registers
were reconciled after ADR 0038. The ADR texts were not.

## 5. Quantification

| Measure | Value |
| --- | --- |
| Slice 1 source (four files, current `main`) | 8,344 lines: 3,646 production + 4,698 test |
| Inline tests attributable to Slice 1 | 91 (44 + 37 + 10), ~9.5-14% of the workspace suite depending on the counted baseline |
| Fixtures | 13 files, 108 lines (10 domain goldens, 3 hello goldens) |
| Non-test consumers of Slice 1 code | 0 outside its own modules; the hello handshake and version constants are the exceptions |
| Load-bearing Slice 1 mechanisms | < ~200 production lines |
| Coverage obligation | tier A (95%) on `intention-domain` and `intention-protocol`; `negotiation.rs` at 100% (263/263) |
| New dependency | `sha2 0.11` + block-buffer, const-oid, crypto-common, digest, hybrid-array, typenum |
| Storage Slice 1 footprint | 444 lines added; ~1,135 deleted by the wave-4 work within a day |
| Tag inventory copies | >= 6 (registry, domain parity test, protocol descriptors, protocol parity test, self-test doc pin, ADR text) |
| Test profiles x CI | up to 3 profiles x 2 OS runners per push for these tests |

## 6. Documentation contradictions (ADR 0036/0038/0044 vs code)

1. **"No new crate, dependency, feature, coverage tier, or exclusion is
   introduced"** (ADR 0036 lines 101-102): contradicted by `sha2 = "0.11"` in
   `crates/intention-domain/Cargo.toml:15` and the allowlist change in
   `quality/architecture.toml:96`, both from the same commit. The registers are
   honest and contradict the ADR (`evidence-register.md:61`,
   `deferred-excluded-register.md:54`).
2. **Tag statuses**: ADR 0036 (lines 148-189) still presents 0x0206-0x020B as
   `Wired (Slice 2)`; the registry says `ReservedForSlice2`
   (`canonical.rs:975-1000`). ADR 0036 remains Accepted.
3. **`TagStatus` variants**: ADR 0038 (lines 124-125) says the enum keeps
   exactly `Wired`, `ReservedForSlice3`, `ReservedForSlice4`; the code has four
   variants including `ReservedForSlice2` (`canonical.rs:886-895`).
4. **"No public DTO exists for 0x0206-0x020B"** (ADR 0044 lines 170-171):
   false; five public DTOs and their parity descriptors remain (see zone 7).
5. **Dangling error code**: ADR 0036 line 32 lists
   `execution_meaning_capability_required`, removed by PR24 and absent from the
   tree.
6. **Evidence count mismatch**: `source-of-truth-matrix.md:68` says "962
   default-profile tests" while `evidence-register.md:57` says "626 tests" for
   the same verified run.
7. Minor: roadmap duplicate "(2026-09) (2026-09)"
   (`architecture/11-implementation-roadmap.md:640`); appendix notes at ADR
   0036 lines 269/275/281/287 retain a disjunction ("ReservedForSlice3 or Wired
   (Slice 2)") that describes no registry state; orphaned source-scan doc
   comment in `crates/intention-storage-sqlite/tests/sqlite_contracts.rs:948-949`.

## 7. Cleanup candidates (audit observations; decisions pending)

Applying the project's own bar to the surfaces above:

**Remove or defer under the bar (no producer, no consumer, no committed
claim):**

- `negotiation.rs` gate helpers, the 11-code error table,
  `ProtocolNegotiationResultDto`, and the two self-referential hello goldens
  (keep the constants, the hello DTO, and the exact-version gate).
- `provider_profiles_v1` and the six Slice-2 family DTOs still public in
  `contract_families.rs`, or re-authorize them with a committed slice.
- ~16 orphan DTOs and the `TryFrom` fork-preview bridges.
- Identity-exclusion setters, the zero-caller canonical helpers, and the
  identity/digest surface that no durable or IPC consumer uses.
- `FixedActivityLimits` frozen-value validation (the constants can stay as
  documentation once classified), the `policy_selection_digest` field, and the
  "authenticated input" wording.
- The credential detector's `"api-key"` rejection (or make it consistent with
  the `SafeHeader` feature), since the protected field does not exist.

**Keep but stop presenting as delivered protection (reserved for committed
slices):** reserved wire families consumed by slices 3/4/Mandate
(tool/registry/loop, bridge, fork, activity); the v4 record shapes and
goldens; `AgentActivitySelectionV1`; bound validators (inert until a producer
exists).

**Documentation fixes:** items 1-7 in section 6.

**Cheap functional fix worth considering:** have the daemon send a typed
`incompatible_protocol_version` frame before closing, so the one justified
negotiation mechanism actually surfaces its diagnosis instead of a three-second
stall and a spurious daemon spawn.

## 8. What is justified and must not be swept away

- Hello handshake and `Ordinary`/`RunStream` role routing.
- Exact-version equality (fail-closed against stale peers), improved diagnosis
  optional.
- TOML fail-closed schema version.
- Single SQLite schema created directly on open; no migration machinery.
- Structural credential absence (`StartupProviderMaterial`, `credential_configured`).
- `ExecutionKind` closure, strict framing, byte-stability goldens (cheap
  discipline; becomes load-bearing with a consumer).
- Durable storage secret assertions and resource caps (predate Slice 1).
- CI cache housekeeping (not security).

## 9. Verification notes

Spot checks performed by the controller against the current tree (read-only):

- `"api-key"` is pinned as credential-shaped in
  `crates/intention-protocol/src/contract_families.rs:2442`.
- `crates/intention-domain/tests/fixtures/goldens/identity-v1.txt` and
  `identity-exclusion-v1.txt` differ only in the `record=` line.
- `POST_M5_CAPABILITIES` is referenced only inside `intention-protocol`
  (definition, the dead module, and its tests).
- `git merge-base --is-ancestor c7c0c78 HEAD` is false: the ADR 0038 wave-4
  content reached `main` through the PR #36 squash `37bad4e`, not as the
  standalone commit.
- `git status` clean at audit time; the audit changed nothing.

## 10. Open questions

- Whether the project treats reserved failure codes and gate helpers as
  "authorized vocabulary" for future slices; ADR 0038 expressly protected
  `require_capability`/`require_gateway_tool_loop` from one removal wave, which
  is why zones scored the helpers as theater rather than unauthorized code.
- Whether Slice 3 will consume the current `contract_families.rs` constructors
  or generate its own; the ledger field tables will be consumed, but the
  validators as-built likely will not.
- Whether `FixedRunLimits` should be a contract record at all, or prose in
  architecture 27 until Slice 3 freezes values.
- Whether the v4 golden payloads (including five Slice-2-family records) will
  be re-pinned by the next activating specification.
