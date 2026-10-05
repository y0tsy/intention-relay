# Complexity audit: over-engineering inventory and Pareto cut plan

**Status.** Non-authoritative audit artifact. This document records findings and a proposed cut
plan. It is not a normative record, it owns no policy, and it is deliberately **not** in the
architecture reading path (`architecture/README.md`). Nothing here is authorized work until an
owner document or ADR adopts it.

**Baseline.** `docs/complexity-audit` at `8f90cf9`. Measurements are line counts of the cited
ranges and results of read-only searches at that revision.

**Method.** Seven independent read-only audits, one per zone, each with the same instructions:
find over-engineering by neutral operational definition (code, configuration, documentation, or
process that costs more to keep than the reachable behaviour it serves); cite every claim with a
path and line range; steelman the mechanism before proposing a cut; assign one of four
dispositions (`DELETE`, `MERGE`, `SIMPLIFY`, `KEEP-BUT-STOP-INVESTING`); rank cuts by complexity
removed per unit of real capability lost; and state where cutting stops paying. Documentation was
treated as a cost, never as proof of value. Zone reports were not permitted to see each other, and
the audit brief contained no examples of suspected problems. Where two zones independently reached
the same mechanism, that convergence is noted rather than merged away.

**Zones.**

| Zone | Scope | Production Rust | Test Rust | Documents |
| --- | --- | ---: | ---: | ---: |
| Z1 contracts | `intention-types`, `intention-domain`, `intention-protocol` | 5,792 | 3,544 | — |
| Z2 storage | `intention-storage`, `intention-storage-sqlite` | 3,694 | 3,571 | — |
| Z3 execution | `intention-runtime`, `intention-application`, `intention-model`, providers | 6,695 | 8,403 | — |
| Z4 edges | `intention-daemon`, `intention-transport`, `intention-client`, `intention`, `intention-test-support` | 7,080 | 6,472 | — |
| Z5 periphery | `intention-tools`, `intention-workspace`, `intention-hooks`, `intention-config`, placeholders, `quality/`, `Makefile`, `.github/` | 4,279 | 3,883 | — |
| Z6 architecture | `architecture/*` | — | — | 9,904 |
| Z7 records and root docs | `decisions/`, `closeout/`, READMEs, `AGENTS.md`, `docs/reference/`, root audit | — | — | 3,806 + root audit |

Repository totals at baseline: **28,411 production Rust lines, 25,015 test Rust lines, 24 crates,
179 distinct `*Dto` types, 320 public functions, 14 SQLite tables, 512 test functions, 43 ADRs,
31 architecture documents.** The product is one binary (`intention-daemon`) with no UI.

---

## 1. Executive summary

The audit found a real, measurable over-engineering problem, and it is **not** where the surface
area suggests.

- **~16,000 lines can be removed for ≤2% real capability or protection loss.** That is ~3,700
  production Rust lines (13.0% of production code), ~2,830 test lines (11.3%), and ~9,800
  documentation lines. No reachable user capability is lost in the zero-loss subset.
- **The removal is lopsided, and one file carries most of the documentation half.** Of the
  ~9,800 removable documentation lines, 6,157 are a single orphaned audit file. Within the live
  architecture-and-decision corpus (13,710 lines), ~3,650 lines (27%) are removable, and **57% of
  the architecture corpus (5,607 of 9,873 lines) normatively describes systems with zero
  implementing code** (Z6); counting the 33 unbuilt decision records, 7,855 lines describe
  systems with no code. Only **~13% of production Rust is removable without losing behaviour.**
- **The Pareto target inverts.** Cutting 80% of production code would destroy the durable-fact
  append authority, the decode-boundary validation, the transport liveness bounds, and the
  commit-before-publication boundary. The honest result is: **the removable fraction is large in
  the documentation (~9,800 lines) and small in the code (~13% of production Rust)**, and the
  largest single item is not code at all.
- **The most expensive-looking code is the cheapest to keep.** 751 lines of hand-written
  `Deserialize` and 33 mirror structs (Z1), the stream-ordering validator, the two-step
  cancellation, and the typed error shape are the mechanisms the whole workspace depends on.
  Deleting them is the most damaging change available, not the most valuable.
- **The repository has already proven it accepts this material as disposable.** Three compaction
  commits (`1c9ecab`, `6ae38d6`, `8f90cf9`) removed ~29,000 documentation lines. The residue is
  that the compaction deleted artifacts without repairing the 54 documents that still reference
  them (Z7-04).

### Headline numbers

| Measure | Value |
| --- | ---: |
| Production Rust removable, zero reachable loss | ~3,700 lines (13.0%) |
| Test Rust removable, zero reachable loss | ~2,830 lines (11.3%) |
| Documentation removable, zero reachable loss | ~9,800 lines (6,157 in one orphaned file) |
| **Total removable at ≤2% real loss** | **~16,000 lines** |
| Production Rust where cutting stops paying | ~24,700 lines |
| Distinct concepts (types, resolvers, registries, vocabularies) removed | ~120 |
| Public types removed | ~65 |
| Crates removed | 2 (`intention-test-support`, `quality/harness`) |
| SQLite tables removed | 2 (`tool_results`, `session_snapshots`) |
| False statements corrected | ~40 |

---

## 2. Cross-zone themes

Seven patterns recur across independent zones. Each is the same underlying defect wearing
different clothes, so a fix should be applied per pattern, not per finding.

### T1 — Vocabulary authored ahead of its consumer

Typed DTOs, event variants, error codes, and closed sets declared for systems that have no
producer or reader. This is the dominant cost in Z1 and Z6 and reappears in Z2, Z5, and Z7.

- Z1-F01 (dead `ToolResultRecorded` event family), Z1-F02 (`PlanRevisionId` is declared once and
  referenced nowhere, not even in tests), Z1-F03 (pagination DTOs dead since M1), Z1-F05 (`schema_version`
  on 17 DTOs, validated nowhere), Z1-F07, Z1-F08, Z1-F09, Z1-F10.
- Z2N-5 (`ToolResultKindDto` is a second tool-name vocabulary), Z2N-11 (error codes no production
  code branches on).
- Z6-F04 (**121 closed failure codes specified; none appear anywhere in `crates/`**), Z7-01 (33
  "accepted future direction" ADRs with zero code).

The counter-rule this violates already exists in the corpus: ADRs 0032/0033/0034 bundle many
directions into short registers, which is the form the corpus itself converged on.

### T2 — One fact owned in several places, already disagreeing

Not duplication for readability: the copies have drifted, and a gate checks none of them.

- Z1-F06 (same session identity carried twice in one message; ~122 lines assert the copies agree).
- Z2N-6 (512 KiB as three constants in three crates), Z2N-9 (one queue fact in four places,
  inconsistent after a removal).
- Z4-F02 (two daemon e2e binaries duplicate a ~250-line fixture harness), Z4-F09 (platform path
  layout re-derived in seven places).
- Z5-2 (provider-SDK ownership enforced twice by two disagreeing mechanisms).
- Z6-F02 (**four of seven restated facts are false as written**), Z6-F05, Z6-F07, Z6-F09.
- Z7-02 (`production-ceiling-removal.md:56` says "these five safeguards are the only numeric
  safeguards retained"; ADR 0048 lists **nine**, and code confirms all nine exist), Z7-05 (root
  README claims a capability its own ADR says is not implemented), Z7-07, Z7-09.

### T3 — A durable record with no reader

Data is written, committed, and never read; sometimes it is read only by a test.

- Z1-F04 (session replay tail is always empty), Z2N-1 (`load_tool_result` has **zero production
  callers**), Z2N-10 (run replay tail always empty), Z3-F01 (write-only canonical tool-result
  document duplicating a typed fact that *is* read), Z4-F03 (`RunStreamClient` has no non-test
  consumer), Z5-7 (second unreachable result path in `intention-tools`).

Z2N-1 and Z3-F01 are the same defect found independently from the storage and the application
side: the same tool result is stored twice, and the copy that the model consumes is not the copy
the durable reader would read.

### T4 — Production API that exists only for tests

- Z3-F04 (`ModelRunFirstAppendGate` and a third service constructor, ungated in production API),
  Z3-F06 (public tool-invocation wrappers whose only callers are tests).
- Z4-F01 (`intention-test-support` serves exactly one 70-line TUI test), Z4-F05 (daemon
  test-support surface inside production source: 69 conditional sites, six copies of one struct
  literal, a divergent second dispatcher).
- Z2N-12 (~330 lines of DTO accessor tests driven by the 80% line-coverage floor).
- Z5-11 (test targets split by coverage goal rather than by contract).

### T5 — Documentation describing code that does not exist

- Z6-F01 (architecture 14: 127 lines, entire subject is deleted machinery, still in the reading
  path), Z6-F04, Z6-F06 (architecture 29 specifies a reverted surface in the present tense),
  Z6-F09 (removed codec re-narrated in 24 of 32 documents).
- Z7-04 (dangling references to deleted artifacts in 54 documents, including live ADR 0049),
  Z7-11 ("fake-secret absence tests" named as live evidence in nine documents but names no
  runnable artifact).
- Z5-6 (four documents still claim deleted expected-failure fixtures exist), Z5-15.
- Z1's zone report adds architecture `04:183` and `02`'s "Future DTO families" as the only
  justification for several dead types.

### T6 — Redundant validation of one invariant

- Z3-F02 (provider-selection invariant validated three times; the first check is a **tautology**
  in the only production request constructor), Z3-F10.
- Z1-F06 (identity agreement re-checked), Z2N-4 ("terminal result evidence is mandatory" asserted
  in a comment and enforced nowhere — the zone's own test commits a `Completed` event with no
  evidence and expects success), Z4-F07.

### T7 — One policy, three owners, no claim gate

`quality/check_docs.py` (84 lines) validates fence balance, Mermaid kind, link resolution, and a
secret-shaped-assignment regex. It checks no factual claim. Meanwhile one rule has an owner
document, consequence paragraphs in up to nine shipped documents, and a README table row.

- Z6-F02 (no claim gate; stale SDK pins, a required migration the policy forbids, a non-ancestor
  baseline hash), Z7-02, Z7-04, Z7-06.
- Z5-6 (no negative proof that any checker can fail).
- Z6-F03 (**620 lines of future-mechanism "consequence" sections inside the ten shipped
  `Current policy` documents**, plus 122 README lines).

---

## 3. Consolidated cut plan

Three phases. Phase 1 needs no product decision; phase 2 needs one named decision; phase 3 is
"stop investing" rather than "delete".

### Phase 1 — cut now, zero reachable loss (~15,400 lines)

| # | Zone | Finding | Disposition | Prod | Test | Docs | Real loss |
| ---: | --- | --- | --- | ---: | ---: | ---: | --- |
| 1 | Z7 | Z7-03 delete root `architecture-fitness-audit.md` | DELETE | — | — | 6,157 | 0% |
| 2 | Z7 | Z7-01 bundle 33 unbuilt ADRs into one direction register | MERGE | — | — | ~2,000 | 0% |
| 3 | Z7 | Z7-07 de-duplicate architecture README | MERGE | — | — | ~100 | 0% |
| 4 | Z7 | Z7-08 shrink ADR 0038 to policy + ledger | SIMPLIFY | — | — | ~90 | 0% |
| 5 | Z7 | Z7-02 delete stale `production-ceiling-removal.md` | DELETE | — | — | 75 | 0% |
| 6 | Z7 | Z7-04/06/09 repair dangling refs, closeout boilerplate, slice restatement | SIMPLIFY | — | — | ~82 | 0% |
| 7 | Z6 | Z6-F03 future-mechanism sections inside shipped docs | SIMPLIFY | — | — | ~500 | low |
| 8 | Z6 | Z6-F04 closed failure-code fences for unbuilt systems | DELETE | — | — | ~280 | 0% |
| 9 | Z6 | Z6-F01 delete architecture 14 | DELETE | — | — | ~127 | ~0% |
| 10 | Z6 | Z6-F05/F08/F06/F09/F02/F07 (roadmap restatement, CI narrative, reverted detail, codec re-narration, stale facts, README invariants) | SIMPLIFY | — | — | ~385 | 0–low |
| 11 | Z1 | Z1-F01/F02/F03/F04/F07/F08/F09/F10 dead vocabularies and unreachable states | DELETE | ~1,110 | — | — | 0% |
| 12 | Z3 | Z3-F01 write-only canonical tool-result document + orphan reader | DELETE | ~424 | ~933 | — | 0% |
| 13 | Z3 | Z3-F03 six inline hook-phase ladders → one | SIMPLIFY | ~180 | — | — | 0% |
| 14 | Z3 | Z3-F02/F04/F05/F06/F09/F10 | SIMPLIFY | ~200 | ~40 | — | 0% |
| 15 | Z2 | Z2N-1 tool-result read path (step 1) | DELETE | ~90 | ~275 | — | 0% |
| 16 | Z2 | Z2N-7 delete `opaque_json_guard.rs` | SIMPLIFY | — | ~450 | — | alias-evasion coverage only |
| 17 | Z2 | Z2N-12 accessor tests driven by the coverage floor | SIMPLIFY | — | ~330 | — | 0% |
| 18 | Z2 | Z2N-11/05/02/06/09/03/04/08 error boilerplate, second tool vocabulary, duplicated query/loop, 512 KiB constants, queue fact, snapshot cache, false invariant | SIMPLIFY/MERGE | ~260 | ~50 | — | ~0% |
| 19 | Z4 | Z4-F01 delete `intention-test-support` crate | DELETE | 188 | 274 | — | 0% |
| 20 | Z4 | Z4-F02 merge duplicated e2e harness into the fixture crate | MERGE | — | ~250 | — | 0% |
| 21 | Z4 | Z4-F05/F06/F07/F08/F09/F10/F11 | SIMPLIFY/MERGE/DELETE | ~285 | ~100 | — | 0% |
| 22 | Z5 | Z5-1/3/4/5/10/13 dead policy keys, dead manifest, empty extension point, marker crate, aliases, fallback | DELETE | ~144 | — | — | 0% |
| 23 | Z5 | Z5-2 merge duplicate provider-SDK check | MERGE | 17 | — | — | 0% |
| 24 | Z5 | Z5-6 add negative proof for checkers; fix four false doc claims | SIMPLIFY | — | — | — | 0% |

**Phase 1 total:** ~2,900 production lines, ~2,700 test lines, ~9,800 documentation lines.

### Phase 2 — needs one named decision (~1,500 lines)

| # | Zone | Finding | Decision required | Lines |
| ---: | --- | --- | --- | ---: |
| 1 | Z2/Z3 | Z2N-1 step 2 / Z3-F01 row-vs-content: delete the whole durable tool-result evidence store | Product: is doc `04:238`'s "stored tool/run audit" a requirement? | ~500 |
| 2 | Z4 | Z4-F04 collapse two transport stacks to one | Design: does the client keep a blocking API? | ~90–300 prod |
| 3 | Z4 | Z4-F03 `RunStreamClient` (unreachable today) | Roadmap: is a streaming client surface still scheduled? | ~150 |
| 4 | Z5 | Z5-7 second unreachable result path in `intention-tools` | Tool-contract: keep or delete the envelope family | ~250 |
| 5 | Z5 | Z5-8 hook non-`Continue` surface | Docs: edit doc 05's fail-open bullet | ~180 |
| 6 | Z2 | Z2N-10 always-empty run replay tail | Wire contract: is a one-shot catch-up backend planned? | ~70 |
| 7 | Z6 | Z6-F03 remainder: owner documents 13/16/17/19/20/21/23/24/27 | Scope: keep or delete the design record for M10–M12 systems | ~4,000 |
| 8 | Z7 | Z7-10 `docs/reference/prime-agent-research/` | Retention: keep as reference (KEEP-BUT-STOP-INVESTING) | 4,153 |

### Phase 3 — never build (stop authoring ahead of consumers)

These are not deletions; they are an engineering rule the audit recommends adopting, because T1 is
the mechanism that produced most of the removable material.

- Do not declare closed failure-code sets, lifecycle sets, DTO families, or event variants before a
  consumer exists. Pre-named codes for unbuilt systems are the single largest documentation cost
  (Z6-F04: 121 codes, none in use).
- Do not add `schema_version` (or equivalent) to a DTO under the single-version policy (Z1-F05,
  Z1-F13).
- Do not carry the same identity or the same limit in two fields "for safety"; the second copy
  needs a reader or it does not exist (Z1-F06, Z2N-6).
- Do not add test seams to unconditional production API; gate them behind the existing
  `test-support` feature (Z3-F04, Z4-F05).
- Do not author a normative document for a mechanism that has no producer; record the decision in
  an ADR instead (Z6-F01, Z6-F06, Z7-01).

---

## 4. Do-not-cut register

Mechanisms that look expensive and are load-bearing. Cutting these is the most damaging change
available, not the most valuable.

| # | Mechanism | Why it survives |
| ---: | --- | --- |
| 1 | 751 lines of hand-written `Deserialize` + 33 mirror structs (Z1-F11) | The decode-boundary validation the entire workspace depends on. 12% of Z1 production code. |
| 2 | Stream-ordering validator; `!durable_output` retry gate; two-step cancellation + append-race recovery; commit-before-publication (Z3) | Real correctness boundaries; the only coverage of the terminalizer/publication-retry paths. |
| 3 | `MAX_MESSAGE_BYTES`, `CONNECT_TIMEOUT`, `SYNC_IO_TIMEOUT`, subscriber queue capacity, write deadline, stale-socket identity-checked reclaim (Z4) | Without them a peer that accepts and stops reading hangs a thread or allocates without bound; an unclean exit blocks every restart. ADR 0048 keep-list. |
| 4 | Structural credential absence; typed error shape (Z1-F12, Z4) | Keeps secrets out of errors without runtime content scanning. |
| 5 | `normalize_tool_result` (the *read* representation) | Do not confuse it with Z3-F01's write-only document. |
| 6 | 8-phase hook pipeline; capability flags (Z3-F07) | Documented invariants (arch 05, arch 08); underused, not wrong. |
| 7 | Observational CI collectors (~1,014 lines, Z5-16) | They are the evidence behind the CI restructuring decisions. KEEP-BUT-STOP-INVESTING: build no more. |
| 8 | `THIRD_PARTY_NOTICES.md` disclosure gate (Z5-17) | The only artifact a future distributor needs. |
| 9 | `intention-test-support` *feature* (as opposed to the crate) | Integration tests cannot see `cfg(test)` items; the daemon's five outcome binaries need the seams. |
| 10 | `WorkspaceRoot` resolution ordering; `parse_tool_input`'s exhaustive match | Ordering guarantee is real; the match is compile-time coupling to `ToolId`. |
| 11 | `docs/reference/prime-agent-research/` (4,153 lines, Z7-10) | Preserved research with no reachable consumer; KEEP-BUT-STOP-INVESTING, do not delete. |
| 12 | The designated-files mechanism with an empty list (Z4) | Adopted by ADR 0049; costs one config key. Do not add entries without a named high-risk file. Removed 2026-10-05 by [ADR 0051](decisions/0051-per-crate-coverage-tiers.md), which replaced it with per-crate tiers. |

---

## 5. Where the surface area misleads

Three things the audit expected to be the problem and were not:

- **The cursor surface.** Cursors (`RunEventCursorDto`) appear 262 times and the user-facing
  complaint named them. The audits found the cursor *plumbing* is the live bounding mechanism the
  system actually uses (Z1-F03: pagination DTOs are dead precisely because cursors replaced them).
  The cost is in the DTOs that cursors superseded, not in cursors.
- **The "verification and mandate" vocabulary.** "Mandate" appears **zero** times in `crates/`.
  The cost is the ~5,600 lines of architecture documents (06, 07, 13–30) plus 2,248 lines of
  unbuilt ADRs that specify it (Z6-F04, Z7-01), not any runtime machinery.
- **The DTO count (179 types).** Most are the price of the DTO-first policy and are consumed. The
  dead ones are individually small; the cost is the vocabulary a maintainer must hold, which is
  why the recommendations are per-pattern (T1–T7), not per-type.

---

## 6. Confidence and open questions

**High confidence** on reachability and line counts for: all Z1 findings (exhaustive `rg` outside
the defining crate), Z2N-1/2/5/6/9/10/11/12, Z3-F01/02/03/06, Z4-F01/02/05/08, Z5-1/3/4/5/10/13,
Z6-F01/02/04/05/07/08, and all Z7 findings (code- and git-verified, including
`git merge-base --is-ancestor bf40567 HEAD` → exit 1).

**Medium-high confidence:** Z1-F05/F06 dispositions (both are required by architecture 02's policy
text, so a reviewer may prefer the belt-and-braces form; the cheapest check is whether any
validator has ever fired — none, since all fixtures go through the same constructors); Z4-F04
(needs a client-API design decision); Z5-2/6/7/8/11; Z6-F03/F06/F09.

**Medium confidence:** Z2N-3 (read-latency trade-off needs a measurement); Z2N-13 and Z3-F07
(KEEP-BUT-STOP-INVESTING, need a product/adapter check); Z5-14/17.

**Open questions for the maintainer.**

1. Is doc `04:238`'s "stored tool/run audit" a requirement? A yes keeps ~500 lines (Z2N-1 step 2);
   a no removes the whole durable tool-result evidence store.
2. Does the client keep a blocking API? A no collapses two transport stacks (Z4-F04).
3. Are M10–M12 systems (Mandate, scheduler, child graph, bridge, kernel, Goals/Skills, forks)
   still scheduled? A yes keeps their owner documents (~4,000 lines); a no removes them.
4. Should the direction register (Z7-01) replace the 33 unbuilt ADRs, or should they be deleted?

**Known uncertainty.** LOC figures are line counts of cited ranges, not compiled diffs; no edits
were made. "Real loss" is a judgement of reachable capability, not a measured product metric. The
audits did not measure reviewer time or build cost.

---

## 7. Relation to the pre-existing `architecture-fitness-audit.md`

The 6,157-line root audit covered the same repository earlier. This audit's zone numbering does
not match it, and its findings are partly stale at this revision: ADR 0046 removals have already
landed, so `intention-domain/src/canonical.rs`, `run_execution_meaning.rs`, and
`intention-protocol/src/contract_families.rs` no longer exist, and the audit's "8-vs-9 checks"
defect is fixed (`architecture/12:66,75` both say "eight").

What this audit adds that the prior audit did not report: the false evidence invariant (Z2N-4),
the orphaned `load_tool_result` and its byte-exact document tests (Z3-F01), the six-copy hook-phase
ladder and its drift (Z3-F03), the ungated runtime race-fixture seam (Z3-F04), the single-waiter
cancellation primitive (Z3-F05), the duplicated e2e harness (Z4-F02), the entire quality-tooling
surface (12 of Z5's 17 findings), the `architecture-fitness-audit.md` file itself (Z7-03), the
5-vs-9 safeguard contradiction (Z7-02), the root README overclaim (Z7-05), and the quantified
dangling-reference residue (Z7-04).

What the prior audit reported and this one confirms without restating: the `run_snapshots`
write-only table, the single-connection/mutex model, unbounded context, the session event stream
delivered to nobody, content duplication, commit granularity, the trait-default bodies, and the
instance-independent database path.

---

## Appendix A — Zone reports

Each zone produced a full report with per-finding evidence, steelman, minimal alternative, impact
estimate, confidence, and coverage statement. The reports are the source of record for every claim
in this document.

| Zone | Findings | Estimated reduction | Stopping point |
| --- | ---: | ---: | --- |
| Z1 contracts | 13 | ~1,292 prod (22% of zone) | after tier 2 (F11 is the mechanism) |
| Z2 storage | 13 | ~370 prod + ~1,085 test (top 8, zero loss); ~470 + ~1,150 with the first genuine losses | after Z2N-1 step 1 |
| Z3 execution | 11 | ~785 prod + ~975 test | after rank 6 |
| Z4 edges | 11 | ~690–900 prod + ~725 test (~11% of zone) | below the seams and the single transport |
| Z5 periphery | 17 | ~470 prod + 1 crate | after tier 2 |
| Z6 architecture | 9 | ~1,300 doc (13% of zone) | at Z6-F07/F09 |
| Z7 records and root docs | 11 | ~8,505 doc | at rank 9 |

### Z1 — Contracts and domain

| ID | Title | Disposition | LOC | Loss |
| --- | --- | --- | ---: | --- |
| Z1-F01 | Dead `ToolResultRecorded` domain event family | DELETE | ~508 | 0% |
| Z1-F02 | Dead plan/config-revision vocabulary, incl. 2 never-produced events | DELETE | ~202 | 0% |
| Z1-F03 | Pagination DTOs dead since M1 | DELETE | ~107 | 0% |
| Z1-F04 | Session replay tail always empty; continuity validator unexercised | SIMPLIFY | ~183 | 0% |
| Z1-F05 | `schema_version` on 17 DTOs, validated on no read path | SIMPLIFY | ~60 | 1 redundant check |
| Z1-F06 | Same identity carried twice; ~122 lines assert agreement | SIMPLIFY | ~122 | 1 redundant check |
| Z1-F07 | `ErrorDetailDto` reserve, zero producers | DELETE | ~79 | 0% |
| Z1-F08 | 3 unreachable `DaemonReadinessDto` states | SIMPLIFY | ~27 | 0% |
| Z1-F09 | `RunStatusDto::WaitingInput` never entered | DELETE | ~10 | 0% |
| Z1-F10 | `ProtocolAcceptedDto.correlation_id` random UUID, no reader | DELETE | ~8 | 0% |
| Z1-F11 | 751 lines hand-written `Deserialize` + 33 mirror structs | KEEP-BUT-STOP-INVESTING | 0 | — |
| Z1-F12 | 45 zone / 216 repo error codes, no closed list | KEEP-BUT-STOP-INVESTING | 0 | — |
| Z1-F13 | `EventMetadataDto.schema_version` write-only and inconsistent | SIMPLIFY | ~2 | 0% |

### Z2 — Storage

| ID | Title | Disposition | Prod | Test | Loss |
| --- | --- | --- | ---: | ---: | --- |
| Z2N-1 | Durable tool-result content store has no reader | DELETE (read) + VERIFY (write) | ~90 | ~275 | none |
| Z2N-2 | Duplicated run query + projection loop in two snapshot fan-outs | MERGE | ~25 | — | none |
| Z2N-3 | `session_snapshots` materializes a recomputed projection | SIMPLIFY | ~26 | ~10 | small latency |
| Z2N-4 | "Terminal result evidence is mandatory" enforced nowhere | THEATER | 2 | — | none |
| Z2N-5 | `ToolResultKindDto` second tool-name vocabulary | MERGE | ~56 | ~20 | durable discriminator stability (unread) |
| Z2N-6 | One number (512 KiB), three constants, three crates | MERGE | ~4 | — | none |
| Z2N-7 | 528-line hand-written parser to enforce a doc rule | SIMPLIFY | — | ~450 | alias-evasion coverage |
| Z2N-8 | `recover_unfinished_runs` returns evidence all callers discard | SIMPLIFY | ~12 | — | none |
| Z2N-9 | One queue fact in four places, already inconsistent | MERGE | ~25 | ~20 | none |
| Z2N-10 | `RunReplayDto.tail` always empty, no production reader | SIMPLIFY | ~30 | ~40 | a one-shot catch-up backend |
| Z2N-11 | 9 error-constructor boilerplate fns + 3 synonymous codes | SIMPLIFY | ~110 | — | none |
| Z2N-12 | ~330 lines of DTO accessor tests for the coverage floor | SIMPLIFY | — | ~330 | none |
| Z2N-13 | Turn-acceptance idempotency: two mechanisms, no retry | KEEP-BUT-STOP-INVESTING | 0 | 0 | — |

### Z3 — Execution and models

| ID | Title | Disposition | Prod | Test | Loss |
| --- | --- | --- | ---: | ---: | --- |
| Z3-F01 | Write-only canonical tool-result document + orphaned reader | DELETE | ~424 | ~933 | none |
| Z3-F02 | Provider-selection invariant validated 3×, once tautologically | SIMPLIFY | ~35 | small | none |
| Z3-F03 | Six near-identical hook-phase ladders in one method | SIMPLIFY | ~180 | — | none |
| Z3-F04 | Race-fixture seam in unconditional production API | SIMPLIFY | ~40 | — | none |
| Z3-F05 | Multi-waiter cancellation primitive for single-waiter workload | SIMPLIFY | ~70 | ~40 | none |
| Z3-F06 | Test-only public tool-invocation wrappers | DELETE | ~35 | — | none |
| Z3-F07 | Capability negotiation: 4 requested / 6 declared for 1 distinction | KEEP-BUT-STOP-INVESTING | 0 | 0 | — |
| Z3-F08 | Reasoning-echo bound machinery fails a run instead of degrading | SIMPLIFY (low) | ~30 | small | documented invariant |
| Z3-F09 | Full replay reload per transition for one status field | SIMPLIFY | ~20 | — | none |
| Z3-F10 | Guards for states no production caller can reach | SIMPLIFY | ~15 | — | none |
| Z3-F11 | Docs governing the zone: 1 superseded, 2 unbuilt designs | (docs, in Z6) | — | — | — |

### Z4 — Edges

| ID | Title | Disposition | Prod | Test | Loss |
| --- | --- | --- | ---: | ---: | --- |
| Z4-F01 | `intention-test-support` exists for one 70-line TUI test | DELETE | 188 | 274 | none |
| Z4-F02 | Two daemon e2e binaries duplicate a ~250-line harness | MERGE | — | ~250 | none |
| Z4-F03 | `RunStreamClient` has no non-test consumer | SIMPLIFY / KEEP | ~150 | — | none today |
| Z4-F04 | Two complete transport stacks for one daemon | SIMPLIFY / DELETE | ~90–300 | ~100 | none shipped |
| Z4-F05 | Daemon test-support surface in production source | SIMPLIFY | ~150 | — | none |
| Z4-F06 | `inflight` refcount unreachable generality | SIMPLIFY | ~10 | — | none |
| Z4-F07 | Reasoning-cursor set cloned per frame; one watermark suffices | SIMPLIFY | ~15 | — | none |
| Z4-F08 | Three no-op ports; hook registry built per tool call | DELETE | ~50 | — | none |
| Z4-F09 | Platform path layout re-derived in seven places | MERGE | ~60 | ~100 | none |
| Z4-F10 | Live e2e budget is a paper guarantee (checker deleted) | SIMPLIFY | — | — | none |
| Z4-F11 | Composition crate: 1,196 inline test lines, no declared target | SIMPLIFY (policy) | — | — | none |

### Z5 — Tools and periphery

| ID | Title | Disposition | Prod | Loss |
| --- | --- | --- | ---: | --- |
| Z5-1 | Three `[forbidden]` policy keys no checker reads | DELETE | 13 | 0% |
| Z5-2 | Provider-SDK ownership enforced twice, disagreeing | MERGE | 17 | 0% |
| Z5-3 | `quality/timing.py` dead second metrics manifest | DELETE | 54 | 0% |
| Z5-4 | Critical-feature-combination extension point, zero entries | DELETE | 30 | 0% |
| Z5-5 | `quality/harness` M0 marker crate | DELETE | 21 + crate | 0% |
| Z5-6 | No negative proof; four docs claim deleted fixtures | SIMPLIFY | — | 0% |
| Z5-7 | Second unreachable result path in `intention-tools` | DELETE | ~250 | 0% |
| Z5-8 | Hook non-`Continue` surface has no production producer | SIMPLIFY | ~180 | 0% (needs doc edit) |
| Z5-9 | Tool submodules are forwarders; `ToolExecutor` declared-dead | DELETE | ~44 | 0% |
| Z5-10 | `ci-source`/`ci-coverage` aliases no workflow invokes | DELETE | 10 | convenience word |
| Z5-11 | Test targets split by coverage goal, not contract | SIMPLIFY | — | 0% |
| Z5-12 | `resolve_new_file_path` byte-identical alias | DELETE | 5 | 0% |
| Z5-13 | `package_source_roots` unreachable from runner | DELETE | 16 | manual-run flag |
| Z5-14 | `check_deny_policy` restates `deny.toml` as Python | SIMPLIFY | ~35 | review artifact |
| Z5-15 | Docs assert removed or never-existing mechanisms | SIMPLIFY (docs) | — | 0% |
| Z5-16 | Observational CI machinery: 800+ lines that never gate | KEEP-BUT-STOP-INVESTING | 0 | — |
| Z5-17 | `THIRD_PARTY_NOTICES.md` gate for a product with no distribution | KEEP-BUT-STOP-INVESTING | 0 | — |

### Z6 — Architecture documents

| ID | Title | Disposition | Lines | Loss |
| --- | --- | --- | ---: | --- |
| Z6-F01 | Architecture 14: 127 lines about deleted machinery | DELETE | ~127 | ~0% |
| Z6-F02 | Restated facts are stale; no gate checks a claim | SIMPLIFY | ~25 | 0% |
| Z6-F03 | Future mechanisms restated inside shipped documents | SIMPLIFY | ~500 | low |
| Z6-F04 | 121 closed failure codes for zero code; none exist | DELETE | ~280 | 0% |
| Z6-F05 | Roadmap states the same mapping in four places | SIMPLIFY | ~100 | low |
| Z6-F06 | Architecture 29 specifies a reverted surface | SIMPLIFY | ~70 | 0% |
| Z6-F07 | README re-owns cross-domain invariants | SIMPLIFY | ~20 | low |
| Z6-F08 | Architecture 12 restates the CI implementation | SIMPLIFY | ~90 | low |
| Z6-F09 | Removed codec re-narrated in 24 of 32 documents | SIMPLIFY | ~80 | near zero |

### Z7 — Records and root docs

| ID | Title | Disposition | Lines | Loss |
| --- | --- | --- | ---: | --- |
| Z7-01 | 33 accepted future-direction ADRs with zero code | MERGE | ~2,000 | 0% |
| Z7-02 | "Limits by precedent" owned 3×, two copies contradict (5 vs 9) | DELETE stale copy | 75 | 0% |
| Z7-03 | Root `architecture-fitness-audit.md`: 6,157 lines, orphaned, stale | DELETE / move | 6,157 | 0% |
| Z7-04 | Dangling refs to deleted artifacts in 54 documents | SIMPLIFY | ~12 | 0% |
| Z7-05 | Root README claims an unmerged capability; restates 3 tables | SIMPLIFY | 1 | 0% |
| Z7-06 | Closeout baseline `bf40567` not in `main`; boilerplate | SIMPLIFY | ~40 | 0% |
| Z7-07 | Architecture README duplication hub (~150 of 284 lines) | MERGE | ~100 | 0% |
| Z7-08 | ADR 0038: 194 lines recording an executed removal program | SIMPLIFY | ~90 | 0% |
| Z7-09 | M5+ slice status restated in four places | SIMPLIFY | ~30 | 0% |
| Z7-10 | `docs/reference/prime-agent-research/`: 4,153 lines | KEEP-BUT-STOP-INVESTING | 0 | — |
| Z7-11 | "Fake-secret absence tests" names no runnable artifact | SIMPLIFY | ~0 | 0% |
