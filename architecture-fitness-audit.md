# Architecture fitness audit: целесообразность, delayed-action risks, and predicted fate

**Status.** Read-only audit record and working document. It is not an
implementation authorization; the recommendations below are proposals, not
approved changes.

**Baseline.** `main` @ `3291e50` (2026-10-02), working tree clean except the
untracked `pr-44-45-review-report.md` (not read, not touched).

**Method.** Seven independent read-only zones ran in parallel, each with its
own object of analysis (daily operation; data and concurrency; consumer
surfaces; LLM-agency fit; unbuilt platform; shipped code; plan and process).
Agent prompts contained no list of suspected problems, no reviewer examples,
and no mechanism names; every finding below was derived by the zone from the
repository itself. The controller then spot-checked the highest-severity
claims against the cited `file:line` anchors (section 9), deduplicated
cross-zone findings, and assembled this report from the seven full zone
dossiers, which are reproduced as appendices A-G. Nothing in the repository
was modified by the audit; no build, test, or measurement run was performed.

**How to read this report.** Sections 1-10 are the controller synthesis.
Appendices A-G are the complete zone drafts, including method, per-vector
verdicts, all finding cards, keep-lists, predictions, metrics, and stated
uncertainty.

---

## 1. Criteria and rubric

The audit judged *целесообразность*: whether a mechanism pays for itself in
real daily work by one user on one machine, and what happens when the project
leaves the documentation-and-unit-tests stage. Categories used by every zone:

| Category | Meaning |
| --- | --- |
| `LANDMINE` | Will materially degrade the product in real use: identified trigger and blast radius |
| `BUREAUCRACY` | Ceremony that raises manual or cognitive cost without equivalent protection |
| `EXCESSIVE` | A real function delivered at disproportionate cost |
| `DEAD` | No reachable consumer |
| `THEATER` | Enforcement-shaped machinery with no reachable effect |
| `MIXED` / `JUSTIFIED` | Partly real / fully justified (keep-list) |
| `PREDICTION` | Predicted fate of a mechanism (cut / rewrite / survive) with signals |

Every finding carries a lifecycle stage (`shipped` — M0-M5 code; `committed` —
M5+ or M6-M9 approved documentation; `doc-only` — M10-M12 or unactivated
directions), severity (S1 breaks daily use; S2 rewrite or major drag; S3
tolerable tax; S4 minor), a simplest alternative, a disposition, an effort
estimate (S/M/L/XL plus rough days), confidence, and cost of delay.

## 2. Executive summary

The shipped spine (single daemon, typed protocol, durable facts, queue,
recovery semantics, bounded tools, quality gate) is sound and largely survives
scrutiny. The problems concentrate in three layers: **(a) shipped operational
constrictions that break daily work**, **(b) a consumer surface that cannot
yet deliver a product**, and **(c) a plan/documentation layer that gates the
product behind unbuilt work and is not protected against its own factual
drift**. Seven zones produced 110 finding cards; after cross-zone
deduplication, twelve distinct delayed-action mines remain. Four of them are
already in shipped code.

### 2.1. Deduplicated mines

| # | Mine | Zone findings | Stage | Sev | The trigger in real use |
| --- | --- | --- | --- | --- | --- |
| M1 | Hard 30 s `execute` cap kills the process tree and records an unknown external effect | Z1-F02, Z4-F02, Z5-F01, Z6-F01 | shipped | S1 | Any `cargo build`/`cargo test`/install longer than 30 s: `make quick` itself cannot pass through the tool |
| M2 | A failed tool call terminalizes the run and the model never receives the error text | Z4-F01 | shipped | S1 | One malformed tool name or a single tool error destroys the whole turn; the model cannot correct itself |
| M3 | Restart with queued input strands a promoted `Starting` run that nothing admits | Z1-F01, Z2-F03, Z4-F09 | shipped | S1 | Daemon restart while a turn is queued: the session wedges, every later turn queues behind the orphan |
| M4 | No consumer-visible history, no surfaced streaming, no client actions, one binary | Z3-F01, Z3-F02, Z3-F03, Z1-F06 | shipped | S1 | The first real UI (M6): the transcript does not exist as a read model, live text is not delivered, the shared client has no send path |
| M5 | Unbounded session context: no budget, no trimming, no compaction | Z1-F08, Z2-F06, Z4-F05 | shipped | S2 | Long sessions resend all history until the provider rejects the request; then every turn fails the same way |
| M6 | Every commit rebuilds and rewrites full session/run projections and parses all run facts | Z1-F04, Z2-F01 | shipped | S1-S2 | Normal use with reasoning deltas: O(history) work and writes per commit; UI latency and disk growth |
| M7 | Provider round is one non-re-armed 30 s deadline; retry budget is 2 attempts with a fixed 250 ms wait; retry disabled after any durable output | Z6-F02, Z4-F15, Z1-F03 | shipped | S2 | A slow or reasoning-heavy stream fails the run; a mid-stream failure is never retried |
| M8 | M5+ is a hard prerequisite of M6-M9 combined with a "no slice ships half-ready" pre-authorization rule | Z7-F01, Z7-F02, Z5-F02, Z5-F16 | committed | S1 (product) | Every UI attempt must first replay a retrospective package; contracts keep preceding their consumers |
| M9 | Two accepted directions disagree on per-action confirmation: arch 27 requires exact confirmation; ADR 0017/0018 and arch 07 mandate confirmation-free Build Autopilot | Z4-F06 | committed | S1 (spec) | The first implementation of either path contradicts the other accepted document |
| M10 | Mandate uncertainty quarantine (`ExternalEffectUnknown` -> `PausedAwaitingDecision`) has no specified operator-answerable exit | Z1-F13, Z5-F04, Z5-F05 | doc-only | S3 now / S1 if built as written | Any crash with started external work becomes a manual adjudication ritual with a deferred command surface |
| M11 | Verification is modelled as an authority algebra (targets, baselines, verdicts, mutations, gate templates) in a system whose only authority is the user | Z4-F07, Z4-F08, Z5-F06 | doc-only | S2-S3 | The edit -> test -> fix loop pays mandate-grade ceremony on every quality claim |
| M12 | Claim layer is unguarded: no gate checks cross-document numbers; live defects already exist | Z7-F06, Z2-F10, Z1-F12 | shipped docs | S2 | 8-vs-9 required checks (arch 12:44 vs :329), `bf40567` is not an ancestor of `main`, a claimed 4 MiB reasoning bound exists nowhere |

Additional independently found landmines, detailed in the appendices, include:
workspace-wide `grep`/`glob` traversal with no ignore rules and literal-only
matching (Z4-F03/F04); `run_snapshots` written every commit and read by no one
(Z2-F02); readiness hardcoded to `Ready` while the client branches on states
that never occur (Z6-F06); startup-only configuration with no stop/reload
surface (Z1-F05); the agent running without a system prompt or `AGENTS.md`
and without a reachable `ask_user` path (Z1-F07, Z3-F11); failure memory
erased between runs (Z1-F09); 8 of 14 registry slots reserved (Z6-F07); and a
dead tool-result vocabulary and envelope family (Z6-F03/F04).

### 2.2. Convergence (independent discovery)

The strongest reliability signal is that the most consequential mechanisms
were found independently by several zones from different directions:

| Mechanism | Zones that found it independently |
| --- | --- |
| 30 s `execute` deadline | Z1, Z4, Z5, Z6 |
| Restart-promoted run never admitted | Z1, Z2, Z4 |
| Unbounded context / no compaction | Z1, Z2, Z4 |
| Per-commit full-history projection work | Z1, Z2 |
| No history/streaming/client action surface | Z1, Z3 |
| Readiness/lifecycle and configuration constrictions | Z1, Z6 |

### 2.3. Headline numbers

- Shipped: 24 crates, 53,319 Rust lines (28,357 source + 24,962 test), one
  binary (the daemon).
- Documentation corpus: 39,166 lines under `docs/`; architecture 13,549 lines
  of which 7,257 describe unbuilt systems versus 5,886 for shipped v1;
  decisions 7,522; reconciliation registers 3,617; `m4plus_concept.md` 7,731;
  the AGENTS-mandated reading set is 228,123 bytes.
- Per-change cost: PR #44 changed 60 documentation files against 53 code
  files; PR #36 created 43 files of which 29 were deleted within four days;
  one 2,183-line review ledger exists for a PR that was never merged.
- Gate: 8 required checks, 8.9-minute mean CI wall over 40 runs; the local
  full gate is serial and reported at 20-40 minutes.
- Known live claim defects: 8-vs-9 required checks; M5 closure names a
  baseline commit that is not an ancestor of `main`; 62% of registered
  evidence is `Planned`.

### 2.4. What survives (headline)

Single daemon ownership; typed DTO-only JSON-RPC over a local socket; durable
append-only facts with replay; the queue ticket and one-active-run invariant;
`WorkspaceRoot` as an honest addressing anchor; interrupt-not-resume recovery;
two-step cancellation; commit-before-publication; the six bounded tools; the
no-backward-compatibility and single-version policies; limits-by-precedent and
the no-content-scanning ban; the quality gate itself (with one caveat: it has
no model-facing evaluation harness). Section 7 aggregates the full keep-list.

## 3. Zone verdicts

| Zone | Verdict |
| --- | --- |
| Z1 Daily operation | Shipped paths do not survive a normal working day: restart-with-queue wedges (S1), long commands cannot run (S1), failures destroy turns, and there is no operator surface. Keep-list confirms the spine. |
| Z2 Data and concurrency | Correct invariants on top of a cost model that replays full history per commit; one mutex-guarded connection and a read path taking the write lock; dead schema; recovery admission hole. |
| Z3 Consumer surfaces | The data a transcript needs exists durably, but none of it is exposed: no streaming surface, no history read, no client actions, no second binary; M6 cannot be built against today's contract without a bypass. |
| Z4 LLM-agency fit | The formal model (verification, authority, retries, no-resume) is consistently more rigorous than the work it governs; the model's own feedback loop is absent where it matters (tool errors, context budget, search semantics). |
| Z5 Unbuilt platform | Block-by-block: the daily surface is the least planned; the Mandate track is a governance/identity plane for one local user; the bridge is enforcement theatre by the corpus's own admission; most blocks are predicted to be cut or rewritten. |
| Z6 Shipped code | No dead dependencies or invented error codes; a long tail of zero-consumer API (envelope family, event family, pagination, ports, reserved slots) and two time contracts that contradict their own ADR. |
| Z7 Plan and process | The plan layer is the actual blocker: a hard prerequisite gate, a pre-authorization rule the project's own history has already falsified twice, a corpus whose claims no gate checks, and hand-maintained registers. |

## 4. Cross-cutting themes

1. **Irreversibility anxiety applied to ordinary local work.** The strongest
   formal machinery (unknown-effect classification, quarantine, no-resume)
   governs local commands on a single trusted machine. The shipped M5
   behavior (mark `Interrupted`, let the user re-send) is *more operable* than
   the mandated future pause (Z1-F13), while the actually dangerous
   constriction - a 30-second kill switch on ordinary commands - already
   ships.
2. **Contracts ahead of consumers.** The pre-authorization rule produced
   contracts with zero consumers (Slice 1: 3,646 production lines, removed
   within days), and 29 of 43 files created in PR #36 were deleted within
   four days. The pattern continues in the shipped tree (envelope family,
   domain tool-result event family, pagination DTOs, reserved slots,
   adapter stack).
3. **Gates before the product.** M6 is gated behind M5+, the daily-use
   surface is the least-planned block, and there is no client binary. More
   architecture lines describe unbuilt systems than the entire shipped
   implementation.
4. **O(history) bookkeeping at the core.** Full-history projection rebuilds
   per commit, event-log scans where lookups suffice, unbounded context
   resend, one connection behind one mutex. The persistence layer pays the
   full-history price on every interaction.
5. **Ritual where the model needs feedback.** Tool failure terminalizes the
   run instead of teaching the model; verification requires typed gate
   templates instead of running the project check; provider failures are
   opaque codes with a two-attempt budget; the only production hook is a
   no-op. Formality is concentrated where it does not change model outcomes
   and absent where it would.
6. **Paper guarantees versus live reality.** "Progress-bounded" timeouts are
   absolute wall clocks; readiness is hardcoded; a 4 MiB durable bound is
   claimed but absent; required-check counts disagree across one document;
   the M5 closure baseline is not on `main`; most registered evidence is
   planned. No gate checks any of these claims.
7. **The documentation is becoming the product.** Register rows are
   hand-maintained (317 rows), one entity is declared in 27 files and three
   hand-maintained lists, and a mechanism change forces a corpus-wide sweep
   (1.13 documentation files per code file). The corpus's maintenance cost
   now competes with delivery.

## 5. Predicted fate (aggregated, with signals)

Zone predictions converge on the following fate map. Confidence is the
zones'; signals are the observable events that will confirm the prediction
before the cost is paid.

### 5.1. Will be cut

| Prediction | Signal | Confidence |
| --- | --- | --- |
| The M5+ hard-prerequisite clause is cut or rewritten; M6 starts against M5 contracts | A new ADR reordering the roadmap, or an `impl/m6-*` branch appearing while slices 3-5 are incomplete (Z7-P1) | high |
| The pre-authorization rule is replaced by consume-first | The first activating spec declaring fewer contracts than its direction list (Z7-P2) | high |
| Per-call confirmation for ordinary runs; verifier authority algebra; gate templates; read-and-delegate harness classes | First dogfooding of a multi-file change; M10-M12 re-scoping to "run the project check, attach evidence" (Z4 predictions) | high / medium |
| `session.subscribe` as a read duplicate; always-empty replay tails in current form; `WaitingInput` screens until a producer exists | First UI using `session.snapshot` only; first reconnect flow attempt (Z3 predictions) | high / medium-high |
| `run_snapshots`; `sessions.config_revision_id`; snapshot `sequence` columns; the second no-op context build; the PR #24 ledger; `m4plus_concept.md` and `m4.md` leaving the active reading path | First schema-touching change; a "retire the ledger" commit; concept moved under `docs/reference/archive/` (Z2, Z7 predictions) | high / med-high |
| The unbuilt stack as specified, by roughly half or more | The first activating specification after this audit (Z5 prediction) | medium |

### 5.2. Will be rewritten

| Prediction | Signal | Confidence |
| --- | --- | --- |
| The tool layer (timeouts, error feedback, search semantics, read window) | First real repository-wide search; first build over 30 s; first hallucinated tool name (Z4) | high |
| Restart recovery/admission (startup sweep or durable held rows) | Any restart-with-queue report (Z1-P3, Z2, Z4) | high |
| Context management (budget, threshold, compaction) moving into ordinary runs | First provider context-length failure (Z1-P7, Z2, Z4) | high / medium |
| Per-commit snapshot fan-out becomes incremental or scoped | First profiler trace of a long session; first M6 UI stall (Z1-P5, Z2) | high on change |
| Provider round deadline becomes progress-based and re-armed; retry economics become usable | First slow reasoning-model failure in daily use (Z6) | high |
| The consumer subscription API surfaces typed frames plus commands plus reconnect; cumulative snapshot becomes bounded/pageable | An M6 smoke test asserting visible text within ~1 s cannot pass without replay polling (Z3) | high |
| The Mandate aggregate/scheduler, child graph, activity layer, MCP, control plane (partially) | M10 activating spec hedging; deferrals in fixture lists (Z5) | high / medium-high |

### 5.3. Will survive

Single daemon + typed protocol + durable facts + run replay; queue ticket,
one-active-run, atomic terminal-and-promote; `WorkspaceRoot` anchor;
interrupt-not-resume; two-step cancellation; commit-before-publication; the
quality gate (`make quick`/`make verify`, pinned tools, 80% coverage);
no-backward-compatibility and single-version schema; limits-by-precedent and
no-content-scanning; the hello/version gate; structural credential absence;
the six bounded tools; M6-M9 as the real product surface (Z1-P6, Z2, Z4, Z5,
Z6, Z7).

## 6. Prioritized action program

Effort: S <= 0.5 day, M 1-3 days, L 1-2 weeks, XL > 2 weeks (rough; the
repository records no effort data). IDs refer to finding cards in the
appendices. This program is a proposal only.

### P0 - Unblock the product path (strategy and spec decisions)

| Action | Reference | Effort |
| --- | --- | --- |
| Cut or re-scope the M5+ hard prerequisite; let M6 start against M5 contracts with slices running in parallel or later | Z7-F01, Z5-F02 | S (decision), then M6 anyway |
| Replace the pre-authorization "no slice half-ready" rule with a consume-first rule: a contract ships only with its named consumer, or is deleted | Z7-F02, Z7-F05 | S |
| Resolve the confirmation conflict (arch 27 vs ADR 0017/0018 + arch 07) in one policy document | Z4-F06 | S |
| Fix the live claim defects while touching the corpus: 8-vs-9 checks, M5 baseline hash, the non-existent 4 MiB bound | Z7-F06, Z2-F10 | S |

### P1 - Shipped landmines to fix before daily use

| Action | Reference | Effort |
| --- | --- | --- |
| Replace the 30 s `execute` contract: shell-text or command template, progress-based or configurable deadline; reconcile with arch 15's own `ShellCommandTextDto` | Z1-F02, Z4-F02, Z5-F01, Z6-F01 | M |
| Feed tool errors (including malformed calls) back to the model as typed results; reserve run failure for infrastructure faults | Z4-F01 | M |
| Admit durable pending work after restart (startup sweep); keep no-resume for external effects | Z1-F01, Z2-F03, Z4-F09 | M |
| Surface the consumer contract: typed frames or exposed reducer state including text, session/run transcript read, client commands and a client binary | Z3-F01, Z3-F02, Z3-F03, Z1-F06 | L |
| Add a context budget with an honest failure and compaction trigger | Z1-F08, Z2-F06, Z4-F05 | M |
| Make projection maintenance incremental or scoped to the affected run | Z2-F01, Z1-F04 | M |
| Re-arm the provider round deadline on progress; honor `Retry-After`; allow retry after durable partial output | Z6-F02, Z4-F15, Z1-F03 | S-M |
| Make search tools ignore-aware and either regex-capable or honestly named; give `read` a window | Z4-F03, Z4-F04 | S-M |

### P2 - Committed-scope corrections before activation

| Action | Reference | Effort |
| --- | --- | --- |
| Define the reconciliation exit (three choices) before any quarantine activation, or re-scope quarantine to dangerous effects only | Z1-F13, Z5-F04 | S (decision) / L if built |
| Re-scope verification to "run the project check, attach evidence"; drop the authority algebra and gate templates | Z4-F07, Z4-F08, Z5-F06 | S (decision) |
| Re-justify or drop the surviving numeric caps (fork rate limit, harness bounds, kernel cell bound); make the limits doctrine executable | Z5-F12, Z5-F17, Z1-F12 | M |
| Give readiness real states or collapse the enum; add stop/reload lifecycle and configuration visibility | Z6-F06, Z1-F05 | M |
| Deliver the session discovery contract and the instruction channel (`AGENTS.md`, mode, `ask_user`) as part of M6/M5+ slice scope | Z1-F06, Z1-F07, Z3-F11 | L |
| Re-scope the child graph/bridge/harness/MCP blocks per the Z5 verdict table before writing activating specs | Z5-F04-F11, F16-F20 | M (planning) |
| Separate read access from the write lock; revisit the global command gate when concurrency becomes real | Z2-F05 | M |

### P3 - Cleanup and hygiene

| Action | Reference | Effort |
| --- | --- | --- |
| Delete the zero-consumer surfaces (envelope family, domain tool-result event family, pagination DTOs, `LocalToolInvocationPort`, dead schema, no-op dispatch, dead even in tests) | Z6-F03-F17, Z2-F02/F11 | M |
| Adopt the rule: every reserved slot or placeholder gets its consumer named in the next milestone spec or is deleted in the same change | Z6-F05, Z6-F07, Z5-F18 | S |
| Retire or generate the hand-maintained registers; move history out of the active reading path | Z7-F03/F07/F11/F12/F13/F14, Z5-F20 | M |
| Add targeted claim checks (required-check count, baseline ancestry, evidence-status counts) to the documentation gate | Z7-F06, Z7-P10 | S-M |

## 7. Cross-zone keep-list

Mechanisms that multiple zones independently judged worth their cost, and
which any amputation should preserve:

- **Runtime spine**: single daemon ownership; typed DTO-only JSON-RPC 2.0
  over a local socket; exact-version hello; durable append-only facts with
  replay; run cursors with bounded pages and typed resync; one-active-run +
  never-reused queue ticket + atomic terminal-and-promote; two-step
  cancellation; commit-before-publication; interrupt-not-resume recovery;
  immutable config revisions.
- **Policy that deletes work**: no-backward-compatibility and the
  single-version rule (ADR 0038); limits-by-precedent and the
  no-content-scanning ban (ADR 0048); honest trusted-local framing instead
  of fake sandboxes.
- **Tooling and quality**: `WorkspaceRoot` as an addressing anchor; six
  bounded tools; the adapter dependency boundary; fail-closed TOML with
  structural credential absence; the 8-phase hook pipeline (currently
  underused, not wrong); the 80% coverage gate and `make quick`/`make
  verify` split with pinned tools.
- **Safety vocabulary worth keeping while cutting its excess**: unknown
  effect as a distinct class; no automatic resumption of external work;
  durable tool facts; redaction by structure.

## 8. External-lead coverage check

The controller kept a sealed list of externally supplied review leads
(maintained outside every agent prompt; no zone saw it). After the drafts
were final, each lead was matched against independently discovered findings.
**All leads were independently found; no external addendum was required.**
The mapping is recorded here for traceability (paraphrased leads):

| External lead (paraphrased) | Independently discovered as |
| --- | --- |
| Quarantine of uncertain effects makes the agent a manual-approval bureaucrat | Z1-F13 (quarantine has no operator-answerable exit; shipped interrupt path is more operable), Z5-F01, Z5-F04 |
| Independent cursors plus one SQLite writer produce contention/ordering storms under sub-agent fan-out | Z2 section 2.1 inventory, Z2-F01, Z2-F05 (16-child cap), Z2-F03 |
| The M6 frontend cannot render a chat without gluing many entities and streams | Z3-F01, Z3-F02, Z3-F03, Z3-F07, Z1-F06 |
| Formal transactional/verification semantics contradict the probabilistic nature of LLM agents and are useless for output quality | Z4-F01, Z4-F07, Z4-F08, Z5-F05, Z5-F06 |
| Bridge grants are "Kerberos on localhost" | Z5-F09 (enforcement theatre by the corpus's own admission) |
| The continual harness builds an enterprise cron with DST/catch-up semantics | Z5 block inventory and comparative practice (Z5 section 2.7); Z4 section 2.3; Z1-F12 |
| The roadmap is split into a normal track and a parallel Mandate track | Z7-F01, Z7-F03, Z7-F12; Z5-F02 |
| A "second great purge" will amputate much of the remaining theory | All zone prediction sections; aggregated in section 5 |
| The project remains an ontological bureaucracy ("SAP/ministry of justice") after the codec cleanup | Z7-F03, Z7-F07, Z7-F08, Z7-F12, Z7-F13, Z7-F14; Z5-F20 |

## 9. Controller verification notes

Spot-checked by the controller against the cited anchors (independently of
the drafting agents; "verified" means the controller read the cited code or
document location):

| Claim | Result |
| --- | --- |
| 30 s `execute` cap, unconditional, kill + unknown effect | **Verified**: `crates/intention-tools/src/lib.rs:146` (`EXECUTE_TIMEOUT = 30s`), `:393` (sole call site), `:461-477` (deadline kill, `tool_execute_external_effect_unknown`) |
| Tool failure terminalizes the run, no error feedback to the model | **Verified**: `crates/intention-runtime/src/lib.rs:770-780` (Failed fact + `FailedTerminal`; only `Succeeded` appends a tool-result message) |
| Restart-promoted `Starting` run is never admitted | **Verified by code path and two zones**: promotion writes `'starting'` at `intention-storage-sqlite/src/lib.rs:486`; production admission sites are `intention-daemon/src/lib.rs:1180` and `:522-524`; no boot sweep found. The cited restart fixture assertion was not re-read by the controller |
| Per-commit full projections/parse | **Partially verified**: `Self::snapshot` called from multiple write paths (`intention-storage-sqlite/src/lib.rs:520,607,1084`), `snapshot_model_runs` inside it (`:402`); the full parse-cost derivation (Z2 section 2.4) was not re-measured |
| Live streaming/history not consumer-visible | **Verified with nuance**: `receive()` returns only `Option<RunResyncDto>` (`intention-client/src/lib.rs:357-364`); reducer state (including `reasoning_content()`) is reachable via `reducer()` (`:442,667`), so reasoning is accumulated state, not surfaced frames; assistant text still arrives only via replay/status-change snapshots (`intention-daemon/src/lib.rs:616-623`); `SessionProjectionDto` has no messages (`intention-domain/src/lib.rs:413-425`); session tails are always empty (`intention/src/lib.rs:838-859`) |
| One binary, client cannot act | **Partially verified**: only `crates/intention-daemon/src/main.rs` exists; client public methods begin at connection/health/snapshot/subscribe, with no send-turn path observed in the first 70 public methods |
| Provider round timeout is a single non-re-armed deadline | **Verified**: `crates/intention-runtime/src/lib.rs:819-823` (one fused sleep), `:847-857` (`provider_attempt_timed_out`) |
| Readiness hardcoded `Ready` | **Verified**: `crates/intention/src/lib.rs:783-785` |
| Confirmation conflict is real | **Verified**: arch 27 requires exact confirmation for named admissions (`27:215,236,261`); arch 07 and ADR 0017 mandate no per-action confirmation for Build Autopilot (`07:37,40,221-226`; `0017:51`) |
| M5+ hard prerequisite and no-half-ready rule | **Verified**: `docs/intention-relay/architecture/11-implementation-roadmap.md:404-405,441-445,591-597`; `docs/intention-relay/decisions/0035-m5plus-complete-foundation-activation.md:109-119` |
| Corpus counts (PR #44: 60 docs vs 53 code files; arch split 5,886 vs 7,257 lines) | **Verified** by direct counts at the baseline |
| 8-vs-9 required checks | **Verified**: `12-quality-gates-and-makefile.md:44` ("eight") vs `:329` ("nine") |
| M5 closure baseline `bf40567` is not an ancestor of `main` | **Verified**: `git merge-base --is-ancestor bf40567 main` exits 1; the PR #14 squash commit on `main` is `254a029` |
| All other findings | Reported by zones with `file:line` anchors; not independently re-verified by the controller. Treat zone-reported claims as evidence with the station's stated confidence |

## 10. Open questions and limits of this audit

- No runtime measurement was performed (no builds, tests, or profiling):
  absolute latency, memory, and disk figures are static derivations, not
  measurements.
- Effort estimates are coarse and not backed by recorded effort data.
- Provider behavior is modelled from contracts and code, not exercised
  against a live API.
- M10-M12 findings are judgments about documents; an activating
  specification could legitimately change the cost-benefit case for specific
  blocks.
- Post-ADR-0044 re-landing of control-plane work would change the cost of
  delay for the configuration and provider blocks.
- Whether adapters are officially allowed to depend on
  `intention-protocol`/`intention-transport` directly (`quality/architecture.toml`
  permits it; the prose says "sole ingress") is unresolved and affects the
  M6 bypass question.
- The audit did not attempt to falsify its own keep-list at the same depth;
  the keep-list is a judgment of surviving value, not a proof of optimality.

---

The seven zone dossiers follow as appendices. They are reproduced verbatim
from the working drafts, including their own scope, method, metrics, and
uncertainty sections.


---

# Appendix A — Zone 1: daily operation and operator burden

- Baseline: `main` @ `3291e50`, working tree clean except untracked audit drafts and reports
  (`git status --short` shows only `?? audit-drafts/` and `?? pr-44-45-review-report.md`).
- Audit type: independent first-principles review of one architecture zone, shipped code and
  accepted documentation alike, judged by fitness for real-world daily use.
- Constraint honored: read-only. No tracked file was modified; no build, test, or cargo command
  was executed beyond offline metadata-level inspection. The only file written is this draft.
- All `file:line` citations resolve at the baseline commit above. Numbers are derived in place;
  measurements are static (no runtime profiling was possible under the read-only constraint) and
  are labeled as such.

---

## 1. Scope and method

### 1.1 Zone definition

This zone covers the complete user-facing lifecycle of a work request across shipped code and
future documents:

1. how a request becomes a session, turn, and run;
2. how a run progresses (streaming, tool loop, completion);
3. how failures, interruptions, cancellations, questions, and retries are handled;
4. what the human must do and must see at each step;
5. how the operator experience behaves under realistic adverse conditions — unstable network or
   provider, interrupted processes, resource pressure (disk, CPU, time), restarts, concurrent
   activity.

### 1.2 Sources read

Shipped code: `crates/intention-runtime` (1,414 lines), `crates/intention-application` (1,629),
`crates/intention-daemon` (2,151 + 32), `crates/intention-client` (794),
`crates/intention-protocol` (1,919 + 849), `crates/intention-domain` (1,770 + 1,005),
`crates/intention` composition (2,247), `crates/intention-storage-sqlite` (2,720),
`crates/intention-tools` (2,254), `crates/intention-config` (1,093),
`crates/intention-transport` (1,662), `crates/intention-model` (1,087), the two provider drivers
(901 + 1,658), `crates/intention-hooks` (754), `crates/intention-workspace` (78),
`crates/intention-tui` (46), plus the daemon integration tests
(`facade_e2e.rs`, `m4_streaming_foundation.rs`, `m5_tool_loop_wiring.rs`, `real_api_e2e.rs`).

Accepted documentation: architecture 03, 04, 05, 07, 10 (referenced), 11 (roadmap), 13, 14, 15,
16, 21, 24, 26, 27, 28, 29, 30, the M5+ slice plan, `production-ceiling-removal.md` and the
cited ADRs (notably 0035, 0038, 0043, 0044, 0045, 0047, 0048, 0049), plus
`docs/intention-relay/reconciliation/pr24-code-review-ledger.md` used only as prior-art context,
never as a substitute for my own code reading.

### 1.3 Method

For each lifecycle stage and adverse event I wrote an explicit end-to-end walkthrough, named the
exact mechanism with `file:line`, and counted what the human must issue or wait for. I then
tested each mechanism against a "simplest sufficient world" for one local user, and recorded what
survives in the keep-list. Every finding carries the mandated card: stage, type, trigger,
mechanism, blast radius, simplest alternative, disposition, effort, confidence, severity, and cost
of delay.

Stage labels used below follow the audit rubric:

- `shipped` = M0–M5 code on `main`;
- `committed` = M5+ slices or M6–M9 approved documentation (not implemented);
- `doc-only` = M10–M12 and later accepted directions.

### 1.4 What "daily use" means at this baseline (important framing)

There is exactly one binary in the workspace: `intention-daemon`
(`crates/intention-daemon/src/main.rs:1-32`). There is no client binary. `intention-tui` is a
46-line library that can connect and subscribe and nothing else
(`crates/intention-tui/src/lib.rs:1-46`). The README states this directly: "There is no
interactive client or UI binary yet; today the daemon is driven through the shared
`intention-client` crate" (`README.md`, "Running the daemon today"). Consequently most daily-use
claims in this draft are counterfactual by construction: they describe what happens when the
shipped backend is driven as if a product existed. That gap is itself finding Z1-F06, and it
bounds the severity of everything else: nothing in this zone can break a user who does not yet
have a client.

---

## 2. Walkthroughs, vectors, and verdicts

### 2.1 Walkthrough A — "one turn" today, without a product

Goal: one user question reaches a model and the answer comes back.

| # | Step | Mechanism | Human action |
| --- | --- | --- | --- |
| 1 | Write `~/.config/intention-relay/config.toml` with a real key | `crates/intention-config/src/lib.rs:154-186` (path), `:360-421` (parse) | yes, 1 file edit |
| 2 | Make it owner-only readable or startup fails | `crates/intention-config/src/lib.rs:771-790` (`unsafe_config_permissions`) | yes, `chmod 600` |
| 3 | Build the workspace (only path to the daemon binary) | `crates/intention-daemon/src/main.rs:12-31` | yes, `cargo build` |
| 4 | Write Rust code that boots a client and launches the daemon | `crates/intention-client/src/lib.rs:130-160` (bootstrap), `:48-80` (process launcher) | yes, write a program |
| 5 | Create the session (project, session, workspace ids, root, mode) | `crates/intention-protocol/src/lib.rs:613-627`; `crates/intention-application/src/lib.rs:814-841` | yes, 1 call |
| 6 | Send the turn (turn id is caller-chosen; run id is derived from it) | `crates/intention/src/lib.rs:900-925` (`RunId::parse(turn_id)`) | yes, 1 call |
| 7 | Subscribe to the run and reduce frames to text | `crates/intention-client/src/lib.rs:306-420` | yes, code |
| 8 | Stop if needed | `crates/intention-protocol/src/lib.rs:613-627`; `crates/intention-daemon/src/lib.rs:282-292` | 1 call |

Minimum interventions for the first turn, counting distinct human operations: 2 file/system steps
(config + chmod) and one Rust program (≈100+ lines using the client). A shell-only operator could
hand-write JSON-RPC lines (`hello` first; client and daemon negotiation at
`crates/intention-transport/src/lib.rs:350` and `:528`), but must reproduce the typed DTO shapes
exactly; feasible, unowned,
unshipped. Confidence in this walkthrough: high (code paths verified); the exact number of lines a
client programmer writes is not measured.

### 2.2 Walkthrough B — normal daily turn (target product, M6 UI assumed)

Once a UI exists, per turn: 1 user action (send), 0 confirmations (Build has no per-action
confirmation by design — architecture 05, "Autopilot and Mandate tool boundary"), N automatic
tool calls, 1 wait for the terminal state, 1 read of the answer. The operator-visible costs are
the waits and the failure paths, not confirmations.

### 2.3 Walkthrough C — crash/reboot with queued input (adverse, shipped)

Preconditions: run `R1` active, queued turns `T2`, `T3`.

| # | What happens | Mechanism | Human action |
| --- | --- | --- | --- |
| 1 | Daemon dies (crash, reboot, `kill`) | no OS signal handling or graceful shutdown path (`crates/intention-daemon/src/main.rs:12-31`, `crates/intention-daemon/src/lib.rs:982-997`) | none |
| 2 | Startup recovery marks every unfinished run `Interrupted` | `crates/intention/src/lib.rs:937-943` → `crates/intention-storage-sqlite/src/lib.rs:1337-1371` | none |
| 3 | The terminal `Interrupted` transition atomically promotes `T2` to a durable `Starting` run | `crates/intention-storage-sqlite/src/lib.rs:976-984` → `:461-499`, insert at `:486` | none |
| 4 | Nothing admits `T2`. The daemon has no startup scan and no recovery-held state | `crates/intention-daemon/src/lib.rs:982-1012` (`run`, `serve_async_listener`); only admission triggers are a fresh accepted turn (`:1151-1185`) and a terminal transition (`:518-529`) | none |
| 5 | User sends a new turn `T4` | `crates/intention-application/src/lib.rs:843-905` → storage sees a non-terminal run | yes, 1 send |
| 6 | `T4` is queued behind the stranded `Starting` run and never executes | `crates/intention-storage-sqlite/src/lib.rs:792-799` (active-run check), `:850-870` (queue insert) | none |
| 7 | Session is wedged; only escape is `run.stop` on the stranded run | `crates/intention-daemon/src/lib.rs:282-312`, `:314-356`; stop terminalizes and reschedules the successor through `:518-529` | yes, 1 stop per stranded run, requires a client |

Counts: 1 crash loses 0 durable data but blocks 2 turns at step 4; recovering requires 1 explicit
stop; each additional restart advances the queue by exactly one turn and strands the next one
(recovery at step 2 always includes the previously stranded `Starting` run because it is
non-terminal). Interventions without a client: impossible — the daemon has no session list, no
run list, and no operator surface. Confidence: high; every step is a straight-line reading of
storage and daemon code. Test evidence confirms the gap rather than covering it: the restart
fixture asserts the promoted successor stays `Starting` and the restarted driver executes nothing
(`crates/intention-daemon/tests/m4_streaming_foundation.rs:784-863`, assertions `:855-863`), and
the only code that can admit such a run is the test-support-gated seam `admit_starting_run`
(`crates/intention-daemon/src/lib.rs:1402-1406`), used only by tests
(`crates/intention-daemon/tests/m4_streaming_foundation.rs:989`). The only production reader of
`current_starting_run_for_daemon` outside tests is the daemon's `on_terminal`
(`crates/intention-daemon/src/lib.rs:522`).

### 2.4 Walkthrough D — long command (adverse, shipped)

1. User asks the agent to run the test suite. Model emits `execute` with `program` + `args`
   (`crates/intention-tools/src/lib.rs:1409-1412`).
2. Hard 30-second cap applies (`crates/intention-tools/src/lib.rs:146`, applied at the execution
   call `:393`).
3. Timeout returns `tool_execute_external_effect_unknown` (`:46`, `:83`, `:477`, `:508`, `:524`),
   and the runtime turns a failed tool outcome into a terminal run failure
   (`crates/intention-runtime/src/lib.rs:770-779`, the terminal `Failed` append for a failed tool
   outcome).
4. The model never sees the partial output or the exit status; the run is dead.
5. Recovery requires the user to re-send and pick up the pieces; probe commands via pipes or
   `&&` are also impossible because `ExecuteInput` is program-plus-args, not a shell string.

Counts: 1 re-send + full prompt re-billing per long command. `make quick`/`make verify` — the
commands this repository's own `AGENTS.md` orders agents to run — exceed 30 s.

### 2.5 Walkthrough E — key rotation / config change (adverse, shipped)

1. Edit `config.toml`. The daemon keeps the startup snapshot (`crates/intention/src/lib.rs:399-410`,
   `:958-977`); nothing re-reads the file at runtime; the reverted Slice 2 would have added reload
   (ADR 0044, `docs/intention-relay/decisions/0044-revert-of-m5plus-slice2-control-plane.md`).
2. No visible signal that configuration changed; `daemon.health` exposes readiness only
   (`crates/intention/src/lib.rs:783-784`).
3. There is no stop/shutdown method in the protocol (`crates/intention-protocol/src/lib.rs:613-634`).
   The operator must find and kill the process.
4. Killing interrupts any in-flight run; a queued successor strands (Walkthrough C).

Counts: 1 external process kill + relaunch per config change; work loss if a run was active.
Architecture 03 lists "explicit daemon stop command and future idle shutdown policy" as an open
decision, so this is a known, deliberate interim state.

### 2.6 Walkthrough F — disk pressure (adverse, shipped)

1. If a commit fails (disk full, DB unopenable), the executor error path tries to terminalize the
   run as `Failed` (`crates/intention-daemon/src/lib.rs:235-265`).
2. If that write also fails, a terminalizer task retries with one immediate attempt, then loops
   at a fixed 25 ms with no backoff, no cap, and no error surface
   (`crates/intention-daemon/src/lib.rs:47`, `:314-356`, pacing at `:349-353`).
3. The run never becomes terminal, so the session keeps queueing all new input; the operator sees
   nothing in the product.
4. At startup, platform state directory or DB failure yields a typed failure and process exit
   (`crates/intention/src/lib.rs:1002-1046`); the client reports unavailability after a 3-second
   bootstrap window (`crates/intention-client/src/lib.rs:30-31`).

Counts: silent retry loop at up to ~40 attempts/second per affected run (fixed 25 ms pacing); the
operator must diagnose from outside the product.

### 2.7 Walkthrough G — resuming work after a week (adverse, shipped + committed gap)

1. There is no session-list or project-list command or query anywhere in the protocol
   (`crates/intention-protocol/src/lib.rs:613-634`) and no storage read that enumerates sessions
   (`crates/intention-storage/src/lib.rs:809-971`; the trait is session-scoped — create, accept,
   remove, transition, load snapshot, accept revision — with no enumeration). Grep across
   `docs/intention-relay/` for `session.list`, `ListSessions`, or "list sessions" returns nothing.
2. Therefore the M6 deliverable "minimal Svelte UI to create/open a session"
   (architecture 11, "Milestone 6: Tauri bridge and primary desktop UI") has no discovery
   contract to consume, and the first
   daily-use screen — "pick up yesterday's session" — is unowned.
3. Even with a known session id, replay is bounded: `session.subscribe` always returns the current
   projection plus an empty tail (`crates/intention/src/lib.rs:811-860`; both branches construct
   `SessionEventTailBatchDto::new(..., Vec::new())`), so historical session events are not
   retrievable over the protocol; run-scoped subscriptions are the only replay path
   (`crates/intention-daemon/src/lib.rs:673-780`).
4. A new run re-sends the whole session history (see Z1-F08), so cost grows with session age.

### 2.8 Vector table → verdicts

| Vector | Verdict | Evidence anchor |
| --- | --- | --- |
| Request → session/turn/run | Coherent, minimal, but only reachable from library code | `intention-protocol` 613-634; `intention-application` 843-905 |
| Run progress and streaming | Sound replay/live separation; replay-only session channel is a gap | `intention-daemon` 673-780; `intention-client` 306-420 |
| Queue semantics | Correct at rest, landmine after restart (F01) | `intention-storage-sqlite` 461-499, 792-870 |
| Cancellation | Correct two-step design; terminalizer retry is unbounded (F10) | `intention-runtime` 95-134, 1200-1234; `intention-daemon` 314-356 |
| Failure and retry | Under-provisioned for real providers (F03, F09) | `intention-config` 616-617; `intention-runtime` 179, 590-600 |
| Questions / clarification | Impossible today; `WaitingInput` and `ask_user` are unreachable (F07, F11) | `intention-domain` 43, 84-92; `intention-tools` 1142-1150 |
| Config lifecycle | Startup-only, no reload, no stop (F05) | `intention` 399-410, 958-977 |
| Restart/recovery | Honest no-resume policy with a broken queue handoff (F01) | `intention-storage-sqlite` 1337-1371; `intention-daemon` 982-1012 |
| Resource pressure | Unbounded snapshot writes + silent terminalizer spin (F04, F10) | `intention-storage-sqlite` 358-457, 510-521; `intention-daemon` 314-356 |
| Concurrent activity | Subscriber isolation and slow-peer resync are sound (keep-list) | `intention-daemon` 673-780 |
| Operator visibility | No session/run/cost surface at all (F06) | protocol surface; `intention-tui` 1-46 |
| Context growth | Unbounded, unowned for ordinary runs (F08) | `intention-storage-sqlite` 1185-1250; architecture 21 (Goals/context track, doc-only) |
| Instructions to the model | None are sent; optional field always `None` (F07) | `intention-application` 1395-1418; `intention-model` 587-623 |
| Future Mandate/harness operation | Heavy manual adjudication models, unbuilt (F12, F13) | architecture 13, 16, 26, 27 |

### 2.9 Answers to the five core questions (compact)

1. **First obstacle to daily use**: there is no client at all (F06). Once a client exists, the
   first *functional* obstacles are the 30-second `execute` ceiling (F02), the one-shot retry
   policy (F03), and the restarted-queue wedge (F01).
2. **Full trace**: today one turn requires a config file, a chmod, a build, and a custom Rust
   driver (Walkthrough A). In the target product: 1 send + waits per turn; adverse events add 1
   re-send per provider failure, 1 stop per stranded run, 1 external kill per config change,
   and 1 hand-run of any command longer than 30 s.
3. **Simplest sufficient world**: a single local agent process (or daemon) with sessions, turns,
   streaming, six tools, cancellation, crash-interrupt, config reload, a session list, a system
   prompt plus `AGENTS.md`, a context budget, and a configurable/background long-command path.
   The two-step cancellation machine, per-commit full snapshots, the fixed 14-slot registry
   scaffold, `WaitingInput`, and the entire Mandate/harness/MCP/kernel/bridge document stack do
   not survive that test at v1 scope. (Compaction for long sessions does survive — as an
   ordinary-run feature, not as M10–M12 Mandate machinery.)
4. **Comparative practice**: successful single-user coding agents are single-process CLIs with
   project-instruction files and per-command permission prompts rather than a session/broker
   daemon with a protocol (`https://aider.chat/docs/`,
   `https://code.claude.com/docs/en/overview`, hooks: `https://code.claude.com/docs/en/hooks`).
   Local language servers show that one local process per client is a normal, small architecture
   when the lifecycle is client-managed (`https://microsoft.github.io/language-server-protocol/`).
   JSON-RPC 2.0 for a local endpoint is standard practice (`https://www.jsonrpc.org/specification`),
   and SQLite WAL is the standard local durability choice (`https://sqlite.org/wal.html`).
   Interactive agents let long commands run or time out configurable — general practice.
5. **Future amputation**: the session-subscribe tail contract, `WaitingInput`, the startup-only
   config path, the fixed 30-second `execute` cap, and the absolute per-round provider timeout
   will be cut or rewritten; the single-daemon + typed protocol + durable-fact spine will survive.
   Signals and confidence in section 5.

---

## 3. Findings (ranked cards)

Severity: S1 breaks daily use / S2 rewrite or major drag / S3 tolerable tax / S4 minor.

Ranking is by (severity × probability of being hit in daily use).

---

### Z1-F01 — "Recovery-promoted queued turn is orphaned; the session wedges"

- **ID / severity**: Z1-F01, **S1**
- **Stage**: `shipped` (M3/M4/M5)
- **Type**: LANDMINE (trigger + blast radius below)
- **Trigger**: any unclean daemon stop (crash, reboot, `kill`, terminal close) while one run is
  active and at least one turn is queued. This is the normal state for a user who types the next
  question while the agent is still working.
- **Mechanism**: recovery transitions unfinished runs to `Interrupted`
  (`crates/intention-storage-sqlite/src/lib.rs:1337-1371`); every terminal transition atomically
  promotes the oldest queued turn into a durable `Starting` run
  (`:976-984`, promotion at `:461-499`, insert `:486`); the daemon has no startup admission sweep
  and no held-run state (`crates/intention-daemon/src/lib.rs:982-1012`; admission only from an
  accepted turn at `:1151-1185` or a terminal transition at `:518-529`). A new turn then queues
  behind the stranded active run (`crates/intention-storage-sqlite/src/lib.rs:792-799`, `:850-870`).
- **Blast radius**: UX and human-days. One queued turn is silently skipped per restart; the
  session stops accepting new work; each additional restart advances the queue by exactly one
  turn and strands the next (verified by reading the recovery selection, which includes the
  stranded `Starting` run on the next pass). With no client, there is no way to see or clear it.
- **Simplest alternative**: on `open_platform`/`run`, after recovery, schedule every session's
  current non-terminal `Starting` run exactly once (the daemon already has `schedule_if_starting`
  and `current_starting_run_for_daemon` for exactly this shape); or persist a held row and admit
  it on first client action. The reverted Slice 2 chose the held-row variant (ADR 0037, removed by
  ADR 0044), which leaves the plain path exposed.
- **Disposition**: **fix (simplify)**. Add the startup admission sweep (small) and a regression
  fixture that recovers a queued successor without the test-only `admit_starting_run` seam
  (`crates/intention-daemon/src/lib.rs:1404`).
- **Effort**: S (1–2 days) for the sweep plus fixture; M (3–5 days) if the team chooses durable
  hold + explicit admission instead.
- **Confidence**: high. Every link in the chain was read directly; the failure does not depend on
  timing races.
- **Cost of delay**: the first real user who queues a second question and then reboots loses work
  and cannot recover without developer help. Because the fix is tiny and the failure is silent,
  delay is unusually expensive.

---

### Z1-F02 — "`execute` cannot run real builds or tests: hard 30-second cap, no shell"

- **ID / severity**: Z1-F02, **S1**
- **Stage**: `shipped`
- **Type**: LANDMINE
- **Trigger**: any command taking longer than 30 seconds — `cargo test`, `cargo build`,
  `npm install`, `pytest`, or this repository's own `make quick` / `make verify`, which
  `AGENTS.md` explicitly orders agents to run.
- **Mechanism**: `EXECUTE_TIMEOUT = 30 s` (`crates/intention-tools/src/lib.rs:146`); on expiry the
  tool returns `tool_execute_external_effect_unknown` (`:477`, `:508`, `:524`) after killing the
  process tree (`:543-546`); the runtime maps a failed tool outcome to a terminal run failure
  (`crates/intention-runtime/src/lib.rs:770-779`). There is no timeout parameter in
  `ExecuteInput` (`crates/intention-tools/src/lib.rs:1409-1412`) and no configuration key for it
  (`crates/intention-config/src/lib.rs:693-710`).
- **Blast radius**: the core work loop of a coding agent. Commands that cannot complete in 30 s
  fail the run, discard the output, and force the operator to run them manually. The absence of
  shell semantics (program plus argv only) additionally rules out pipelines, redirects,
  environment prefixes, and `&&`, so the most common developer one-liners are inexpressible.
- **Simplest alternative**: make the bound a typed, precedent-backed input (or a config value with
  a documented precedent, per ADR 0048) and stream output; alternatively run long commands as
  detached processes with a poll tool. Architecture 15 already specifies a future
  `ShellCommandTextDto` and descriptor versioned shell semantics — that is the rewrite.
- **Disposition**: **re-scope** the 30-second cap now (raise or make configurable), **simplify**
  the input contract toward the already-documented shell descriptor. Do not delete the timeout.
- **Effort**: S (config + plumbing) to M (shell descriptor with tests, ~5–10 days).
- **Confidence**: high on the mechanism; high on the impact for coding workflows.
- **Cost of delay**: every real project's first agent-driven test run fails; users learn to stop
  asking the agent to verify its own work, which erodes the product's value proposition.

---

### Z1-F03 — "Retry economics guarantee mid-stream failure loss"

- **ID / severity**: Z1-F03, **S2**
- **Stage**: `shipped`
- **Type**: MIXED (honest budget logic, wrong constants for reality)
- **Trigger**: provider 429/5xx, connection drop, or a slow first token; especially with reasoning
  models that stream for more than 30 seconds before any answer text.
- **Mechanism**: defaults are `attempt_timeout_seconds = 30`, `max_attempts = 2`
  (`crates/intention-config/src/lib.rs:616-617`). A retry happens only if the failure is
  classified delayed, no durable output exists, and attempts remain
  (`crates/intention-runtime/src/lib.rs:590-600`). Any durable text, reasoning, or usage fact
  permanently disables retry. The wait is a fixed 250 ms (`:179`, `:1018-1046`); `Retry-After` is
  never honored. The 30-second deadline is absolute per provider round (`:805-870`), so a
  reasoning-heavy or slow model is killed mid-stream, after which retry is impossible because the
  reasoning deltas are already durable (`:943`). The policy is not merely a default: config
  validation caps `attempt_timeout_seconds` at 1..=60 and `max_attempts` at 1..=2
  (`crates/intention-config/src/lib.rs:618-628`), so an operator cannot trade money for
  resilience without a code change.
- **Blast radius**: money and human-days. Every failed mid-stream attempt costs the full prompt
  again on re-send, and the user repeats the request by hand. The failure is not rare: reasoning
  deltas make "durable output before timeout" the common case.
- **Simplest alternative**: replace the absolute per-round deadline with the progress deadline the
  project already specifies (`model_stream_progress_timeout_v1`, 60 s between non-empty deltas,
  architecture 15, "Model progress deadline") and allow bounded retries with backoff before the
  first irreversible fact; honor `Retry-After` when the provider supplies it.
- **Disposition**: **rewrite** the retry policy; keep the "no retry after durable output" rule.
- **Effort**: M (3–5 days with deterministic clock fixtures; the runtime already has a time port).
- **Confidence**: high on mechanism; medium on the exact field failure rate (no production
  telemetry exists; this is a reasoned estimate from the constants and stream ordering).
- **Cost of delay**: users learn the agent "dies on hard tasks", which is precisely when its value
  is highest.

---

### Z1-F04 — "Every commit rewrites full projections; reasoning deltas commit one-per-chunk"

- **ID / severity**: Z1-F04, **S2**
- **Stage**: `shipped`
- **Type**: EXCESSIVE (real function, disproportionate cost)
- **Trigger**: any long model response, and especially reasoning models; a long session.
- **Mechanism**: every state-changing commit runs `finish` → `project` → `snapshot`
  (`crates/intention-storage-sqlite/src/lib.rs:510-521`). `snapshot` upserts one session snapshot
  and one run snapshot per run (`:358-405`) and then rebuilds every run's model snapshot
  (`:406-457`), where `model_projection` re-reads and JSON-parses **every fact of the run** and
  writes the accumulated assistant content into the snapshot row (`:1729-1781`). Assistant text is
  appended per 4 KiB (`crates/intention-runtime/src/lib.rs:178`, `:1238-1262`), but reasoning is
  appended once per provider delta (`:943`), i.e. one full-projection transaction per streamed
  reasoning chunk. All writes serialize on one SQLite connection behind one mutex
  (`crates/intention-storage-sqlite/src/lib.rs:139`, `:167-169`).
- **Blast radius**: latency, IO/SSD wear, DB growth, and contention: command dispatch and tool
  evidence writes queue behind the streaming writer. Cost per run is O(facts × content) — with N
  commits and a snapshot that re-parses up to N facts each, the write-side work is quadratic in
  response size; reasoning deltas multiply N by one to two orders of magnitude relative to text.
- **Simplest alternative**: batch reasoning deltas (the text path already batches at 4 KiB),
  maintain the model projection incrementally instead of re-deriving it from all facts on every
  commit, and write the frozen run snapshot on state transitions rather than on every content
  fact.
- **Disposition**: **simplify** (keep durability and replay; change the write shape).
- **Effort**: M (4–8 days with fault-injection fixtures intact).
- **Confidence**: high on the mechanism; medium on the absolute magnitude (no profiling was
  permitted; the derivation is structural, not measured).
- **Cost of delay**: reasoning-model daily use will feel slow and hot, and the DB grows without
  any retention policy (architecture 04, "compaction/retention policy remains future work").

---

### Z1-F05 — "Configuration is startup-only, with no reload, no stop, no visibility"

- **ID / severity**: Z1-F05, **S2**
- **Stage**: `shipped` (fix direction is `committed`; Slice 2 was reverted by ADR 0044)
- **Type**: MIXED (deliberate interim state with a heavy daily tax)
- **Trigger**: key rotation, model switch, endpoint change, or fixing a typo; also first-run setup.
- **Mechanism**: config is read once in `open_platform`
  (`crates/intention/src/lib.rs:399-410`, loader `:958-977`); nothing re-reads it. The protocol has
  no stop/shutdown/restart method (`crates/intention-protocol/src/lib.rs:613-634`); the only
  `shutdown` in the daemon crate belongs to a test-only lifecycle
  (`crates/intention-daemon/src/lib.rs:1366`, `:1483`). Health exposes
  readiness only (`crates/intention/src/lib.rs:783-784`). On Unix the file must be mode-safe or
  startup fails closed with `unsafe_config_permissions`
  (`crates/intention-config/src/lib.rs:771-790`).
- **Blast radius**: every configuration change requires an out-of-band process kill and relaunch;
  in-flight work is interrupted and queued work strands (Z1-F01). Nothing tells the operator that
  a config edit had no effect. The chmod requirement is a first-run cliff whose error message is
  safe but does not say how to fix it.
- **Simplest alternative**: a typed `daemon.shutdown` (or `daemon.reload`) command; expose the
  active `ConfigRevisionId` in health; auto-set 0600 on a file the daemon owns, or report the
  required mode in the error detail. Reload is larger and can stay deferred, but stop/restart and
  visibility are cheap.
- **Disposition**: **re-scope**: add stop/restart plus config-revision visibility now; keep full
  controlled reload deferred with the reverted control-plane direction (ADR 0037, reverted by
  ADR 0044).
- **Effort**: S–M (2–4 days).
- **Confidence**: high.
- **Cost of delay**: the first credential rotation forces the user to hunt a background process;
  each rotation risks the F01 wedge.

---

### Z1-F06 — "There is no operator surface, and the planned UI has no session discovery contract"

- **ID / severity**: Z1-F06, **S2**
- **Stage**: `shipped` (UI gap); `committed` (M6 deliverable has an unowned dependency)
- **Type**: MIXED (accepted milestone boundary, but the discovery gap is unowned, not deferred)
- **Trigger**: first day of use; every "which session was I in?", "what is queued?", "why did it
  fail?", "what did that cost?" question.
- **Mechanism**: one binary (`crates/intention-daemon/src/main.rs:1-32`); TUI is a connect/subscribe
  library (`crates/intention-tui/src/lib.rs:1-46`); protocol surface is 4 commands and 2 queries
  (`crates/intention-protocol/src/lib.rs:613-634`); the storage trait is session-scoped with no
  enumeration (`crates/intention-storage/src/lib.rs:809-971`); usage facts are appended
  (`crates/intention-runtime/src/lib.rs:956-961`) and projected per run inside storage
  (`crates/intention-storage-sqlite/src/lib.rs:1745-1776`), but that projection appears in no
  protocol response (no `usage` or model-run projection reference exists in
  `crates/intention-protocol/src/lib.rs`) and there is no cross-run aggregation;
  no `session.list`/`ListSessions` exists anywhere under `docs/intention-relay/`.
- **Blast radius**: human-days of external bookkeeping (notes of session ids), no failure
  diagnosis without a run subscription, no cost awareness. For M6, "create/open a session" cannot
  be delivered without inventing a protocol method and a storage query that no document owns.
- **Simplest alternative**: define one bounded `session.list` query (project/session summary with
  last activity and active-run status) and one run-summary query (status, failure code, usage);
  the DTO/type crate already has pagination primitives
  (`crates/intention-types/src/lib.rs:208`, `:244-247`).
- **Disposition**: **re-scope**: add discovery to the M6 activation contract explicitly.
- **Effort**: S–M (2–4 days for query + storage read; UI is M6 scope).
- **Confidence**: high that no such method exists; medium on whether an M6 spec would quietly add
  it (nobody owns it today).
- **Cost of delay**: the primary UI milestone inherits an undefined dependency, which typically
  becomes an ad-hoc protocol extension without DTO-policy review.

---

### Z1-F07 — "The agent runs blind and cannot ask: no system prompt, no AGENTS.md, no mode, no ask_user"

- **ID / severity**: Z1-F07, **S2**
- **Stage**: `shipped` (instruction channel is `committed` for M5+ slice 5; `ask_user` is
  `doc-only` for M10–M12)
- **Type**: LANDMINE (unusable-by-default behavior) + DEAD (`WaitingInput`)
- **Trigger**: the first real coding task: the model receives only the user's message and tool
  schemas, cannot ask a clarifying question, and cannot be told it is in Plan mode.
- **Mechanism**: `schedule_from_context` builds `ModelRequestDto::new(run_id, model, messages,
  None, None)` — `system_context` is always `None` (`crates/intention-application/src/lib.rs:1395-1418`;
  field semantics `crates/intention-model/src/lib.rs:532-560`, validation `:587-623`). No
  `AGENTS.md` reader exists (architecture 30 is documentation-only; slice 5 not implemented).
  `RunModeDto::Plan` is stored but inert: `intention-plans` is a compile-only skeleton
  (`crates/intention-plans/src/lib.rs:1-6`), no mode hook is registered
  (`crates/intention/src/lib.rs:209-236` registers one no-op observer), and both modes advertise
  all six tools (architecture 07, "Mode does not currently filter the tool definitions").
  `WaitingInput` exists in the state machine but has no producer
  (`crates/intention-domain/src/lib.rs:43`, `:84-92`), and `ask_user` is a reserved slot with no
  schema or executor (`crates/intention-tools/src/lib.rs:1142-1150`, `model_visible_descriptors`
  at `:1245`).
- **Blast radius**: the model guesses instead of asking; the operator must pre-load every
  convention into each prompt; Plan-mode intent is silently a label; clarification costs a stop +
  re-send instead of one question. Instruction text is the cheapest, highest-leverage daily-use
  feature in a coding agent.
- **Simplest alternative**: send a minimal daemon-owned instruction projection now (identity +
  tool-use guidance + `AGENTS.md` content when present), and add `ask_user` as a normal
  long-running tool rather than a new run state. Architecture 30 and 15 already specify both; a
  reduced first version is small.
- **Disposition**: **defer** the full projection contract, **re-scope** a minimal system prompt
  and `ask_user` earlier; **remove** `WaitingInput` from the live surface until a producer exists.
- **Effort**: S–M for the prompt (1–3 days); M for `ask_user` (3–6 days).
- **Confidence**: high (fields, call sites, and registry entries read directly).
- **Cost of delay**: every session pays the cost of a context-free model; agents look "dumber"
  than they are, and users over-specify prompts.

---

### Z1-F08 — "Ordinary sessions have no context budget and no compaction; history is resent forever"

- **ID / severity**: Z1-F08, **S2**
- **Stage**: `shipped` (mechanism) + `doc-only` (any remedy; architecture 21 is doc-only)
- **Type**: LANDMINE (delayed: fires after N turns)
- **Trigger**: a long-lived session (tens of turns, large files, tool output) or a provider with a
  modest context window.
- **Mechanism**: `load_starting_run_model_context` assembles every prior user turn plus the
  completed assistant content of every prior run, in order (`crates/intention-storage-sqlite/src/lib.rs:1185-1250`).
  No request-side trimming, token budgeting, or summarization exists in the runtime or drivers:
  provider usage tokens are recorded after the fact for reporting
  (`crates/intention-provider-openrouter/src/lib.rs:128-134`) and never bound the next request.
  Compaction is specified only for the doc-only Goals/Skills/context track (architecture 21,
  "Compaction, cancellation, and recovery"; architecture 13 assigns it that ownership at
  `docs/intention-relay/architecture/13-mandate-domain-and-durable-lifecycle.md:468`), which is
  M10–M12 scope.
  Nothing prunes stored history (architecture 04: "M3 retains complete stored history").
- **Blast radius**: input-token cost grows quadratically with turns (run k sends k turns of
  history; a 20-turn session sends roughly 10× the unique content: derivation
  Σ_{k=1..20} k / 20 = 10.5), and eventually the provider rejects the request with a
  context-length error that the product surfaces only as a failed run. The only workaround is a
  new session, which is itself hard to find (F06) and loses context.
- **Simplest alternative**: an explicit per-run context budget with oldest-turn trimming plus
  pointer to compaction, or at minimum a pre-flight size check that fails with a clear remedy.
  Own the ordinary-session context contract somewhere before M12.
- **Disposition**: **re-scope** (move ordinary-session context management out of the doc-only
  Goals/context track).
- **Effort**: M (5–10 days for a minimal budget + fixtures) — larger if compaction is included.
- **Confidence**: high on the mechanism; medium on how quickly a user hits it (depends on model
  window and turn size).
- **Cost of delay**: money (invisible token duplication) and a hard wall that makes the product
  unusable for long projects, exactly where a coding agent is most valuable.

---

### Z1-F09 — "Failure memory is erased: neither the failure nor the partial work reaches the next run"

- **ID / severity**: Z1-F09, **S3**
- **Stage**: `shipped`
- **Type**: MIXED (clean-context correctness vs. operator re-narration)
- **Trigger**: any failed or interrupted run followed by "try again".
- **Mechanism**: context includes the user turn of every started run but assistant content only
  when the historical run status is `Completed` (`crates/intention-storage-sqlite/src/lib.rs:1227-1234`);
  failed/interrupted partial output and the failure code never enter the next request. There is no
  retry command in the protocol (`crates/intention-protocol/src/lib.rs:613-634`); retry means a
  new send with a new turn id (`crates/intention/src/lib.rs:900-925`).
- **Blast radius**: the user must re-explain the failure and the work state; the agent restarts
  from the last completed answer and may repeat expensive tool work. Token cost is paid again.
- **Simplest alternative**: allow the user (or the UI) to attach the previous failure's safe code
  and the partial answer as an explicit user-visible context item; keep the default clean.
- **Disposition**: **simplify** (a typed "continue with failure context" path), keep the default.
- **Effort**: S–M (2–4 days).
- **Confidence**: high.
- **Cost of delay**: minor per event, constant across days.

---

### Z1-F10 — "Persistent storage failure produces a silent infinite retry and a wedged session"

- **ID / severity**: Z1-F10, **S3**
- **Stage**: `shipped`
- **Type**: LANDMINE (resource pressure)
- **Trigger**: disk full, DB file locked/unwritable, or repeated transient write failures while a
  run is active.
- **Mechanism**: executor error → try to fail the run (`crates/intention-daemon/src/lib.rs:235-265`);
  when that write fails too, `spawn_cancellation_terminalizer` loops with one immediate retry and
  then a fixed 25 ms pacing (`:47`, `:314-356`). There is no backoff, no cap, no operator-visible
  error, and no shutdown path. While the run stays non-terminal, every new turn queues
  (`crates/intention-storage-sqlite/src/lib.rs:792-799`).
- **Blast radius**: up to ~40 failed transactions/second (fixed 25 ms pacing), silent; the
  session is blocked until the operator discovers the condition outside the product; on some
  storage failures this also churns the log or blocks startup permanently.
- **Simplest alternative**: exponential backoff with a bounded attempt count, a durable
  "storage_degraded" health projection, and surfacing the failure to connected subscribers.
- **Disposition**: **simplify**.
- **Effort**: S (1–2 days).
- **Confidence**: high on mechanism; low on frequency (requires storage failure).
- **Cost of delay**: low until it happens; then it is the least debuggable failure in the system.

---

### Z1-F11 — "Unreachable surfaces on the operator path (`WaitingInput`; the session-subscribe tail; the no-op production hook)"

- **ID / severity**: Z1-F11, **S4**
- **Stage**: `shipped`
- **Type**: DEAD / THEATER (no reachable effect at this baseline)
- **Trigger**: none today; fires when a client author trusts the documented surface.
- **Mechanism**:
  1. `WaitingInput` exists as a variant and in the transition table
     (`crates/intention-domain/src/lib.rs:43`, `:84-92`) and in the storage codec
     (`crates/intention-storage-sqlite/src/lib.rs:1845`, `:1859`), but no production code
     transitions a run into it (searched all crates; only tests reference it).
  2. `session.subscribe` always returns an empty event tail — both branches construct
     `SessionEventTailBatchDto::new(..., Vec::new())` (`crates/intention/src/lib.rs:811-860`),
     while architecture 03's reconnect diagram promises "event tail or typed resync"; the real
     replay path is run-scoped (`crates/intention-daemon/src/lib.rs:673-780`).
  3. The production hook pipeline registers exactly one no-op `Continue` observer at one phase
     (`crates/intention/src/lib.rs:209-236`), hook-failure observations are dropped by the
     composition's `()` sink (`crates/intention-application/src/lib.rs:191-193`), and the reserved
     `ask_user` slot has neither schema nor executor (`crates/intention-tools/src/lib.rs:1142-1150`).
- **Blast radius**: an adapter author implements the documented session-tail reducer and discovers
  it never receives data; a state-machine reader builds UI for a state that cannot occur. Both
  waste implementation time, then require rework.
- **Simplest alternative**: delete the tail field from the session-subscribe result (or
  implement the read), delete `WaitingInput` until `ask_user` exists, and either implement a real
  observer hook or drop the no-op registration until its consumer lands.
- **Disposition**: **remove** the tail and `WaitingInput`; **defer** the hook wiring.
- **Effort**: S (0.5–1 day each).
- **Confidence**: high.
- **Cost of delay**: small per unit, but dead public surfaces are copied into clients and tests
  and are then expensive to remove.

---

### Z1-F12 — "Harness documentation reintroduces numeric caps the project's own policy forbids"

- **ID / severity**: Z1-F12, **S4**
- **Stage**: `doc-only` (architecture 26, M5+ slice 3 / Mandate-track adjacent)
- **Type**: BUREAUCRACY
- **Trigger**: implementation as specified.
- **Mechanism**: architecture 26 fixes 64 rules per daemon, 16 concurrent non-terminal subtree
  work items, 256 total launches per original cause, 8-deep cause chains
  (`docs/intention-relay/architecture/26-continual-harness.md`, "Bounds"). The project's
  limits-by-precedent policy requires a real demonstrated precedent for every numeric cap
  (`docs/intention-relay/production-ceiling-removal.md`, ADR 0048; `AGENTS.md`, "Never add
  speculative limits").
- **Blast radius**: for a single-user tool, none of these caps protects a demonstrated failure
  mode; they add closed error codes, tests, and operator-visible rejections. When a user does hit
  64 rules, the remedy is a code change.
- **Simplest alternative**: keep only the bounds with named precedents (dossier/checkpoint size
  are representation bounds and are defensible), and treat concurrency as capacity that waits
  rather than an error.
- **Disposition**: **re-scope** during the activating specification.
- **Effort**: S (documentation) — the cost is paid at implementation if not resolved.
- **Confidence**: high on the tension; medium on the eventual outcome (the doc explicitly says
  limits must be re-justified before activation, so the team may already intend this).
- **Cost of delay**: doc-level inconsistency that a future implementer will resolve ad hoc.

---

### Z1-F13 — "Mandate uncertainty quarantine has no operator-answerable exit"

- **ID / severity**: Z1-F13, **S3**
- **Stage**: `doc-only` (architectures 13/16/17, M10–M12)
- **Type**: MIXED (honest safety law, unowned operator burden)
- **Trigger**: any crash, cancellation, or restart while a Mandate-run external attempt has
  started without durable terminal proof (shell command, MCP call, child run, bridge operation).
- **Mechanism**: started work without terminal proof becomes `ExternalEffectUnknown`, which
  atomically blocks retry/continuation/rediscovery and moves the Mandate to
  `PausedAwaitingDecision` (`docs/intention-relay/architecture/13-mandate-domain-and-durable-lifecycle.md`,
  "External attempts, recovery, and reconciliation"); the only exits are exact manual
  reconciliation to `Active` or `Stopped`, and the record requires "exact uncertainty and frozen
  baseline". The reconciliation command/projection shape is explicitly deferred ("Exact SQL
  tables, migrations, event variants, wire tags, pages, retention, crate activation, and protocol
  implementation are deliberately deferred"), as is architecture 17's verifier authority.
- **Blast radius**: human-days per crash event for a single user, on a question the system cannot
  answer either. The shipped M5 behavior (mark `Interrupted`, let the user re-send) is actually
  *more operable* than the mandated Mandate pause; the Mandate model adds a durable obligation
  without a mechanism to discharge it.
- **Simplest alternative**: define the user-facing reconciliation action (one command with three
  choices: "effect happened", "effect did not happen, safe to retry fresh", "stop") in the same
  package that mandates the pause, before the pause is activated; or scope the quarantine to
  effects where a wrong answer is dangerous (external publication), and keep ordinary local
  commands on the shipped interrupt path.
- **Disposition**: **defer** activation until the reconciliation surface is specified; **re-scope**
  the quarantine class for local single-user work.
- **Effort**: L (10+ days, because it touches lifecycle, protocol, UI).
- **Confidence**: medium-high (documents are explicit; the eventual activation spec could close
  the gap).
- **Cost of delay**: if M10–M12 are built as written, daily failure recovery becomes a mandatory
  adjudication ritual.

---

## 4. Keep-list (justified mechanisms)

These are mechanisms I tested against the simplest-sufficient-world and judged worth their cost
under real project conditions, with the reason they survive and any cheap improvement.

| # | Mechanism | Why it survives | Evidence |
| --- | --- | --- | --- |
| K1 | One daemon owning sessions, runs, state, and publication | Removes state duplication across adapters; the alternative is each UI re-implementing durability and recovery | architecture 03, "Ownership"; `crates/intention-daemon/src/lib.rs:982-1012` |
| K2 | Typed DTO-only protocol over a private local socket (JSON-RPC 2.0, NDJSON, exact version equality) | Standard, debuggable, and small; the version equality rule prevents silent misinterpretation | `crates/intention-transport/src/lib.rs:33-45`; `crates/intention-protocol/src/lib.rs:613-634`; `https://www.jsonrpc.org/specification` |
| K3 | Durable append-only facts plus replayable run stream | Makes restart semantics honest and lets a UI reconnect without guessing; the audit trail is the product's memory | architecture 04, "Event taxonomy"; `crates/intention-daemon/src/lib.rs:673-780` |
| K4 | Recovery interrupts unfinished work instead of auto-resuming | Correct for unknown external effects; auto-resume would be a lie for shell/MCP/child work | architecture 03, "Daemon restart semantics"; `crates/intention-storage-sqlite/src/lib.rs:1337-1371` |
| K5 | Two-step cancellation (`Cancelling` → `Cancelled`) | Cheap to implement, prevents a stop request from racing a terminal commit; the queue promotion path depends on it | `crates/intention-runtime/src/lib.rs:95-134`, `:1200-1234` |
| K6 | Atomic terminal-and-promote transaction; queued turns keep their proposed run id and config revision | Removes a whole class of queue/state races at low cost | `crates/intention-storage-sqlite/src/lib.rs:944-999`, `:461-499` |
| K7 | Six working typed tools with bounded, truncated, redacted output and cancellation-aware execution | The output bounds and truncation flags are what keep both the model context and the DB honest | `crates/intention-tools/src/lib.rs:145-156`, `:1409-1412`; architecture 05 |
| K8 | `WorkspaceRoot` as an addressing anchor (relative base, execute CWD, default search scope), not a fake sandbox | Removes the pretense of a lexically enforced containment boundary that would be unsound; absolute paths are addressed as given | architecture 05/15; ADR 0047 |
| K9 | No per-action confirmation in Build; Plan `write`/`edit` denial as a product policy | Correct daily-use default for a single trusted user; confirmations are the classic agent deadweight | architecture 05, "Autopilot and Mandate tool boundary" |
| K10 | Local endpoint permissions (0700 parent, 0600 socket), 1 MiB message cap, stale-socket identity probe, subscriber queue + write deadline + typed resync | Liveness safeguards with demonstrated failure modes; they protect the daemon from a bad adapter without new bureaucracy | `crates/intention-transport/src/lib.rs:33-45`, `:868-884`; `crates/intention-daemon/src/lib.rs:45-49`, `:673-780` |
| K11 | Config file with owner-only permission enforcement and credential-free snapshots pinned per run | The permission check is one command away and prevents accidental key exposure; per-run pinning keeps a mid-run config edit from changing an admitted run | `crates/intention-config/src/lib.rs:771-790`; architecture 04, "Durable input queue" |
| K12 | No speculative quotas in Mandate admission; limits only with precedent | Right policy; the counterexample is Z1-F12 | ADR 0048; `docs/intention-relay/production-ceiling-removal.md` |
| K13 | Bounded, typed tool-result evidence (512 KiB) separate from model-visible content (64 KiB) | Keeps audits honest without flooding the model | `crates/intention-storage/src/lib.rs:95`; `crates/intention-tools/src/lib.rs:145` |

---

## 5. Predictions (cut / rewrite / survive)

| # | Prediction | Signal to watch | Confidence |
| --- | --- | --- | --- |
| P1 | **Cut**: the session-subscribe event-tail contract will be deleted or the subscription replaced by a session-level live channel during M6 | An M6 UI spec that needs session-level updates (queue depth, other-run status) and discovers the tail is always empty | high |
| P2 | **Cut**: `WaitingInput` will be removed before or during M10–M12, because `ask_user` is specified as a normal long-running tool that explicitly does not use it | Architecture 15's `ask_user` wording surviving into an activation spec | high |
| P3 | **Rewrite**: admission after restart will gain a startup sweep or durable held rows; the shipped orphaned-promotion path will not survive the first real crash-with-queue report | Any bug report of the form "my turn never runs after restart"; the existing PR24 ledger entry (PR24-007) resurfacing | high |
| P4 | **Rewrite**: the fixed 30-second `execute` cap and the absolute per-round provider deadline will both be replaced (shell descriptor; progress deadline) | Architecture 15's `model_stream_progress_timeout_v1` and `ShellCommandTextDto` being scheduled into an activation slice | high |
| P5 | **Rewrite**: per-commit full snapshot derivation will become incremental or batched once reasoning deltas land in real daily use | Profiling during M6; a complaint about UI latency/heat | medium-high |
| P6 | **Survive**: single daemon + typed protocol + durable facts + run replay + two-step cancellation + interrupt-not-resume recovery + WorkspaceRoot anchor + bounded tools | These are the parts every alternative also needs; nothing in the audit found a cheaper correct substitute | high |
| P7 | **Rewrite/own**: ordinary-session context management (budget/compaction) will move out of the doc-only Goals/context track and become an ordinary-run feature before M10 | The first provider context-length failure in real use; M5+ slice 3/4 scope decisions | medium |
| P8 | **Defer or amputate**: most of the M10–M12 Mandate stack (scheduler, child graph, verifier authority, MCP lifecycle, bridge, kernel) will not reach daily use in its documented form for a single local user; slices will be cut or re-scoped | M6–M9 delivery slipping; absence of a second user or team scenario; frequency of actual harness use | medium (this is a scoping judgment, not a code fact) |
| P9 | **Survive with re-scoping**: the Continual Harness idea survives only as scheduled read-and-delegate verification work with a small number of user-authored rules | Harness rule usage counts once shipped; the numeric caps in architecture 26 being re-justified or dropped | medium |
| P10 | **Rewrite**: the 0600 config cliff will be softened (auto-chmod or precise remedy text) | First-run support questions; M5+ slice 2 re-activation including configuration editing | medium |

---

## 6. Metrics and commands used

### 6.1 Static measures collected

| Metric | Value | Derivation |
| --- | --- | --- |
| Production Rust lines | 28,357 | `cat crates/*/src/*.rs \| wc -l` |
| Test Rust lines / files | 24,962 / 53 | `find crates -path '*/tests/*.rs'` then concatenate and count |
| Workspace crates | 24 | `ls -d crates/*/` |
| Architecture documentation | 13,549 lines / 787,971 bytes in 32 files | `wc -l docs/intention-relay/architecture/*.md` |
| ADR files | 50 | `ls docs/intention-relay/decisions/*.md \| wc -l` |
| Transport message cap | 1,048,576 bytes | `crates/intention-transport/src/lib.rs:33` |
| Connect timeout / sync IO timeout | 500 ms / 10 s | `crates/intention-transport/src/lib.rs:34`, `:45` |
| Provider attempt timeout / attempts | 30 s / 2 | `crates/intention-config/src/lib.rs:616-617` |
| Retry wait | 250 ms fixed | `crates/intention-runtime/src/lib.rs:179` |
| Assistant content fact size | 4 KiB | `crates/intention-runtime/src/lib.rs:178` |
| Durable fact cap / tail page | 512 KiB / 256 facts, 512 KiB | `crates/intention-storage-sqlite/src/lib.rs:32-34` |
| Execute timeout / output cap | 30 s / 64 KiB | `crates/intention-tools/src/lib.rs:145-146` |
| Grep aggregate / edit target cap | 128 KiB / 1 MiB | `crates/intention-tools/src/lib.rs:152`, `:156` |
| Subscriber queue / write deadline | 64 messages / 10 s | `crates/intention-daemon/src/lib.rs:45-46` |
| Terminalizer retry delay | 25 ms, uncapped | `crates/intention-daemon/src/lib.rs:47` |
| Client startup window / stream reply timeout | 3 s / 30 s | `crates/intention-client/src/lib.rs:30-33` |
| Protocol surface | 4 commands, 2 queries, 1 run-subscription request, 1 daemon-to-client `run.frame` notification | `crates/intention-protocol/src/lib.rs:613-634`, `:1454` |
| Registered tools / active model-visible tools | 14 slots / 6 active | `crates/intention-tools/src/lib.rs:1041`, `:1245` |
| Production hooks registered | 1, no-op | `crates/intention/src/lib.rs:209-236` |

### 6.2 Per-scenario intervention counts (derived, not measured)

| Scenario | User commands | Confirmations | Decisions | Waits | Loss if not handled |
| --- | --- | --- | --- | --- | --- |
| First turn today (Walkthrough A) | config write + chmod + build + custom client (4+) | 0 | 3 (provider, model, root) | build time | not usable without writing code |
| Normal turn (product) | 1 send | 0 | 0 | model latency | none |
| Provider failure mid-stream | 1 re-send | 0 | 1 (retry/skip) | 0 | full prompt re-billed |
| Long command > 30 s | 1 re-send + manual run | 0 | 1 | 30 s to fail | run output lost |
| Crash with 1 queued turn | 1 stop (if a client exists) | 0 | 1 | indefinite | 1 turn stalled per restart |
| Crash with N queued turns | 1 stop per stranded run to drain, or 0 with no client | 0 | N | indefinite | N turns advanced only by restarts |
| Config change | 1 edit + 1 external kill + relaunch | 0 | 1 | 3 s bootstrap | active run interrupted |
| Disk full during run | 0 in product | 0 | 0 | infinite | silent retry loop, session wedged |
| Long session (20 turns) | 0 extra | 0 | 0 | growing latency | ~10× token duplication; context-limit failure risk |

### 6.3 Commands actually used

`rg` / `Grep` pattern searches across `crates/**` and `docs/intention-relay/**`; `wc -l` and
`wc -c` for sizes; `find` for test inventory; targeted reads with offsets for each cited region;
`rg` for absence claims (for example `session.list`, `ListSessions`, `validate_reasoning_fact_output_bound`,
`mark_recovered_run_held_for_daemon`, `WaitingInput`, `load_tail` callers). No `cargo` command was run
(offline/no-build constraint); no profiling, no tests.

### 6.4 Note on the missing per-run reasoning bound (supporting evidence for Z1-F04)

`crates/intention-runtime/src/lib.rs:949-955` claims a per-run 4 MiB reasoning bound is "enforced
at the durable append authority" via `intention_domain::validate_reasoning_fact_output_bound` and
per-run `reasoning_aggregate_bytes` accounting. Neither symbol exists anywhere in `crates/` at this
baseline (repo-wide search returns only the comment and the reconciliation ledger). The only live
reasoning bounds are 512 KiB per fact and 512 KiB per round echo
(`crates/intention-runtime/src/lib.rs:189`, `crates/intention-model/src/lib.rs:186`). This is a
stale comment referencing removed machinery; it matters because it makes a real absence look
handled when reading the runtime. (Recorded here rather than as a separate card because its
practical blast radius is the write amplification of Z1-F04 plus DB growth.)

---

## 7. Uncertainty and open questions

1. **No runtime measurement.** All performance claims (write amplification, quadratic snapshots,
   25 ms spin rate) are structural derivations from code, not profiles. The read-only constraint
   forbade building or running. A 30-minute profiling session with a reasoning-model stream would
   confirm or refute Z1-F04's magnitude.
2. **Provider behavior is modeled, not observed.** Frequency estimates for Z1-F03 (timeouts and
   429s after first output) depend on provider behavior that this audit could not exercise.
3. **M6 scope is unknowable from this baseline.** Z1-F06's claim that no session-discovery
   contract exists is verified by absence in code and docs; whether the M6 authors intend to add
   one quietly is unknown, which is exactly why I flag it as an unowned dependency.
4. **The reverted Slice 2 changes the counterfactual.** Several mechanisms I flag as missing
   (reload, held-run admission, usage aggregation) existed in a reverted branch. I judged `main`
   as the baseline; if the controller intends a near-term re-landing, the cost-of-delay estimates
   for Z1-F05 and Z1-F01 should be reduced accordingly.
5. **Mandate-track severity depends on activation.** Z1-F12 and Z1-F13 are documentation-level
   judgments; if M10–M12 are never activated in their current form, both are cheap to fix in
   documents now and expensive to fix later.
6. **Single-user trust assumptions.** I treated "no sandbox, no per-action confirmation" as
   justified (keep-list K8/K9). If the project's actual usage model later includes running
   untrusted repositories or third-party prompts as a daily practice, that judgment should be
   revisited — the architecture itself is explicit that these are logical policies, not OS
   boundaries.


---

# Appendix B — Zone 2: data, concurrency, ordering, recovery

Baseline: `main` @ `3291e50` (working tree clean except the untracked file named in
the task, which was not touched). Read-only audit: no build, no test run, no
tracked file modified. This is an independent first-principles audit; every claim
below is derived from `file:line` evidence in this repository, from the vendored
dependency sources, or from cited public documentation, and every number shows
its derivation.

Deliverable file: `/home/data/intention-relay/audit-drafts/zone-2-data-concurrency.md`.

---

## 1. Scope and method

### 1.1 Scope

Persistence and coordination end to end, as shipped on `main`, plus the accepted
documentation that governs it:

- storage engine, schema, and every persisted structure:
  `crates/intention-storage-sqlite/src/lib.rs` (2,721 lines, the only backend),
  `crates/intention-storage/src/lib.rs` (the DTO-only repository contract),
  `crates/intention-domain/src/lib.rs`, `crates/intention-domain/src/model_facts.rs`;
- write paths and transaction boundaries: `RuntimeService`,
  `ModelRunExecutionService`, `ApplicationService`, `DaemonApplicationFacade`,
  the daemon host;
- read paths and live consumers: session projections, run replay/tails, live
  publication, tool-result publication reread, model-context assembly;
- every persisted sequence, cursor, ticket, and identity, with owner and
  consumers;
- locking and contention: repository mutex, SQLite connection, daemon task
  registry, publication gate, tool-cancellation registry, command gate;
- durability and recovery: WAL, commit durability, recovery-before-ready,
  promotion, and what happens to recovered work;
- the documented future load: architectures 13, 14, 16, 17, 18, 21, 23, 24 and
  the cross-domain sequence table in `architecture/README.md`, plus the roadmap
  scale statements (M0–M12).

### 1.2 Method

1. **Sequence/identity inventory.** Enumerate every ordering and identity value
   that is persisted, its owner, its creation site, and every consumer
   (Section 2.1). A value with no consumer that costs work on every commit is a
   finding; a value with one consumer that justifies it is a keep-list entry.
2. **Write-transaction map.** Enumerate every write path, its statements, its
   lock acquisitions, and its lock order (Section 2.2). Static lock-order
   analysis for deadlock (Section 2.3).
3. **Cost derivation.** Derive per-commit statement counts, row reads, JSON
   parses, bytes written, and fsync count as functions of session size; carry
   the derivation so the numbers can be falsified (Sections 2.4, 3, 6).
4. **Failure-scenario construction.** For each suspected landmine, write the
   exact trigger, the code path that fires, and the observable consequence
   (Section 3 cards).
5. **Simplest-sufficient-world test.** For each mechanism ask: would a competent
   team building the minimal working solution for one local user write this?
   (Section 2.5, and the `simplest alternative` field of each card.)
6. **Read-only verification commands** (Section 6).

Constraints honored: no build, no test execution, no sub-agents, no writes
outside this file, no modification of tracked files.

### 1.3 Stage vocabulary (per the audit rubric)

- `shipped` — M0–M5 code on `main`.
- `committed` — M5+ or M6–M9 accepted documents (activation not shipped).
- `doc-only` — M10–M12 or uncommitted directions.

---

## 2. Vectors examined → verdicts

### 2.1 Sequence, cursor, identity inventory (owner · creation · consumers · verdict)

| Value | Owner / creation | Consumers | Verdict |
| --- | --- | --- | --- |
| `SessionId`, `ProjectId`, `WorkspaceId` (UUID) | `intention-types`; created by command (`create_session`, `storage-sqlite/src/lib.rs:640-673`) | all tables, projections, protocol | JUSTIFIED |
| `TurnId` (UUID, client-supplied) | `intention-types`; embedded in `SendUserTurnCommandDto` | `turns` PK `(session_id, turn_id)` (`:55-60`), queue ordering, `RunId` derivation | JUSTIFIED |
| `RunId` (UUID) | daemon derives it **from the turn id** (`crates/intention/src/lib.rs:899-907`); queued turns persist a proposed run id at acceptance (`storage-sqlite:815, 864`) | `runs` PK, `run_cursors`, `model_run_facts`, `run_snapshots`, `model_run_snapshots`, protocol | JUSTIFIED with a note: run identity is extensionally equal to turn identity, which the cross-domain table warns against (`architecture/README.md:205-206`); consequence is a typed `turn_identity_conflict` (`storage-sqlite:772-786`) instead of cross-session run reuse — acceptable, documented nowhere |
| Queue ticket | `sessions.next_queue_ticket`, incremented inside the accepting transaction (`storage-sqlite:854-866`) | `QueuedTurnProjectionDto`, promotion `ORDER BY queue_ticket ASC LIMIT 1` (`:461-499`), `SendUserTurnOutcomeDto::Queued` | JUSTIFIED (monotonic, never renumbered, monotonic even after removal). Doc gap: absent from the cross-domain sequence table (`architecture/README.md:209-220`) → Z2-F16 |
| Session event sequence | `sessions.last_sequence` + `domain_events` `UNIQUE(session_id, sequence)` (`storage-sqlite:42-101`, appended at `:314-362`) | committed-change evidence, session snapshot `at_sequence`, `RunSnapshotDto.at_sequence`, `load_tail` (`:1388-1453`), `subscribe` (`intention/src/lib.rs:811-862`), model-context scans (`:1591-1660`) | MIXED: real ordering value, but its client-facing delivery is empty (always) → Z2-F07; and its *contents* are the substrate of the O(F)/O(E) scans → Z2-F04 |
| Run event cursor | `run_cursors.cursor` + `model_run_facts.cursor`, seeded by `ensure_run_cursors` (`:501-509`) | `RunReplayDto`, `RunLiveBatchDto`, live subscribe, optimistic append (`expected_cursor`, `:989-1000`) | JUSTIFIED (bounded contiguous replay, gap detection, conflict detection). Doc/code contradiction on seeding → Z2-F17 |
| `EventId` | `EventId::new()` per appended event (`:322-334`) | `domain_events` PK, `model_run_facts.event_id` FK, `tool_results.event_id` FK | JUSTIFIED as an anchor; note "deduplication" is claimed in `architecture/04:189` but no dedupe path reads it — the real idempotency is the turn-id check and `expected_cursor`; the claim is redundant, the cost is negligible (keep) |
| `ConfigRevisionId` | `ConfigSnapshotDto`; persisted in `configuration_revisions` (`:216-254`) | `turns.config_revision_id`, `runs.config_revision_id`, `load_run_config_snapshot` (`:1100-1140`) | JUSTIFIED (freezes execution meaning at acceptance; same-id idempotency with typed conflict) |
| `sessions.config_revision_id` | always inserted `NULL` (`:673`), never updated | `SessionProjectionDto.config_revision_id` (`domain:419-420`) — always `None`, carried through protocol and client | DEAD → Z2-F11 |
| `AssistantTurnId` | runtime, per attempt (`runtime:515`) | `AssistantContentAppended` facts, model projection | JUSTIFIED |
| `ToolCallId` | provider-normalized tool call | `tool_results` PK, tool lifecycle events, model facts | JUSTIFIED |

### 2.2 Write-transaction map (all on one connection, all `BEGIN IMMEDIATE`)

| Path | Site | Statements of interest | Extra work after the write itself |
| --- | --- | --- | --- |
| `create_session` | `storage-sqlite:640-696` | projects/workspace/sessions inserts, 1 event, `last_sequence` | `finish` → full session + run snapshot fan-out |
| `accept_user_turn` | `:697-906` | turns insert, optional runs insert or queue ticket + `queued_turns` insert, 1-2 events | `finish` (fan-out) |
| `remove_queued_turn` | `:907-943` | `queued_turns` delete, 1 event | `finish` |
| `transition_run` | `:944-988` | runs update, 1-2 events, optional promotion (runs insert + `RunStarted`) | `finish` |
| `append_model_run_facts` | `:989-1099` | per fact: domain_events insert + `model_run_facts` insert; `run_cursors` update; optional status + promotion | `snapshot` + `snapshot_model_runs` |
| `append_tool_lifecycle_event` | `:542-621` | 1 event, optional `tool_results` insert | `snapshot` + `snapshot_model_runs` |
| `accept_configuration_revision` | `:1455-1461` | 1 insert | none (no fan-out) — the cheap path |
| `recover_unfinished_runs` | `:1337-1371` | one `transition_run` transaction **per unfinished run** | N full fan-outs before readiness |

`finish` (`:510-527`) and the two explicit call sites are the funnel: **every**
write path ends in `project()` + `snapshot()` + `snapshot_model_runs()`.

### 2.3 Lock inventory and lock-order analysis

Locks held in the process: `SqliteStorageRepository.connection: Mutex<Connection>`
(`storage-sqlite:138-142`, accessor `:167-169`), `DaemonApplicationFacade.command_gate:
Mutex<()>` (`intention/src/lib.rs:82`, taken at `:548,591,630,678,887`),
`PrivateModelRunDispatch.admitted` (test-only), `HostData` mutex (`daemon:110`),
`publication_gate` (`daemon:111`), `tool_cancellations` (`intention/src/lib.rs:83`).

Observed orders:

- `daemon::stop_run` holds `HostData` → calls `facade.stop_run_for_daemon_host`
  (takes `command_gate` → connection) → drops `HostData` → `publish_current`
  (takes `publication_gate` → connection) (`daemon:282-313, 564-628`).
- `schedule_if_starting` holds `HostData` → calls `facade.schedule_starting_run_for_daemon`
  (connection) (`daemon:156-179`).
- `command_result` holds `command_gate` → connection (`intention/src/lib.rs:886-935`).
- `stop_run_for_daemon_host` holds `command_gate` → connection → `tool_cancellations`
  (`intention/src/lib.rs:548-566`).

**No cycle exists**: every path that nests takes `HostData` → `command_gate` →
connection (or `publication_gate` → connection), and no path acquires them in the
reverse order. Verdict: no deadlock found under static analysis; the ordering
should be recorded as an invariant. The problem is not the order but the
**scope** of the global locks: see Z2-F05.

Liveness bound on SQLite itself: the pinned driver is `rusqlite 0.40.2`
(`crates/intention-storage-sqlite/Cargo.toml:17`, resolved at `Cargo.lock:1611-1614`);
the vendored 0.32.0 lineage sets `sqlite3_busy_timeout(db, 5000)` at open
(`/home/data/registry/src/index.crates.io-1949cf8c6b5b557f/rusqlite-0.32.0/src/inner_connection.rs:119`);
so a second process blocking a write waits up to 5 s, then surfaces as a generic
`storage_unavailable` (`storage-sqlite:1974-1976` discards the SQLite error
entirely). See Z2-F14.

### 2.4 Cost model derived from the code (the core quantitative result)

Let the session have `R` runs and `F` durable model facts (`model_run_facts` rows
across all of its runs), with total envelope bytes `B`.

From `snapshot_model_runs` (`storage-sqlite:406-459`) and `snapshot` (`:358-404`),
**per commit**:

- statements: 3 (`project`) + 2 + `R` (`snapshot`) + 2 + `2R` (`snapshot_model_runs`)
  + the write's own inserts ⇒ ≈ **12 + 3R statements**;
- row reads: `F` (`model_projection` selects every fact of every run, `:1729-1782`)
  + `R` cursor reads + 3 scans of `runs`;
- JSON parse work: **every one of the `F` event envelopes is deserialized**
  (`serde_json::from_str` at `:1560,1750`), i.e. `B` bytes;
- JSON serialize/write work: `R` run snapshots, each containing that run's whole
  `assistant_content` (concatenated at `:1749-1765`), plus the session projection;
- 1 WAL fsync: no `synchronous` pragma is set (`:155` sets only `foreign_keys` and
  `journal_mode`), and the bundled default is FULL
  (`libsqlite3-sys-0.30.0/sqlite3/sqlite3.c:17400-17406`).

The same work is repeated by the read-side twin: `append` clones the whole
`RunSnapshotDto` into the returned outcome (`storage-sqlite:1087-1095`,
`runtime:1051-1080`), the runtime clones it again into the observer payload
(`runtime:1112-1127`), and the daemon observer uses only the status field, then
re-reads and re-parses the same snapshot from disk in `publish_current`
(`daemon:812-822, 564-628`).

Commits per user turn, from the runtime's own call sites (text chunk = 4 KiB,
`runtime:178, 1238-1261`; one commit per reasoning delta, `runtime:928-947`; five
commits per tool call: `runtime:704-740` + `application:408-418, 619-624, 756-761`):

```
commits(turn) ≈ 3 + C + D + 5T      C = 4 KiB text chunks
                                    D = non-empty reasoning deltas
                                    T = tool calls
```

Facts added per turn ≈ `C + D + 5T + 2`.

### 2.5 Simplest-sufficient-world verdict (what survives)

For one local user with one SQLite file, the minimal correct design is: one
session table, one turn table carrying the queue ticket and the run id, one
append-only record table that holds the transcript plus tool evidence, one
monotonic per-session sequence, and projections answered **by query**. On that
measure:

- **Survive**: the DTO-only boundary; typed identities; the one-active-run
  partial unique index (`:66-67`); the monotonic never-reused queue
  ticket with oldest-first promotion inside the terminal transaction
  (`:461-499`); the frozen per-run `ConfigRevisionId` (`:216-254`); the run-level
  cursor with bounded contiguous pages (256 facts / 512 KiB, `:33-34`); the
  two-step `Starting → Cancelling → Cancelled` path; recovery that interrupts
  unfinished work without resuming external effects; the post-commit
  independently-reread publication rule.
- **Do not survive**: three snapshot tables and a full replay of every fact on
  every commit (Z2-F01, Z2-F02); event-log scans that exist only because the
  `runs` table has no creation sequence (Z2-F04); one connection behind one mutex
  with a write-lock on a read path (Z2-F05); per-4-KiB-chunk and per-delta commit
  granularity (Z2-F09); a durable transcript with no reader (Z2-F07) and no
  retention (Z2-F08).

### 2.6 Core questions, answered

**Q1 — which mechanisms become the first obstacle or failure in daily use?**

Ranked by (probability of being hit) × (damage):

1. **Slow drift into unusability of a long session** (Z2-F01): every commit
   re-parses and re-serializes the session's whole fact history; the cost is paid
   inside the model-stream loop, so streaming and tool calls get progressively
   slower the longer a session lives. This is the first thing a daily user feels.
2. **Restart with a queued turn wedges the session** (Z2-F03): the queued message
   is promoted into a `Starting` run that nothing can admit, and all later turns
   queue behind it. The user's next turn silently never runs.
3. **Context ceiling with no mitigation** (Z2-F06): once a session's transcript
   exceeds the model window, every new turn in that session fails the same way;
   there is no compaction (M12), no retention policy (future M5+ slice), and no
   protocol read to inspect or export the history.
4. **A stop request that does not respond promptly** (Z2-F05 + Z2-F04): the
   `SendUserTurn` command holds the global `command_gate` while it builds model
   context twice; `StopRun` needs the same gate (`intention/src/lib.rs:886-935`,
   `:548-566`).

**Q2 — full trace (user, data, request): steps, interventions, latency, human-days.**

*Trace A: one user turn in a mature session (R = 20 runs, F ≈ 5,000 facts,
B ≈ 10 MB of envelopes — derivation: 20 turns × (254 facts × ~300 B envelope
overhead + 8 KiB text + 10 × 20 KiB tool results) + JSON escaping).*

| Step | Site | Work | Estimate |
| --- | --- | --- | --- |
| 1. Gate + timestamp | `intention/src/lib.rs:886-887` | 1 mutex | µs |
| 2. `accept_user_turn` commit | `storage-sqlite:697-906` | ~72 statements, F parses, R snapshot writes, 1 fsync | 40-120 ms |
| 3. context build #1 | `application:843-915` → `storage-sqlite:1185-1255` | 2 full event scans (`:1591-1660`) + R snapshot reads + parse of ~10 MB + `IMMEDIATE` txn | 100-400 ms |
| 4. no-op dispatch | `intention/src/lib.rs:181-195` | none (the payload is dropped) | 0 |
| 5. response to client | `daemon:1151-1190` | — | — |
| 6. context build #2 | `daemon:1180` → `:156-179` → `intention/src/lib.rs:721-730` | identical work repeated | 100-400 ms |
| 7. per provider round | `runtime:656-1050` | each text chunk: commit (F parses, 1 fsync) + publish (1 full snapshot read + tail + broadcast); each reasoning delta: 1 commit | 5-30 ms per chunk; 255 commits at D = 200 |
| 8. per tool call | `runtime:704-740`, `daemon:847-897`, `application:408-761` | 5 commits + 1 publication reread | 5 × (commit) + reread |
| 9. terminal | `runtime:1051-1080, 1200-1236` | `Running→Completing`, `Completing→Completed`, promotion of the next queued turn (extra runs insert + events) | 3 commits |

Totals per turn: **~255 durable commits (D = 200, C = 2, T = 10)**, each
`O(F)` parses; two full history reconstructions; ~50-60 JSON/serde round trips of
the session's history; ~300 KiB of new durable bytes (derivation in Section 6),
of which ~200 KiB are a second copy of tool output.

*Trace B: failure path "restart with one queued turn".* User sends a second
message while a run is active (queue ticket 0) → daemon is killed/restarted →
recovery marks the unfinished run `Interrupted` and, in the same transaction,
promotes ticket 0 to `Starting` (`storage-sqlite:944-988, 461-499`) → the host
starts serving but nothing sweeps `Starting` runs (`daemon:999-1029`; the only
callers of `schedule_if_starting` are `daemon:1180` and `daemon:522-523`) → new
turns are queued (`storage-sqlite:789-866`) → session is stuck. Manual
intervention required: **Stop the phantom run** (one user action, then re-send the
lost message). Repair by code: 1-3 human-days (Z2-F03).

*Trace C: "session too long".* At the model window boundary the provider rejects
the request; the driver normalizes it to a terminal failure and the run ends
`Failed` with a typed failure code. The next turn rebuilds the identical context
and fails identically. Interventions available today: create a new session and
accept the loss of continuity — no compaction, no truncation, no export path
(protocol queries are `GetDaemonHealth` and `GetSessionSnapshot` only,
`intention-protocol/src/lib.rs:629-634`; the snapshot carries no messages,
`:897-902`).

*Human-days for the S1/S2 remediation set (Z2-F01…Z2-F06):* **15-30 days** with
this repository's own per-change obligations (contract/outcome tests, fault
injection, docs, architecture policy updates). Per-card estimates in Section 3.

**Q3 — simplest-sufficient-world.** See Section 2.5. In one sentence: keep the
invariants (`one active run`, ticket monotonicity, frozen config revision, run
cursor, recovery-interrupts), delete the projection-fan-out machinery, and let
reads answer from the record table with two added ordering columns
(`runs.started_sequence`, `facts.cursor` already exists).

**Q4 — comparative practice.**

- SQLite's own model is one writer and many readers in WAL mode, with
  checkpointing on the WAL and a configurable `synchronous` default
  (<https://sqlite.org/wal.html>, <https://sqlite.org/pragma.html#pragma_synchronous>,
  <https://sqlite.org/pragma.html#pragma_wal_autocheckpoint>). The shipped design
  uses one connection for everything, so it takes the single-writer ceiling
  without taking the concurrent-reader benefit.
- Periodic snapshots + incremental projections are the standard event-sourced
  pattern; full replay of an aggregate on *every* append is not (general
  practice — no URL claimed). The repository's own architecture already lists
  "Event sourcing as the sole query model" as a non-goal
  (`architecture/04:293-296`), yet the write path recomputes projections from the
  event/fact log on every commit (`storage-sqlite:406-459`).
- Local agent CLIs and single-user desktop apps typically persist an append-only
  transcript per session (JSONL/SQLite) and rebuild views on read (general
  practice; not verified against a citable source in this audit).

**Q5 — future amputation.** See Section 5 (Predictions).

---

## 3. Findings (ranked cards)

Severity: S1 breaks daily use · S2 rewrite or major drag · S3 tolerable tax · S4 minor.
Type: LANDMINE · BUREAUCRACY · EXCESSIVE · DEAD · THEATER · MIXED · JUSTIFIED.

### Z2-F01 — Every commit replays and rewrites the session's entire fact history  ·  S1

- **Stage / type**: shipped / LANDMINE.
- **Trigger (real-world scenario)**: a session used across one working day. Model
  text is streamed in 4 KiB chunks and reasoning in per-delta facts
  (`runtime:178, 1238-1261, 928-947`), so facts accumulate fast; each chunk
  append, each reasoning delta, each tool lifecycle event, each queue/turn
  mutation, and each status transition triggers a commit that re-reads, parses,
  re-concatenates and re-writes the projections of **all** runs of that session.
  Streaming gets visibly slower as the session ages; a mature session's turns take
  seconds of pure storage work before the provider is called again.
- **Mechanism**: `Self::snapshot` (`storage-sqlite:358-404`) and
  `Self::snapshot_model_runs` (`:406-459`) run at the end of every write path
  (`finish:510-527`, `append_tool_lifecycle_event:542-621`,
  `append_model_run_facts:989-1099`); `model_projection` reads every
  `model_run_facts` row of a run and deserializes each envelope
  (`:1729-1782`), and the run set is `SELECT … FROM runs WHERE session_id=?1`
  with no status filter (`:371, 413`), so terminal runs are re-serialized forever
  (the `runs`-table projection at `:263` does filter terminal runs; only the two
  snapshot fan-outs are unfiltered).
  Read-side twin: the append outcome carries a full snapshot clone
  (`:1087-1095`, `runtime:1051-1080`), which the runtime clones again
  (`runtime:1112-1127`) and the daemon reduces to one status enum, then re-reads
  from disk (`daemon:812-822, 564-628`).
- **Quantification** (Section 2.4): per commit ≈ `12 + 3R` statements, `F` row
  reads, `F` JSON parses, `B` bytes parsed, Σ assistant_content bytes written +
  serialized, 1 fsync. Example R = 20, F = 5,000, B ≈ 10 MB → ≈ 72 statements and
  ≈ 10 MB of JSON parsing per 4 KiB of streamed text; 255 commits per turn
  (Section 2.6, Trace A) → **~2.5 GB of JSON parsing per turn** in that state
  (estimate; parse-rate assumption in Section 7).
- **Blast radius**: UX (streaming stalls, tool latency, "the app got slow"),
  perf (quadratic in session history), disk write amplification, and — because
  the connection mutex is global — latency spill-over into unrelated sessions.
- **Simplest alternative**: (a) scope snapshots to the affected run(s) only
  (one-line SQL filter) — removes the `R` factor; (b) apply the appended fact to
  the stored projection incrementally instead of replaying the run (removes the
  `F` factor); (c) drop `run_snapshots` (Z2-F02). Option (b) is the real fix and
  is what the "simplest sufficient world" contains.
- **Disposition**: simplify (scope first, then incremental) — remove the
  redundant terminal-run rewrites and the per-append replay.
- **Effort**: M (3-6 days: storage change, fault-injection and ordering tests
  across `sqlite_contracts.rs`, `m4_durable_facts.rs`, `m4_model_context.rs`,
  plus docs `04:33-34,109`).
- **Confidence**: high on the mechanism and on the asymptotic claim (pure static
  reading of shipped code); medium on absolute millisecond figures (no benchmark
  run, not permitted in this audit).
- **Cost of delay**: compounds with every future aggregate. Architectures 21 and
  24 repeat the same house rule ("atomically commits its projection, event(s),
  idempotency binding, and affected snapshot(s)", `21:308-310`; activity
  projections/journal, `24:217-220`), so the pattern multiplies before it is
  fixed, and M6's UI will make the stall user-visible.

### Z2-F02 — `run_snapshots` is written on every commit and read by no one  ·  S2

- **Stage / type**: shipped / DEAD.
- **Trigger**: any write, forever; the table grows and is rewritten per run per commit.
- **Mechanism**: table at `storage-sqlite:83-86`; written at `:397` from
  `snapshot()` (`:358-404`). Repository-wide readers: only test code
  (`src/lib.rs:2081, 2096` inside `#[cfg(test)] mod tests`; the table name list at
  `tests/sqlite_contracts.rs:867`). No production read of the table exists, and it
  is structurally redundant: `model_run_snapshots.snapshot_json` stores a
  `RunSnapshotDto` whose `ModelRunProjectionDto` already embeds the
  `RunProjectionDto` (`domain/model_facts.rs:591-599, 723-728`), and it is the
  table the real replay reads (`storage-sqlite:1783-1791`).
- **Why it exists**: it is the assertion surface for the doc rule "every affected
  run … receives a run snapshot at that same durable sequence"
  (`architecture/04:33-34,109`) inside the rollback tests
  (`src/lib.rs:2381-2420`). The test is real; the product consumer is not.
- **Blast radius**: one row rewritten per run per commit (part of Z2-F01's `R`
  factor); a second schema object that every future change must keep consistent;
  the atomicity test asserts a table the product does not use, so it can pass
  while the durable state the product actually reads is broken.
- **Simplest alternative**: delete the table and the write; assert the atomicity
  invariant on `model_run_snapshots` (which is read in production) and update
  `04:34,109` and the roadmap line (`11:287,304`) to say "the run snapshot".
- **Disposition**: remove.
- **Effort**: S (0.5-1 day).
- **Confidence**: high (verified by repository-wide search for the table name).
- **Cost of delay**: cheap now; becomes a data-migration question the moment
  "no backward compatibility" ends and a UI/export reads it.

### Z2-F03 — Recovery promotes a queued turn into a run nothing can ever admit ·  S1

- **Stage / type**: shipped / LANDMINE.
- **Trigger**: user sends a turn while a run is active (it is durably queued,
  `storage-sqlite:854-872`), then the daemon restarts (crash, upgrade, reboot,
  or a second instance killing the first). On restart: recovery transitions the
  unfinished run to `Interrupted` — a terminal status — and, because **every**
  terminal transition attempts promotion, ticket 0 becomes a new `Starting` run
  in the same transaction (`:944-988` → `:461-499`). The daemon then serves
  connections without ever sweeping `Starting` runs: `serve_async_listener`
  constructs an empty host and accepts peers (`daemon:999-1029`);
  `schedule_if_starting` is called only from a `SendUserTurn` that started a run
  (`daemon:1171-1182`) and from `on_terminal` after a run terminalizes
  (`daemon:518-524`). The promoted run never becomes a task, never runs, never
  terminalizes. Every later turn is now queued behind it (`storage-sqlite:789-866`
  selects `active` without a status filter beyond the terminal set), and the
  queue can only advance when the active run terminalizes — which requires an
  executor that does not exist. Escape hatch: the user must press Stop on the
  phantom run, which cancels *that* turn (its message is never sent to the model
  and must be retyped) and promotes the next one.
- **Mechanism anchors**: recovery `storage-sqlite:1337-1371`; promotion
  `:461-499`; boot `daemon:999-1029`; scheduling callers `daemon:156-179,
  518-524, 1171-1182`. The behavior is deliberately documented for the *safety*
  half ("does not resume it or a recovery-promoted `Starting` successor",
  `architecture/04:164-166`, `m4.md:234`, `architecture/10:198-201`), and the
  test asserts only that the successor stays `Starting` and that the provider was
  not called (`tests/m4_streaming_foundation.rs:845-862`) — the usability
  consequence is not covered anywhere. The workflow that used to cover it
  ("held" recovery-promoted runs plus an explicit admit command) belonged to the
  M5+ Slice 2 control plane reverted by ADR 0044 and is not re-provided by any
  active document.
- **Blast radius**: UX (silent stall, "my message did nothing"), lost user work
  (the queued turn is cancelled unexecuted when it is stopped), state (a durable
  `Starting` run with no owner), manual intervention once per restart.
- **Simplest alternative**: after `recover_before_ready`, sweep every session
  whose active run is `Starting` and either (a) schedule it exactly once (the
  turn was accepted and its config snapshot is frozen — this is honest work, not
  resumption of external effects), or (b) deterministically fail it typed so the
  queue advances without user action. Either is a few lines plus a test; the
  decision between them is a product call the active docs do not make.
- **Disposition**: re-scope (define the recovered-successor admission rule), then
  implement; the "never resume interrupted work" rule itself should stay.
- **Effort**: S/M (1-3 days including the daemon-host test that asserts a
  post-restart session can still make progress).
- **Confidence**: high on the mechanism and on the absence of an admission path;
  medium on frequency (needs "queued turn + restart").
- **Cost of delay**: every restart with queued work burns a user action and a
  re-typed message; the M10-M12 Mandate scheduler will make "durable pending work"
  the normal case, so the missing admission path becomes a structural hole, not
  an edge case.

### Z2-F04 — Reads that should be table lookups scan and parse the whole event log  ·  S2

- **Stage / type**: shipped / EXCESSIVE.
- **Trigger**: every started turn (context assembly) and every tool lifecycle
  append. Session with a long history → the scans walk every event envelope of
  the session, including 64-128 KiB tool-result facts.
- **Mechanism**: `started_runs_before` (`storage-sqlite:1591-1633`) and
  `target_started_event` (`:1635-1660`) both `SELECT envelope_json FROM
  domain_events WHERE session_id=?1 ORDER BY sequence` and deserialize every row
  to recover run ordering and one `RunStarted` — although `runs(session_id,
  turn_id)` and the queue tables already hold that data; the `runs` table simply
  has no creation-sequence column (`:61-65`). `latest_tool_lifecycle_status`
  (`:1463-1518`) scans the log backwards with a JSON parse per row to find one
  call's previous status. Then the double build: `send_user_turn_and_schedule`
  builds the context (`application:843-915`) and hands the DTO to a dispatch port
  whose production implementation discards it (`intention/src/lib.rs:181-195`);
  the daemon host rebuilds the same context (`daemon:1180` → `:156-179` →
  `intention/src/lib.rs:721-730`) — under the global `command_gate` for the first
  copy and under the connection mutex for both.
- **Quantification**: per context build ≈ `2 × E` envelope parses (E = session
  events) + `2 × 3R` run/turn/snapshot reads; from Section 2.6 Trace A: ~10 MB
  parsed per build, two builds per turn, ~100-800 ms per turn at R = 20
  (estimate). Growth is linear in stored history, i.e. per-turn latency grows
  without bound over the life of a session.
- **Blast radius**: perceived latency of "send"; stop/cancel responsiveness
  (Z2-F05); CPU/IO per turn.
- **Simplest alternative**: add `runs.started_sequence` (or reuse the
  `RunStarted` event's sequence captured at insert) and order runs by it; order
  the model context from `runs`/`turns`/`model_run_snapshots` directly; build the
  context once (delete the no-op dispatch seam or the second build).
- **Disposition**: simplify.
- **Effort**: S/M (2-4 days; touches storage schema, application, daemon, tests
  `m4_model_context.rs`, `m3_application.rs`).
- **Confidence**: high (mechanism is unambiguous); medium on the latency figures.
- **Cost of delay**: the M6 UI will poll session state; if the read surface is
  built on these scans, every UI refresh pays them.

### Z2-F05 — One connection behind one mutex; a read path takes the write lock  ·  S2

- **Stage / type**: shipped / EXCESSIVE.
- **Trigger**: any two activities at once — two sessions streaming, a live
  publish while a turn is accepted, a UI snapshot query while a tool result
  commits. All of them serialize on `Mutex<Connection>`
  (`storage-sqlite:138-142, 167-169`). A long history in *one* session increases
  the hold time of *every* commit for *all* sessions (Z2-F01), so one big session
  degrades unrelated ones. Separately, `load_starting_run_model_context` — a pure
  read — opens `TransactionBehavior::Immediate` (`:1188-1193`), taking the write
  lock for its whole duration.
- **Mechanism anchors**: single connection/mutex; `immediate_transaction!`
  (`:529-540`) wraps every write; `command_gate` (`intention/src/lib.rs:82, 887`)
  serializes all commands globally, so a `SendUserTurn` in session A holds the
  gate while building context (Z2-F04), delaying `StopRun` (which needs the same
  gate, `:548-566`); `publication_gate` (`daemon:111`) serializes live
  publication, and each publish re-reads the run snapshot (`daemon:564-628`).
- **Quantification**: sustained commit throughput ≈ `1 / cost(commit)` per
  process. At the Section 2.6 example cost (F ≈ 5,000 parses + ~10 MB parse) that
  is on the order of tens of commits per second *total*, shared by all sessions
  and all tool loops.
- **Blast radius**: perf, UX (a stalled stop button), and a hard structural
  ceiling on the documented future (16 concurrent non-terminal children in one
  tree, `architecture/17:314-317`; the RLM/activity journals add more writers).
- **Simplest alternative**: a dedicated writer connection plus a small reader
  pool (WAL allows concurrent readers — <https://sqlite.org/wal.html>); drop the
  `Immediate` behavior from read transactions; key the command gate by
  `(session_id)` or drop it, since every transition is validated inside its own
  transaction (`:944-988`).
- **Disposition**: simplify (per-surface scoping, not a redesign).
- **Effort**: M (3-6 days; needs concurrency tests).
- **Confidence**: high on the mechanism; high on the direction, medium on the
  magnitude until benchmarked.
- **Cost of delay**: the M10-M12 concurrency vision is built on this store; the
  ceiling is invisible today (single user, one run at a time) and will look like
  a mysterious slowdown later.

### Z2-F06 — Unbounded full-history context, no budget, no compaction, no transcript read ·  S2

- **Stage / type**: shipped (mechanism) with committed/doc-only mitigations / LANDMINE.
- **Trigger**: a session that lives long enough for its transcript to exceed the
  model window. Every new turn rebuilds the entire history as the provider
  request, fails identically, and offers no mitigation.
- **Mechanism**: `load_starting_run_model_context` (`storage-sqlite:1185-1255`)
  returns every started user turn plus every completed run's assistant content as
  one DTO (`:1608-1690`), and `schedule_from_context` (`application:1395-1425`)
  turns that into the request. There is no token/byte/message budget anywhere in
  the request path (no such constant in `intention-model`, `application`,
  `runtime`, or the two provider drivers — verified by search), and the transport
  frame cap (`MAX_MESSAGE_BYTES`, ADR 0045) bounds wire frames, not model input.
  The DTO also materializes the whole history in memory at once.
- **Blast radius**: a session becomes unusable; users lose continuity because the
  only workaround is a new session; no export path exists (`GetSessionSnapshot`
  returns a projection with no messages, `intention-protocol/src/lib.rs:629-634,
  897-902`).
- **Committed mitigations and their dates**: compaction is an accepted direction
  owned by architecture 21 (`21:384-391`) but is delivered by **M12**; physical
  deletion/GC is an accepted post-M5 direction to be executed in M5+ slice 4
  (`architecture/04:300-306`; `11:1942`); M6 ships the first real UI.
- **Simplest alternative**: a deterministic byte/token budget with oldest-first
  elision, or an explicit compaction record, delivered with M6 rather than M12;
  plus a protocol read for the transcript so a user can see and export what is
  being truncated.
- **Disposition**: re-scope (move a minimal context policy earlier) or accept the
  one-session-per-heavy-task usage rule in writing.
- **Effort**: M (3-8 days depending on whether elision or compaction).
- **Confidence**: high on the absence of any budget; medium on how soon the
  ceiling is hit (depends on tool-result sizes, which are bounded at 64-128 KiB
  per result, `intention-tools/src/lib.rs:149-159`).
- **Cost of delay**: this is the failure that makes users distrust the product's
  durability promise — the data is safely stored and unusable.

### Z2-F07 — The session event stream is paid for on every commit and delivered to nobody  ·  S3

- **Stage / type**: shipped / THEATER.
- **Trigger**: any client that subscribes to session state. The subscription
  response is documented and coded to always carry an **empty** tail: "receives
  the current durable projection snapshot and an empty contiguous tail at that
  snapshot's sequence" (`architecture/04:191`; implementation
  `intention/src/lib.rs:811-862`, both branches construct `Vec::new()`), so the
  `after_sequence` parameter has no reachable effect on payload. Run-scoped
  subscriptions are the only live feed (M4).
- **Mechanism**: `subscribe` (`intention/src/lib.rs:811-862`) never calls
  `load_tail`; `load_tail` (`storage-sqlite:1388-1453`) is used only by the
  tool-result publication reread (`intention/src/lib.rs:247-286`, a scoped and
  therefore small tail) and by test-support. Meanwhile every commit writes the
  full transcript into `domain_events` (`:314-362`), and there is no protocol
  query that returns turn content (`intention-protocol/src/lib.rs:629-634`), so
  the durable session-event sequence currently has no client consumer at all.
- **Blast radius**: cost (a meaningful share of Z2-F01/F08's bytes) for zero
  delivered value today; API trap (a future client that trusts the tail to catch
  up silently loses updates while the response looks successful); `load_tail` is
  also unbounded (no page limit, unlike the run tail's 256 facts/512 KiB,
  `storage-sqlite:33-34, 1400-1445`), so the first serious consumer inherits an
  O(all events) read.
- **Simplest alternative**: decide the contract — either implement bounded,
  paged session-event delivery (the M6 "session-event delivery" reserved claim,
  `11` Reserved declarations) or remove the tail from the session subscription
  and keep events as internal evidence with a bounded tail read.
- **Disposition**: simplify (bound `load_tail`, page it) + defer the delivery
  decision to the milestone that needs it, recorded explicitly.
- **Effort**: S (1-2 days) for bounding + doc truth; M for real delivery.
- **Confidence**: high (implementation and doc agree; the consumer search is
  exhaustive over `crates/`).
- **Cost of delay**: M6 will be built against this seam; discovering it there
  costs UI rework.

### Z2-F08 — Content duplication and unbounded growth with no retention  ·  S3

- **Stage / type**: shipped / EXCESSIVE.
- **Trigger**: normal use over weeks. Every tool result is stored **twice**: once
  as `tool_results.content` (canonical document, up to 512 KiB, with text
  truncated to 1/8 for read/execute — `application:1162-1189, 1222-1242`,
  `storage-sqlite:588-597`) and again inside the `ToolResultRecorded` model fact
  envelope (`runtime:733-762`, stored via `model_fact_event` + `append`,
  `storage-sqlite:1087-1095, 314-362`). Every user turn is stored twice
  (`turns.content`, `:815, 864`, plus the `UserTurnAccepted` envelope,
  `:826-836`). Assistant text is stored as 4 KiB fact envelopes and again
  concatenated in the run snapshot (`:1749-1765, 447`).
- **Quantification**: a turn with 8 KiB of assistant text and 10 tool results of
  20 KiB (both within the tool bounds, `intention-tools/src/lib.rs:149-159`)
  writes ≈ 200 KiB of duplicated tool content + ≈ 76 KiB of envelope metadata
  (254 facts × ~300 B) + ~16 KiB of assistant text copies ≈ **300 KiB per turn**
  (estimate; JSON escaping adds ~5-15% for code-heavy text). At 50 turns/day that
  is ≈ 15 MB/day ≈ 5 GB/year in one SQLite file (estimate), with no VACUUM, no
  retention policy (`architecture/04:259, 300-306`), and deletion pushed to M5+
  slice 4.
- **Blast radius**: disk, backup/restore time, and — via Z2-F01 — every commit's
  scan cost.
- **Simplest alternative**: store tool content once (keep the canonical evidence
  row, reference it from the fact, or drop one of the two); keep the transcript
  in one place; add a bounded retention/compaction policy at the milestone that
  introduces the UI, not at M12.
- **Disposition**: simplify.
- **Effort**: M (2-5 days, mostly test/doc churn on the M5 tool-evidence
  contracts).
- **Confidence**: high on the duplication (two insert sites with the same bytes);
  medium on the annual figure (usage assumption stated).
- **Cost of delay**: growth is silent; the first symptom is a slow, huge database
  and a slow restore.

### Z2-F09 — Commit granularity: one fsync per 4 KiB chunk and per reasoning delta  ·  S3

- **Stage / type**: shipped / BUREAUCRACY.
- **Trigger**: any long answer or reasoning-capable model. 255 commits per turn
  in the Section 2.6 example, each fsync'd (bundled default `synchronous=FULL`,
  no pragma override at `storage-sqlite:155`) and each paying Z2-F01's cost.
- **Mechanism**: `flush_full_text` appends one fact per 4 KiB crossing
  (`runtime:1238-1261`); reasoning deltas each commit (`runtime:928-947`); one
  tool call is 5 commits across three layers (`runtime:704-740`,
  `application:408-418, 619-624, 756-761`); `abort`/`finish` add 3 more.
- **Blast radius**: per-commit latency paid inside the streaming loop (UX), fsync
  count, WAL churn, and the multiplier on Z2-F01.
- **Simplest alternative**: batch durable writes per provider round/model step
  (or time-batched, e.g. ≥ 50 ms or ≥ 64 KiB, or at each publication point) —
  "durable before publication" stays true for every externally visible unit,
  which is the property the M4 doc actually needs (`architecture/04:190`).
- **Disposition**: simplify.
- **Effort**: S (1-3 days) once Z2-F01 is addressed; the tests that assert
  per-chunk facts (`m5_tool_loop.rs`, `m4_model_execution.rs`) need updating.
- **Confidence**: high on counts; medium on the fsync claim for the exact pinned
  SQLite build (see Section 7).
- **Cost of delay**: minor per commit, significant in aggregate; it is the
  cheapest half of the Z2-F01 fix.

### Z2-F10 — An accepted durable bound is not enforced, and the code says it is  ·  S3

- **Stage / type**: shipped / MIXED.
- **Trigger**: a long reasoning stream. The runtime comment states the per-run
  reasoning bound is enforced "at the durable append authority against the
  per-run `reasoning_aggregate_bytes` accounting, which rejects the whole
  crossing batch before any write (PR24-024)" (`runtime:949-955`). No such
  function, field, or accounting exists anywhere in the workspace (verified by
  search for `validate_reasoning_fact_output_bound`, `reasoning_aggregate`,
  `MAX_REASONING`), and ADR 0041 item 3 lists the "durable per-run bound of
  4 MiB" as remaining "the append authority"
  (`docs/intention-relay/decisions/0041-same-run-reasoning-round-trip.md:91-93`).
  Only the per-fact 512 KiB bound is real and generic
  (`storage-sqlite:32, 1012-1013` enforcing `MAX_CANONICAL_FACT_BYTES`).
- **Blast radius**: an accepted intrinsic bound silently regresses; a run's fact
  count/bytes are unbounded, which feeds Z2-F01 and disk growth; a future reader
  of ADR 0041 will believe a guarantee that does not exist; the green CI is
  evidence the tests do not cover it.
- **Simplest alternative**: either implement the per-run reasoning accounting in
  the durable append path (as the ADR says) or correct ADR 0041 and delete the
  comment. Do not leave a comment that cites a nonexistent function.
- **Disposition**: simplify/correct (documentation truth first, then decide
  whether the bound is wanted — note ADR 0048's "limits by precedent" policy
  requires a precedent, and 4 MiB of reasoning has one only as an ADR decision).
- **Effort**: S (1-2 days).
- **Confidence**: high (exhaustive search).
- **Cost of delay**: comment/ADR drift is how the next agent confidently writes
  code against a guarantee that was never built.

### Z2-F11 — Dead schema surface: `sessions.config_revision_id`, snapshot `sequence` columns  ·  S4

- **Stage / type**: shipped / DEAD.
- **Trigger**: none — these are inert, and that is the point.
- **Mechanism**: `sessions.config_revision_id` is inserted as `NULL`
  (`storage-sqlite:673`) and never updated (no `UPDATE sessions SET
  config_revision_id` exists), so `SessionProjectionDto.config_revision_id`
  (`domain:419-420`) is always `None`, yet it is serialized into every session
  snapshot, returned in every protocol snapshot, and mirrored in the client.
  `model_run_snapshots.sequence` is written (`:447-455`) and never read
  (`load_model_run_snapshot` selects only `snapshot_json`, `:1783-1791`).
- **Blast radius**: small: schema/contract noise, a field every reviewer must
  re-evaluate ("session-level config selection" is not a thing anymore since M4
  selects per run).
- **Simplest alternative**: drop the column and the DTO field, per the
  single-version/no-backward-compatibility policy.
- **Disposition**: remove.
- **Effort**: S (0.5 day).
- **Confidence**: high.
- **Cost of delay**: low; but it is exactly the surface that later gets
  *reinterpreted* rather than deleted (a session-level config selection could be
  filled in by a future slice and change existing behavior silently).

### Z2-F12 — Durability-critical trait methods have `unavailable` default bodies  ·  S4

- **Stage / type**: shipped / MIXED.
- **Trigger**: a new backend, a new fake, or an accidental omission — the code
  compiles and fails at runtime with `run_history_unavailable` /
  `tool_lifecycle_unavailable` (`intention-storage/src/lib.rs:774-934`,
  default bodies at `:774-790, 792-800, 841-852, 863-870, 887-901, 904-916, 922-934`).
- **Blast radius**: silent partial implementations; the local test fakes in
  `intention-runtime/tests/*` already rely on defaults for the M5 methods, so a
  missing production path can look "unavailable" rather than non-existent.
- **Simplest alternative**: make every method required (no default body); the
  single backend and the single-user policy make defaults pure risk.
- **Disposition**: simplify.
- **Effort**: S (0.5-1 day, mostly updating test fakes).
- **Confidence**: high.
- **Cost of delay**: low now; it is how "no consumer" surfaces survive review.

### Z2-F13 — Removed queued turns leave their content durable forever  ·  S4

- **Stage / type**: shipped / DEAD (partial).
- **Trigger**: user removes a queued turn (`RemoveQueuedTurn`).
- **Mechanism**: `remove_queued_turn` deletes only the `queued_turns` membership
  (`storage-sqlite:907-943`); the `turns` row keeps `content`, `outcome='queued'`
  and `queue_ticket` (`:55-60`), and its `proposed_run_id` stays bound by the
  table's `UNIQUE (proposed_run_id)`. A retry of the same turn id then returns a
  typed `accepted_turn_removed` conflict (`:748-754`) — arguably correct — but the
  user text survives indefinitely with no reader.
- **Blast radius**: data retention the user believes they removed; small disk
  cost; one untested-looking transition (`sqlite_contracts.rs:1302` covers the
  error code).
- **Simplest alternative**: null the content on removal (keep the tombstone row
  for audit) or delete the row and drop the uniqueness reliance.
- **Disposition**: keep the tombstone, remove the content.
- **Effort**: S (0.5 day).
- **Confidence**: high on behavior; medium on whether content retention is
  intended (docs say the queue is durable, not that removal retains text).
- **Cost of delay**: low; becomes relevant when a retention/deletion policy is
  finally written and has to answer "where is all the removed text?".

### Z2-F14 — Instance-independent database path with instance-keyed locking  ·  S3

- **Stage / type**: shipped / MIXED.
- **Trigger**: running a second daemon against the same state directory, e.g.
  `intention-daemon <other-instance>` (`crates/intention-daemon/src/main.rs:13-20`
  accepts an instance id). The database path is always
  `<platform state>/intention-relay/intention-relay.sqlite`
  (`intention/src/lib.rs:1002-1006`, `DATABASE_FILENAME` at `:69`), while the
  endpoint and the client bootstrap lock are keyed by instance id
  (`intention-transport/src/lib.rs:61-108`, `intention-client/src/lib.rs:706, 716-722`),
  so two processes can hold the same file while the lock believes they are
  distinct. Writes then wait up to the driver's 5 s busy timeout and finally
  surface as `storage_unavailable` (`storage-sqlite:1974-1976` maps every SQLite
  error to the same unavailable error, dropping constraint/busy/corruption/disk
  detail; `ErrorRetryDto::Manual`).
- **Blast radius**: generic "storage unavailable" errors with no diagnosis;
  dropped-key users blocked up to 5 s per write; overlapping recovery in two
  processes (each recovery transition is its own transaction, `:1337-1371`).
- **Simplest alternative**: bind the database file to the instance id (or take an
  exclusive lock on the database at open, alongside `journal_mode=WAL`), and map
  SQLite failure classes to distinct typed causes.
- **Disposition**: simplify/harden.
- **Effort**: S (1 day, plus one test).
- **Confidence**: high on the path/lock mismatch; medium on the 5 s figure (the
  pinned 0.40.2 source was not available offline — verified in the vendored
  0.32.0, `inner_connection.rs:119`; see Section 7).
- **Cost of delay**: a bad first-run experience is expensive to undo; error
  opacity costs debugging hours every time storage misbehaves.

### Z2-F15 — No bound on tool-loop rounds; the counter silently wraps  ·  S4

- **Stage / type**: shipped / MIXED.
- **Trigger**: a model that keeps calling tools (a common failure with cheap or
  confused models, e.g. repeated failing `read`/`grep`).
- **Mechanism**: `let mut tool_round = 0u8` (`runtime:656`) is incremented once
  per tool-calling round (`:704`) with no cap; the value only gates first-round
  retry eligibility (`:656, 681`). At 256 rounds the counter
  wraps in release builds (workspace has no `overflow-checks` override and no
  release profile, `Cargo.toml:70-79`), silently switching a late failure back to
  "retryable first-round" semantics; in debug/test builds the same input panics.
  Each round adds durable facts and a provider call, so the cost of a runaway
  loop is unbounded money plus compounding Z2-F01 work.
- **Blast radius**: provider spend, run duration, fact growth; a latent panic
  path in test/dev; no typed "too many rounds" outcome exists.
- **Simplest alternative**: a typed, generous round/tool-call budget for one run
  (a liveness safeguard, which the repository's own limit policy explicitly
  permits, ADR 0048) and a wider counter; or explicit saturation with a typed
  terminal failure.
- **Disposition**: simplify (make the boundary explicit and typed).
- **Effort**: S (1 day with a fixture).
- **Confidence**: high on the mechanism; medium on how often a real model reaches
  256 rounds (rare, hence S4).
- **Cost of delay**: low; but a wrapped counter is the kind of defect that shows
  up as an unexplainable retry after a long loop.

### Z2-F16 — The queue ticket is missing from the cross-domain sequence table  ·  S4

- **Stage / type**: doc-only / BUREAUCRACY.
- **Trigger**: none — documentation completeness.
- **Mechanism**: `architecture/README.md:209-220` declares "Sequences and cursors
  are independent authorities and are never interchangeable" and lists eight;
  the M3 queue ticket — the only ordering value a user sees today — is absent,
  although `architecture/04:91` defines its monotonic never-reused rule and even
  uses it as a "must not be reused for" example in the Mandate row.
- **Blast radius**: a future Mandate/scheduler author following the table can
  legitimately conclude that no ordinary queue sequencing authority exists.
- **Simplest alternative**: add one row (owner 04, orders user queue position,
  must not be reused for run cursors or Mandate sequences).
- **Disposition**: simplify (docs).
- **Effort**: S (0.25 day).
- **Confidence**: high.
- **Cost of delay**: negligible; included because the table is the normative
  cross-domain contract this audit was asked to check.

### Z2-F17 — Doc vs code: run-cursor seeding  ·  S4

- **Stage / type**: shipped + doc claim / MIXED.
- **Mechanism**: `architecture/04:117-119` states run cursors are seeded by the
  write path "never by open-time hydration", while the schema batch executed on
  every `open()` contains `INSERT OR IGNORE INTO run_cursors(…) SELECT run_id,
  session_id, 0 FROM runs` (`storage-sqlite:100-101`, executed at `:153-159`).
  Effect is benign (the write path also seeds, and both are idempotent), but the
  document and the code disagree about a sequence-authority rule in exactly the
  area this zone is responsible for.
- **Blast radius**: review noise; a reader cannot tell which seeding rule is
  authoritative.
- **Simplest alternative**: delete the open-time seed (the write path already
  guarantees the row) or correct the doc sentence.
- **Disposition**: simplify.
- **Effort**: S (0.25 day).
- **Confidence**: high.
- **Cost of delay**: negligible.

---

## 4. Keep-list (justified, with why)

These mechanisms cost something and earn it; they should survive any
simplification pass.

1. **One-active-run invariant, enforced twice** — partial unique index
   `one_active_run_per_session` (`:66-67`) plus the typed state
   machine (`domain:74-111`). The database cannot represent the invalid state;
   that is cheap and load-bearing.
2. **Durable queue ticket: zero-based, per-session monotonic, never renumbered**
   (`storage-sqlite:854-866`, `architecture/04:91`). Correct choice of authority:
   per-session counter on a single row, incremented in the same transaction as the
   insert. Removal leaves gaps by design; promotion takes the lowest ticket.
3. **Atomic terminal promotion inside the terminal transition** (`:944-988`,
   `:461-499`), preserving the queued turn's original `proposed_run_id`,
   `ConfigRevisionId` and config snapshot. One transaction, no window where a
   queued turn can be lost or double-started; the frozen selection means a
   daemon-wide config change cannot silently rewrite what the user's queued
   message will execute under. Ordered `RunStatusChanged` before `RunStarted` is
   a real ordering guarantee for replay consumers.
4. **Per-run event cursor separate from the session event sequence**
   (`:87-101, 989-1099`) with bounded contiguous pages (256 facts / 512 KiB,
   `:33-34`), `next_after_cursor`/`has_more`, and typed conflict
   (`run_event_cursor_conflict`, `:1946-1957`). This is what makes run replay
   resumable, gap-detectable and cheap on the *read* side; the cross-domain table
   (`architecture/README.md:212-213`) documents the independence correctly.
5. **Frozen `ConfigRevisionId` per turn/run with same-id idempotency and
   different-content conflict** (`:216-254`, `:697-760`). Small code, strong
   property: what a run was authorized to do can never be re-interpreted by a
   later configuration.
6. **Two-step `Starting → Cancelling → Cancelled`, never collapsed**
   (`architecture/04:71-83`, `domain:74-98`) plus the daemon's registered
   cancellation terminalizer and the "executor owns the terminal transition"
   rule (`daemon:282-420`, `runtime:1082-1110, 1200-1236`). This is the piece that
   keeps a stop request from stranding a durable active run; it is well tested
   (`m4_streaming_foundation.rs`, `daemon` unit tests).
7. **Post-commit independent reread before publication** (`architecture/04:36`,
   `daemon:564-628`, `intention/src/lib.rs:247-286`). It is the only reason a
   live frame can be trusted to reflect committed state. Keep it — but make the
   reread O(1) (Z2-F01) instead of re-parsing the whole run snapshot.
8. **Recovery-before-ready that interrupts unfinished runs without resuming
   provider/tool/external work** (`storage-sqlite:1337-1371`, `runtime:135-138`).
   Correct and conservative given that an interrupted `execute` may already have
   had an external effect (`architecture/04:171-173`). The gap is admission of the
   promoted successor (Z2-F03), not this rule.
9. **WAL + bundled SQLite, single file, no server** (`storage-sqlite:155`). Right
   engine for the product; the problem is how it is driven, not that it is used.
10. **Fault injection at each write stage with rollback assertions**
    (`sqlite-contracts` in-module tests at `src/lib.rs:2333-2430`, `FaultPoint`
    at `:1489-1497`). Cheap, and it is the only reason the atomicity claims in
    `architecture/04:33-34` are credible. It does need to stop asserting the dead
    `run_snapshots` table (Z2-F02).
11. **Lock-order discipline** (Section 2.3): `HostData → command_gate →
    connection`, never the reverse. Worth recording as an invariant; the problem
    is the scope of the global locks, not their order.
12. **Typed liveness guards** such as `queue_promotion_required`
    (`storage-sqlite:809-813`) and `turn_identity_conflict` (`:772-786`): they
    turn what would be raw SQLite constraint failures into typed conflicts. This
    is a liveness/`no leaked internals` safeguard, not a bureaucratic cap, and is
    explicitly permitted by ADR 0048.

---

## 5. Predictions (cut / rewrite / survive, with signals and confidence)

| Prediction | Signals to watch | Confidence |
| --- | --- | --- |
| **Cut: `run_snapshots`** | the first commit that deletes a table; the fault-injection tests re-pointed at `model_run_snapshots` | high |
| **Cut: `sessions.config_revision_id`, `model_run_snapshots.sequence`** | the next schema-touching change; a DTO review that asks "who sets this?" | high |
| **Cut: the second (no-op) context build via `PrivateModelRunDispatch`** | a latency fix in the send-turn path; `dispatch_model_run` losing its production impl | high |
| **Rewrite: per-commit full-run replay/snapshot fan-out (Z2-F01)** | first profiler trace of a real long session; a p95 commit-duration metric; the first M6 UI stall; any commit that filters `snapshot_model_runs` by status | high on "will change", medium on "before M6" |
| **Rewrite: context assembly gains a budget/compaction earlier than M12 (Z2-F06)** | the first real "context length exceeded" failure; user reports about long sessions; an M6 UI that cannot show history | medium |
| **Rewrite: storage concurrency (single connection + global gate) when Mandate children arrive (Z2-F05)** | the first concurrent-child milestone (M10-M12) benchmark; append p99 under two sessions | medium |
| **Rewrite: recovery workflow for promoted successors (Z2-F03)** | any restart-with-queue bug report; the first Mandate scheduler that needs durable pending work admitted after restart | medium-high (it is a correctness hole, not a preference) |
| **Survive: queue ticket, one-active-run, config revisions, run cursors + page limits, two-step cancellation, post-commit reread, recovery-interrupts** | none of these get a rewrite in any plausible simplification | high |
| **Survive but re-scoped: the append-only session event log** (kept as internal evidence, delivered later or never to clients) | an M6 decision record on session-event delivery; `load_tail` gaining a page limit | medium |

Expected sequence of amputation (my own view of the cheapest credible path):
(1) delete Z2-F02/F11 dead surface; (2) scope snapshot fan-out by affected run and
make the projection incremental (Z2-F01 + Z2-F09); (3) add `runs.started_sequence`
and collapse context assembly to one bounded build (Z2-F04); (4) fix recovered-run
admission (Z2-F03); (5) add a context budget (Z2-F06) before the first UI ships;
(6) revisit connection/gate scope when concurrency becomes real (Z2-F05).

---

## 6. Metrics and commands used

Commands (all read-only; no build, no test execution):

```
git status --short --branch                     # baseline: clean except the named untracked file
git log --oneline -5
git log --oneline -1 -S "<symbol>" -- <path>    # provenance of snapshot_model_runs / promotion / run_snapshots
find crates -name '*.rs' -exec wc -l {} +       # sizes
rg -n "<pattern>" crates docs                   # every consumer/anchor claim
cargo metadata --no-deps --offline              # 25 workspace crates
grep -rn "<pragma|default>" <vendored libsqlite3-sys>  # bundled SQLite defaults
```

Derived metrics:

| Metric | Value | Derivation |
| --- | --- | --- |
| Workspace crates | 25 | `cargo metadata --no-deps --offline` |
| Rust LOC in `crates/` | 53,319 | `find crates -name '*.rs' -exec wc -l {} +` |
| Storage backend LOC | 2,721 (`intention-storage-sqlite/src/lib.rs`) | same |
| Statements per commit | ≈ 12 + 3R (R = runs in the session) | Section 2.4 |
| Model-fact rows parsed per commit | F (all facts of all runs of the session) | `storage-sqlite:371, 413, 1729-1782` |
| Envelope bytes parsed per commit | B (all of the session's model-fact envelopes) | same |
| Commits per turn | 3 + C + D + 5T | `runtime:178, 656-740, 928-947, 1238-1261`; `application:408-418, 619-624, 756-761` |
| Example (C=2, D=200, T=10) | 255 commits | 3 + 2 + 200 + 50 |
| Example session (R=20, F=5,000, B≈10 MB) | ≈ 72 statements, ≈ 10 MB parsed, 1 fsync per 4 KiB of text | Section 2.4 |
| Durable bytes added per example turn | ≈ 300 KiB, of which ≈ 200 KiB duplicated tool content | Section 3, Z2-F08 |
| External projections per started turn | 2 identical context builds | `application:843-915` + `daemon:1180, 156-179` |
| SQLite durability defaults | `synchronous` FULL, `wal_autocheckpoint` 1000 pages | vendored `libsqlite3-sys-0.30.0/sqlite3/sqlite3.c:13872, 17400-17406`; project sets neither (`storage-sqlite:155`) |
| Busy timeout | 5000 ms (driver default) | vendored `rusqlite-0.32.0/src/inner_connection.rs:119` |

Facts checked externally (URLs):

- WAL mode: readers and one writer; checkpointing behaviour —
  <https://sqlite.org/wal.html>.
- `PRAGMA synchronous` default FULL —
  <https://sqlite.org/pragma.html#pragma_synchronous>.
- `PRAGMA wal_autocheckpoint` default 1000 pages —
  <https://sqlite.org/pragma.html#pragma_wal_autocheckpoint>.

Claims not sourced externally are marked "general practice" in Section 2.6.

---

## 7. Uncertainty and open questions

1. **No measurements.** This audit was not permitted to build or run code, so all
   latency and throughput figures are static derivations with stated assumptions
   (JSON parse rate assumed 100-400 MB/s for these structures; fsync assumed
   0.1-1 ms on local SSD). The *directions and asymptotics* are robust; the
   absolute milliseconds are not. A one-hour experiment (insert 5,000 facts into
   one session, time one append) would settle Z2-F01's magnitude.
2. **Dependency version gap.** The pinned `rusqlite 0.40.2` / `libsqlite3-sys
   0.35.x` sources were not present in the offline registry; the bundled-default
   and busy-timeout claims were verified in the vendored 0.32.0 amalgamation.
   These defaults are long-standing, but they were not confirmed for the exact
   pinned build.
3. **Intent vs. defect on Z2-F03.** The active documents say recovery must not
   resume provider/tool work, and they describe the recovery-promoted successor as
   existing. They do not define who is allowed to execute it. I read the combination
   as an unowned gap, not a deliberate "hold forever" policy, because the hold
   policy that would have made it intentional was reverted (ADR 0044) and no
   document replaces it. The controller should decide between "admit once at
   boot", "fail typed so the queue drains", or "document a permanent hold plus a
   user-facing admit command" — and then the code must match.
4. **Intent vs. defect on Z2-F06.** "Full history as context" may be an accepted
   v1 posture for a single user who starts a new session when needed. Neither
   `architecture/04`, `08`, `21`, nor the roadmap states a v1 session-length
   rule, so the ceiling is currently undocumented rather than decided.
5. **Whether the empty session tail is a deliberate deferral.** `architecture/04:191`
   and the roadmap's reserved M6 declaration suggest yes (delivery is reserved to
   M6). If so, Z2-F07 is about paying the write cost before the consumer exists and
   about `load_tail` being unbounded, not about a wrong contract.
6. **Provider failure mapping at the context ceiling** was not traced into the two
   provider drivers in this audit (out of scope), so the exact user-visible error
   for an over-window request is unverified; the structural claim (no budget, no
   mitigation, no exit) does not depend on it.
7. **Concurrent-process scenarios** (Z2-F14) are inferred from the path/lock
   mismatch; I did not exercise two daemons.
8. **`ToolResultRecorded` content bounds** differ between the two storage copies
   (model fact ≤ 64 KiB text / ≤ 128 KiB search via the tool projection;
   `tool_results.content` ≤ 512 KiB with a 1/8 text budget, `application:1162-1242`).
   I did not verify every branch of `canonical_tool_result_document`, so the
   duplication factor is "≈ 2 copies", not an exact byte equality.

---

## 8. Summary of the zone

The shipped data layer gets the *invariants* right — one active run, monotonic
queue tickets, frozen per-run configuration, run cursors with bounded pages,
two-step cancellation, recovery that never resumes external work — and gets the
*write economics* wrong: every commit replays the session's entire fact history
and rewrites a snapshot for every run, including runs that are already terminal;
a durable table exists only to be asserted by tests; the session event stream is
paid for on every write and delivered to nobody; context assembly rebuilds the
whole history twice per turn and has no ceiling; and a recovery-promoted queued
turn becomes a run that nothing will ever execute, wedging the session until the
user stops it. Under the documents' own future load (16 concurrent children,
16,384-node fork trees, Mandate schedulers) the store's single connection, single
global mutex, and per-commit full replay are the structural wall — and none of
that is visible today because a single user runs one run at a time.

Counts: 17 findings — 3 LANDMINE (1 S1, 2 S2/S1), 3 EXCESSIVE, 2 DEAD,
1 THEATER, 2 BUREAUCRACY, 6 MIXED; 16 shipped, 1 doc-only;
12 keep-list entries.


---

# Appendix C — Zone 3: consumer surfaces — protocol, projections, adapters

Baseline: `main` @ `3291e50`, working tree clean except untracked files the audit
was told to leave alone. Read-only: no tracked file was modified, nothing was
staged, no build or test was run. Every repo claim below cites `file:line` at
that baseline.

---

## 1. Scope and method

### 1.1 Zone

Zone 3 is everything a consumer must touch to present and operate the product:
the local protocol (`crates/intention-protocol`), the shared client
(`crates/intention-client`), the presentation adapters (`crates/intention-tui`,
`crates/intention-tauri`), the domain DTOs that cross the process boundary
(`crates/intention-domain`, `crates/intention-types`), and the daemon/composition
code that answers them (`crates/intention-daemon`, `crates/intention`). The
audited question is not "is it well typed" but "what does a UI engineer actually
have to build, and what does the daily user actually see".

Project conditions that set the denominator: one local user, one machine, one
daemon, SQLite, no deployed users, no external consumers, backward compatibility
explicitly not required (`AGENTS.md`). The existence proof of the intended
product is the legacy baseline
(`docs/intention-relay/legacy-baseline/01-frontend-surface.csv`): a desktop
chat UI with sessions, projects, history panel, questions, permissions, plan
approval, usage, todos, sub-agents.

### 1.2 Sources read

- Code: `crates/intention-protocol/src/lib.rs` (1919 lines),
  `src/jsonrpc.rs` (849), `crates/intention-client/src/lib.rs` (794),
  `crates/intention-tui/src/lib.rs` (46) and `tests/tui_contract.rs` (81),
  `crates/intention-tauri/src/lib.rs` (5), `crates/intention-domain/src/lib.rs`
  (1770), `src/model_facts.rs` (1005), `crates/intention-types/src/lib.rs`
  (851), `crates/intention/src/lib.rs` (2247), `crates/intention-daemon/src/lib.rs`
  (2152), `crates/intention-storage-sqlite/src/lib.rs` (2720),
  `crates/intention-application/src/lib.rs` (1629),
  `crates/intention-transport/src/lib.rs` (1662), `crates/intention-tools/src/lib.rs`
  (2221).
- Tests used as evidence of what is actually reachable:
  `crates/intention-client/tests/run_stream_contract.rs` (647),
  `crates/intention-protocol/tests/*.rs` (800 + 217 + 159),
  `crates/intention-daemon/tests/real_api_e2e.rs` (1614),
  `crates/intention-daemon/tests/m4_streaming_foundation.rs`,
  `crates/intention-tui/tests/tui_contract.rs`.
- Docs: architecture 02, 03, 13, 21, 22, 23, 24, 29, the roadmap (M5+ and
  M6-M9), `README.md`, `legacy-baseline/01-frontend-surface.csv`,
  `legacy-baseline/06-user-flows.md`, `quality/architecture.toml`.

### 1.3 Stage vocabulary

- `shipped` — M0-M5 code in the tree.
- `committed` — M5+ or M6-M9 approved documentation and reserved code paths.
- `doc-only` — M10-M12 or unactivated directions.

### 1.4 Method

For each representative consumer flow I traced the exact consumer sequence
against the code, counted the round trips and entity joins, and asked what a
competent team building the minimum working product for one local user would
have to add or delete. Comparative practice claims are general practice unless
a URL is given (no external URLs were needed; the comparison targets are the
legacy baseline in-repo and standard local-IPC client shapes).

### 1.5 What this audit is not

It does not evaluate transport security, storage schema quality, or the Mandate
track, except where they directly change what a consumer can see. It does not
repeat the closed M4-era decisions; it judges their residue as a surface.

---

## 2. Vectors examined → verdicts

### 2.1 The shipped consumer contract, in numbers

| Thing | Count | Evidence |
| --- | --- | --- |
| Protocol methods | 8 | `ProtocolMethodDto::ALL`, `intention-protocol/src/lib.rs:1289-1298` |
| Queries | 2 (`daemon.health`, `session.snapshot`) | `lib.rs:629-634` |
| Commands | 4 (`session.create`, `turn.send`, `turn.remove`, `run.stop`) | `lib.rs:613-624` |
| Subscriptions | 2 (`session.subscribe`, `run.subscribe`) | `lib.rs:197-330, 1234-1241` |
| Notifications | 1 (`run.frame`) | `lib.rs:1260` |
| Methods reachable through `intention-client` | 4 of 8 (`health`, `session.snapshot`, `session.subscribe`, `run.subscribe`) | `intention-client/src/lib.rs:100-274, 285-333` |
| User actions reachable through `intention-client` | 0 | no `create_session`/`send_turn`/`stop_run`/`remove_queued_turn` method exists |
| Methods reachable through the TUI proof | 2 (`health` + `session.subscribe`) | `intention-tui/src/lib.rs:30-45` |
| Rendering dependencies of the "TUI" | 0 | `intention-tui/Cargo.toml` has no terminal crate |
| Executables in the workspace | 1 (`intention-daemon`) | `find crates -name main.rs` |
| Distinct error codes in production source | 162 (derivation in §6) | `ErrorDto::*` first-argument `rg` |
| `intention-tauri` production API | 0 lines (compile-only skeleton) | `crates/intention-tauri/src/lib.rs:1-5` |

### 2.2 Vectors, one verdict each

| Vector | What the consumer must do today | Verdict |
| --- | --- | --- |
| Session list (first screen) | Cannot. No list operation exists (`lib.rs:629-634`); the durable tables carry no session timestamps or titles (`intention-storage-sqlite/src/lib.rs:45-65`). | FAIL — capability absent |
| Create / open a session | Adapter must mint `ProjectId`, `SessionId`, `WorkspaceId` and send `session.create`; `intention-client` cannot send it (`intention-client/src/lib.rs:100-274`). Reuse of an existing workspace root needs the previously issued `WorkspaceId`, or the daemon answers `workspace_root_conflict` (`intention-storage-sqlite/src/lib.rs:644-658`). | FAIL — client path absent + hidden identity join |
| Conversation timeline | `session.snapshot` returns mode, workspace root, active run, queued turns with content and `at_sequence` — no user turns, no assistant turns, no tool records (`intention-domain/src/lib.rs:412-425`). `session.subscribe` returns the same snapshot plus an always-empty tail (`intention/src/lib.rs:838-859`). Run content is `ModelRunProjectionDto` only: assistant text, usage, finish, failure (`intention-storage-sqlite/src/lib.rs:1729-1781`; the projection resets its text when a new assistant turn id appears, `lib.rs:1758-1760`, and the current runtime mints one id per run, `intention-runtime/src/lib.rs:546`). | FAIL — user side and tool records unreachable |
| Live streaming | Subscribe yields a replay at the current cursor (`intention-storage-sqlite/src/lib.rs:1260-1272`), then `run.frame` messages. `intention-client` consumes frames internally and returns only `Option<RunResyncDto>` (`intention-client/src/lib.rs:344-364`); the reducer keeps snapshot + cursor + a reasoning string (`lib.rs:454-463`); the daemon re-sends the whole snapshot only on status change (`intention-daemon/src/lib.rs:616-623`). | FAIL — no incremental content reaches an adapter |
| Tool activity | Facts (`ToolCallRecorded`, `ToolResultRecorded`) exist on the wire and are discarded by the client; the snapshot has no tool list; `tool_results` has no production reader (`intention-storage-sqlite/src/lib.rs:1131`, callers only in `mod tests`, `lib.rs:2018`). | FAIL — renderable only by bypassing the client |
| History / replay after restart | Run replay tail is always empty (`intention-storage-sqlite/src/lib.rs:1260-1272`); session event tail is always empty (`intention/src/lib.rs:811-860`); no method lists runs. A past run is addressable only if the adapter still knows its `RunId`; the daemon's run id is derived from the client's turn id (`intention/src/lib.rs:900-906`), a derivation the architecture forbids consumers to make. | FAIL — history not recoverable |
| Questions / approvals | `RunStatusDto::WaitingInput` is declared with transitions and storage mapping but has no producer anywhere in the tree (`intention-domain/src/lib.rs:43,84-92`; grep shows only domain/storage occurrences). `ToolId::AskUser` is a reserved slot with no schema (`intention-tools/src/lib.rs:1141-1154`); the daemon refuses to decode it (`intention-daemon/src/lib.rs:913-920`). No protocol method answers a question or a permission prompt. | FAIL — the legacy `question.answer` / `permission.respond` / `plan.confirm` surfaces have no successor |
| Branching | `session_fork_v1` is doc-only (`architecture/23`, roadmap 493-502). | doc-only, not yet a consumer cost |
| Error states | `ErrorDto` carries code/category/retry/safe message (`intention-types/src/lib.rs:443-453`), and the client maps transport/protocol/daemon errors to typed values. But 162 distinct codes exist with no closed list, and the messages are English constants — a localized desktop UI cannot derive its own text without mapping codes that are not enumerated anywhere. | PARTIAL |
| First run | The daemon reads `~/.config/intention-relay/config.toml` and fails without it (`crates/intention/src/lib.rs:962-983`); the launcher spawns the child, drops the handle without waiting, and never captures or forwards the daemon's stderr (`intention-client/src/lib.rs:70-87`); the client reports only `local_daemon_unavailable` after a 3 s retry loop (`lib.rs:259-273`). The daemon prints only the error code to stderr (`intention-daemon/src/main.rs:24-31`). | FAIL — opaque dead end |
| Multi-window / two live sessions | One connection serves at most one run subscription; a second `run.subscribe` silently evicts the first (`intention-daemon/src/lib.rs:1051,1095-1112`). A peer that cannot keep up is evicted with a `SubscriberTooSlow` resync (`lib.rs:45,630-663`). `intention-client` has no reconnect or re-subscription logic (module doc claims it, `intention-client/src/lib.rs:1-5`). | FAIL — silent stream loss |

### 2.3 Counted consumer trace — "send one turn, watch the answer"

Steps a consumer must take with the shipped crates, counted:

1. Build `LocalEndpoint` (platform default or named instance) —
   `intention-transport/src/lib.rs:101-103`.
2. Choose the daemon program string for `ProcessDaemonLauncher`
   (`intention-client/src/lib.rs:52-67`); nothing discovers or bundles the
   binary.
3. `IntentionClient::new` (`lib.rs:106-117`).
4. `connect_or_bootstrap()` → connect + `hello` + `daemon.health`
   (`lib.rs:130-147,199-234`).
5. Mint `ProjectId`, `SessionId`, `WorkspaceId` and pick a workspace root.
6. `session.create` — **not available in the client**; the daemon's own e2e
   test hand-rolls the request path with public transport+protocol APIs
   (`intention-daemon/tests/real_api_e2e.rs:685-701`).
7. `turn.send` — same problem; read `Started { run_id, .. }` from
   `SendUserTurnAcceptedDto` (`intention-protocol/src/lib.rs:761-817`).
8. `RunStreamClient::new` + `subscribe` (connect + hello + request + reply) —
   `intention-client/src/lib.rs:285-333`.
9. Loop `receive()`; each call returns `None` for a batch, and the facts inside
   are dropped (`lib.rs:357-364,522-587`).
10. Discover there is no text; call `request_replay()` after each batch to pull
    a fresh cumulative snapshot (2 round trips and a full snapshot per chunk) —
    `lib.rs:379-401`.
11. Re-render the accumulated text.
12. To know when a queued turn starts, poll `session_snapshot` — no push exists
    (`intention/src/lib.rs:811-860`); each poll opens a new connection + hello
    (`intention-client/src/lib.rs:236-245`).

So: 12 adapter steps, 2 of them requiring an unsupported bypass of the
documented sole ingress, and step 10 is an accidental polling loop rather than
a contract. A team building M6 as specified would spend its first days on the
client and protocol instead of the UI.

### 2.4 Core question 1 — daily use: the first obstacle

The first obstacle is that the product has no consumer entry point at all: the
only compiled binary is the daemon (`find crates -name main.rs` →
`intention-daemon`), and the only client crate exposes 4 of 8 methods and no
user action. The second obstacle arrives the moment a UI is written against the
wire: **the answer cannot be streamed**. Facts carry text, but the supported
client discards them and the snapshot is refreshed only on status change, so a
normal run shows nothing until it completes. The third obstacle is history: on
any restart, the user's own messages, tool activity, and earlier runs are not
recoverable through any operation.

### 2.5 Core question 2 — full trace (user, data, request)

For "yesterday I asked the agent to refactor X, what did it do?":

- Operations available: `session.snapshot` (needs a session id the user must
  still know), `run.subscribe` (needs a run id the user must still know),
  `daemon.health`.
- Facts available: assistant text of one run's single assistant turn, usage,
  finish reason, failure code, run status.
- Facts missing: every user message, every tool call and result as structured
  data, reasoning (except an overlap window), queued-turn history, session
  list, run list.
- Human interventions if the client kept its own store: none (the client can
  render what it captured live). If it did not (new machine, new client,
  reinstall, second window): no path — the data is in SQLite
  (`turns.content`, `domain_events.envelope_json`, `model_run_facts`,
  `tool_results`) but no protocol operation exposes it.

### 2.6 Core question 3 — simplest-sufficient-world

For one local user, the minimum sufficient consumer contract is five surfaces:

1. `session.list` — projects, sessions, last activity, title seed.
2. `session.timeline` — paged durable turns (user text, assistant text, tool
   call + result summary, status changes) at session sequence order. Storage
   already holds every ingredient (`sessions`, `turns`, `domain_events`,
   `model_run_facts`, `tool_results`); the missing piece is a projection plus
   per-session timestamps/titles.
3. One live feed per open session — session events pushed as frames using the
   already-defined `SessionEventTailBatchDto`/`EventEnvelopeDto` shape, with the
   existing sequence + resync semantics. This subsumes today's run stream for
   presentation purposes and removes the need for two parallel subscription
   mechanism.
4. A client that can send commands and that surfaces typed frames (not cursors)
   to the adapter, with re-subscribe/backoff handled inside the client, as
   architecture 03 already claims (`03-daemon-transport-and-adapters.md:168`).
5. A REPL binary, because a REPL is the cheapest possible daily-use surface and
   the best adapter-isolation proof.

With those five, a chat/terminal client is a normal MVC problem. Run-cursor
token deltas can stay as an optional high-frequency channel; they are not the
minimum.

### 2.7 Core question 4 — comparative practice

- Chat clients (general practice): session list + paged transcript + a live
  event stream per open session, with client-side reconnect that re-fetches a
  snapshot at the last accepted position. Tool calls are transcript items, not
  a separate stream.
- Terminal agent clients (general practice, e.g. the class of local coding
  agents): one process, one event stream, transcript persisted server-side and
  replayed on attach.
- The legacy in-repo baseline shows exactly this shape: `session.create`,
  `session.open_history`, `gateway.getSession`, `chat.send`, `chat.stop`,
  streamed tool chunks into the message list, `question.answer`,
  `permission.respond`, `panel.history`, `status.usage`, `status.todos`
  (`legacy-baseline/01-frontend-surface.csv`; flow diagram
  `legacy-baseline/06-user-flows.md`).
- What the current design adds beyond practice is not capability but a second
  clock (session sequence vs run cursor), a second subscription mechanism, and
  a reducer that hides its own payload from the adapter.

### 2.8 Core question 5 — future amputation

Signals that will force cuts or rewrites are collected in §5. The short form:
`session.subscribe` as a read duplicate, the always-empty replay tails, the
client-side cursor-only stream API, and the doc-only UI families are the
candidates that will be cut or rewritten the first week a real UI is built.

---

## 3. Findings (ranked cards)

### Z3-F01 — Live streaming never reaches an adapter

- **ID / stage / type / severity**: Z3-F01, `shipped`, LANDMINE, **S1**.
- **Trigger**: A user sends a turn in the M6 desktop UI and watches the answer;
  or any second adapter attaches mid-run.
- **Mechanism**: `RunStreamSubscription::receive()` returns
  `DtoResult<Option<RunResyncDto>>` and applies frames internally
  (`intention-client/src/lib.rs:344-364`). The reducer keeps only
  `snapshot`, `last_cursor`, `reasoning_content`,
  `historical_reasoning_cursors`, `history_unavailable`
  (`lib.rs:454-463`). `apply_live_batch` appends reasoning only for facts at or
  below the snapshot cursor and otherwise just advances the cursor
  (`lib.rs:547-587`); the snapshot field is updated only by a `Snapshot` frame
  (`lib.rs:528-534`). The daemon broadcasts a `Snapshot` frame only when the
  run status differs from the last published status
  (`intention-daemon/src/lib.rs:616-623`), while text deltas arrive as
  `LiveBatch` facts with no status change. Net effect: assistant text is
  visible to the adapter only at subscribe time and at each status change —
  in the common case, the whole answer appears when the run completes.
- **Blast radius (UX)**: The product's primary screen does not stream. For a
  long answer the user watches a spinner, then the entire text at once; for an
  errored or cancelled run the partial text is never shown. Adapters must
  either poll `request_replay()` per batch (2 round trips and a cumulative
  snapshot per 4 KiB chunk) or bypass `intention-client` entirely, which the
  daemon's own live e2e test does (`real_api_e2e.rs:929-1009`).
- **Evidence that no test catches it**: the client tests assert only
  `last_cursor` and `reasoning_content`
  (`intention-client/tests/run_stream_contract.rs:389-397,631-645`), and their
  fixtures contain no `AssistantContentAppended` fact at all.
- **Simplest alternative**: expose the applied frame (or at least the ordered
  facts of a batch) on the subscription API and keep the reducer as a
  convenience; or, minimally, broadcast a `Snapshot` frame per durable commit
  so the existing snapshot-based rendering path works.
- **Disposition**: simplify + fix. **Effort**: S-M (expose deltas: ~1-2 days;
  atomic snapshot-per-commit with bounded projection: ~1-3 days).
  **Confidence**: high (code + tests read; no run performed).
  **Cost of delay**: every day M6 is built on this API is a day of workaround
  that becomes the interface.

### Z3-F02 — There is no consumer-visible history: user text, tool records, and past runs are unreachable

- **ID / stage / type / severity**: Z3-F02, `shipped`, LANDMINE, **S1**.
- **Trigger**: Reopen the app, open a second window, or ask "what did we do
  yesterday".
- **Mechanism**: The only durable reads are `session.snapshot` and
  `run.subscribe` (`intention-protocol/src/lib.rs:629-634,1234-1241`).
  `SessionProjectionDto` carries no message content
  (`intention-domain/src/lib.rs:412-425`); `session.subscribe` returns
  snapshot + empty tail in both branches (`intention/src/lib.rs:838-859`);
  `load_current_run_replay` returns `RunEventTailPageDto::empty`
  (`intention-storage-sqlite/src/lib.rs:1260-1272`); the run snapshot keeps
  only assistant text plus usage/finish/failure, with a reset rule that the
  current runtime never triggers (`lib.rs:1729-1781,1758-1760`;
  `intention-runtime/src/lib.rs:546`); `tool_results` has no production reader
  (`lib.rs:1131`, callers only under `mod tests` at `lib.rs:2018`); the M5
  `DomainEventDto::ToolResultRecorded` variant is never produced (grep of
  `DomainEventDto::` constructors outside the domain crate).
- **Blast radius (UX)**: A coding agent without a transcript loses its main
  value: the user cannot review what was decided, which files were touched, or
  what a tool returned, after the session view is gone. It also makes the
  "second window" and "restart" flows unrecoverable, because a run is
  addressable only by an id the adapter must have remembered — and the only
  documented way to obtain that id is the `turn.send` acceptance reply.
- **Simplest alternative**: one paged `session.timeline` read plus
  `session.list`. All source data is already durable (`turns.content`,
  `domain_events.envelope_json`, `model_run_facts`, `tool_results`,
  `sessions`); the missing pieces are a projection DTO, per-session
  timestamps/titles, and the two methods.
- **Disposition**: re-scope M5+ slice 4 / M6 to deliver these two reads before
  any UI polish. **Effort**: L (~3-8 days for projection + methods + tests,
  given storage already holds the data). **Confidence**: high.
  **Cost of delay**: the UI will invent an adapter-local store, which
  architecture 03 forbids ("Tauri, TUI, and REPL own only presentation, user
  input adaptation, local display state, and reconnect UX",
  `03-daemon-transport-and-adapters.md:20`) and which will then have to be
  migrated into the daemon anyway.

### Z3-F03 — The shared client cannot perform a single user action, and the product has no client executable

- **ID / stage / type / severity**: Z3-F03, `shipped`, LANDMINE, **S1**.
- **Trigger**: The first day someone tries to use the product rather than test
  it.
- **Mechanism**: `IntentionClient` exposes `new`, `connect_or_bootstrap`,
  `health`, `session_snapshot`, `subscribe`
  (`intention-client/src/lib.rs:100-274`). `RunStreamClient` exposes
  `subscribe` (`lib.rs:285-333`). No method sends `session.create`,
  `turn.send`, `turn.remove`, or `run.stop`. `request_on` is private and
  hard-codes request id 1 (`lib.rs:247-257`). The workspace's only binary is
  the daemon (`crates/intention-daemon/src/main.rs`; `find crates -name main.rs`
  returns one path). The daemon's own end-to-end tests therefore re-implement
  the client request path with public transport/protocol APIs
  (`intention-daemon/tests/real_api_e2e.rs:685-701`, comment: "replicating the
  shared client's private request path").
- **Blast radius (human-days)**: Every consumer must write ~20 lines of
  transport+hello+dispatch plumbing per action, or the client must grow. The
  M6 deliverable "minimal Svelte UI to create/open a session, send a turn"
  (`11-implementation-roadmap.md:693`) is impossible through the documented
  sole ingress (`03-daemon-transport-and-adapters.md:162-171`). The TUI "proof"
  reaches only 2 of 8 methods and renders nothing
  (`intention-tui/src/lib.rs:30-45`; `intention-tui/tests/tui_contract.rs:34-64`),
  so the architecture's own readiness rule — "a feature is not ready unless a
  Tauri bridge and TUI/REPL can invoke its same public command/query DTOs"
  (`02-dto-and-contract-policy.md:189`) — is unmet for every M3-M5 feature.
- **Simplest alternative**: add typed `create_session`, `send_turn`,
  `stop_run`, `remove_queued_turn` to `IntentionClient` over one reusable
  connection, and ship a REPL binary as the cheapest real adapter.
- **Disposition**: fix (client completion) + add REPL. **Effort**: M
  (~1-3 days for the four methods and their contract tests; ~1-2 days for a
  minimal REPL). **Confidence**: high. **Cost of delay**: the first consumer
  written will set the de-facto pattern, and it will be the bypass.

### Z3-F04 — Two replay contracts are unreachable: the tails are always empty

- **ID / stage / type / severity**: Z3-F04, `shipped`, THEATER, **S2**.
- **Trigger**: Any consumer that tries to replay history, which the docs tell
  it to do on reconnect.
- **Mechanism**: `ApplicationFacade::subscribe` constructs
  `SessionEventTailBatchDto::new(..., Vec::new())` in both branches
  (`intention/src/lib.rs:838-859`); `SqliteStorageRepository::load_current_run_replay`
  answers `RunEventTailPageDto::empty` (`intention-storage-sqlite/src/lib.rs:1260-1272`),
  which is the only replay source for `run.subscribe`
  (`intention-daemon/src/lib.rs:706-729,752-765`). Therefore
  `RunReplayDto`, `RunEventTailPageDto` (`has_more`, 256-fact page, 512 KiB
  canonical bound at `lib.rs:32-34,853-899`), the client's contiguous-tail
  validation and "incomplete tail" rejection
  (`intention-client/src/lib.rs:489-514,589-618`;
  `intention-client/tests/run_stream_contract.rs:276-318`), and the
  `SessionEventTailBatchDto` contiguity rules
  (`intention-protocol/src/lib.rs:1015-1082`) all govern data that never
  exists. The M3 reconnect contract in architecture 03 ("snapshot plus event
  tail") describes the same empty path
  (`03-daemon-transport-and-adapters.md:235-263`).
- **Blast radius**: engineering time and false confidence. A UI team
  implementing the documented reducer gains nothing; a reviewer reading the
  tests believes replay is covered. It also blocks the honest reconnect story:
  the tail is the mechanism that makes reattach cheap, and it can never carry
  a byte.
- **Simplest alternative**: either implement the tail (storage already has
  `load_tail`/`load_run_tail`, `intention-storage-sqlite/src/lib.rs:1274-1335,1388-1454`)
  or delete the unreachable DTOs and their validation under the project's own
  no-backward-compatibility rule (`AGENTS.md`, "When an execution path becomes
  outdated, remove it").
- **Disposition**: implement the session tail for presentation (see §2.6), keep
  the run tail but bound its page size; delete the client-side validation for
  the empty case if the run tail stays replay-only.
  **Effort**: S (delete) / M (implement). **Confidence**: high.
  **Cost of delay**: every new consumer re-derives the same dead end.

### Z3-F05 — Workspace reuse is a hidden identity join with no recovery path

- **ID / stage / type / severity**: Z3-F05, `shipped`, LANDMINE, **S2**.
- **Trigger**: A second session in a folder the user has used before, after
  client-local state is lost (reinstall, second adapter, new machine).
- **Mechanism**: `CreateSessionCommandDto` requires the caller to supply
  `project_id`, `session_id`, `workspace_id`, and `workspace_root`
  (`intention-domain/src/lib.rs:182-239`) — despite the constructor doc calling
  them "daemon-owned stable identities" (`lib.rs:192`). The storage layer binds
  root ↔ workspace id uniquely and rejects a second identity for the same root
  with `workspace_root_conflict` (`intention-storage-sqlite/src/lib.rs:644-658`);
  the daemon's own e2e fixture documents that a second session "must reuse the
  workspace identity" (`intention-daemon/tests/real_api_e2e.rs:703-707`). No
  operation lists projects, workspaces, sessions, or roots
  (`intention-protocol/src/lib.rs:629-634`).
- **Blast radius (UX)**: "New session in this project" fails for every user who
  does not carry an adapter-private registry; the only recovery is editing or
  deleting the SQLite file. That is exactly the flow M6 lists first
  (`11-implementation-roadmap.md:693`).
- **Simplest alternative**: let the daemon own workspace identity for a root
  (bind-or-create by canonical root), or add a `workspace.lookup`/`project.list`
  read.
- **Disposition**: simplify (daemon-owned binding). **Effort**: S-M (~1-2 days
  with tests). **Confidence**: high. **Cost of delay**: the workaround will be
  an adapter-owned durable registry, which contradicts the adapter boundary
  (`03-daemon-transport-and-adapters.md:20`) and will not survive a reinstall.

### Z3-F06 — The cumulative run snapshot is unbounded: expensive to build and impossible to deliver past ~1 MiB

- **ID / stage / type / severity**: Z3-F06, `shipped`, LANDMINE, **S3** (S2 for
  users who hit the cap).
- **Trigger**: A long answer (a large code dump, a long report) or many tool
  rounds in one run.
- **Mechanism**: On every committed fact append the storage transaction
  re-reads *all* facts of the run, re-concatenates the whole assistant text and
  rewrites `model_run_snapshots.snapshot_json`
  (`intention-storage-sqlite/src/lib.rs:406-459`; `model_projection`
  `lib.rs:1729-1781`; called from the append transaction,
  `lib.rs:601-609`). The cost grows quadratically with run length. The same
  snapshot is then delivered whole in a `RunSnapshotFrameDto`
  (`intention-protocol/src/lib.rs:493-534`), whose NDJSON line must fit the
  1 MiB transport cap; `encode_message` rejects anything larger
  (`intention-transport/src/lib.rs:33,698-711`), and the daemon's write path
  tears the connection down on that error
  (`intention-daemon/src/lib.rs:1136-1144`). Assistant text has no total bound
  (only a 4 KiB per-append bound, `intention-domain/src/model_facts.rs:11,365-384`;
  flush loop `intention-runtime/src/lib.rs:1238-1261`).
- **Blast radius (perf + UX)**: a long run slows its own token stream (snapshot
  rebuild is on the commit path) and, past ~1 MiB of accumulated text, the
  subscription dies with a connection error precisely when the user wants the
  result; the text remains in SQLite with no protocol path to read it (see
  Z3-F02).
- **Simplest alternative**: bound the projection — keep the last N KiB of
  assistant text plus a `truncated` flag (the pattern the tool layer already
  uses, `intention-daemon/src/lib.rs:941-969`), and let a pageable timeline
  serve the rest. Incremental snapshot maintenance would also remove the
  quadratic rewrite.
- **Disposition**: simplify. **Effort**: M (~2-4 days). **Confidence**: medium
  (derived from code; no >1 MiB run was executed).
  **Cost of delay**: silent, data-shape-dependent failure that unit tests with
  short fixtures will never reach.

### Z3-F07 — The planned UI foundation is large and is the declared prerequisite of the minimal UI

- **ID / stage / type / severity**: Z3-F07, `committed`, EXCESSIVE (risk), **S3**.
- **Trigger**: M6 planning.
- **Mechanism**: Milestone 6 asks for a "minimal Svelte UI" that also renders
  "safe activity/notification/acknowledgement projections"
  (`11-implementation-roadmap.md:693`) and consumes contracts delivered by the
  M5+ UI-foundation slice
  (`11-implementation-roadmap.md:493-502,696-700`). That slice carries session
  branching, activity/notification, reasoning delivery, RLM packaging, export,
  cross-workspace clone/rebind, limits classification, and retention/GC policy.
  Architecture 24 alone specifies 17 journal record kinds, 4 direct-message
  kinds, 2 journals plus an acknowledgement aggregate, and 16 closed failure
  codes for a single local user
  (`docs/intention-relay/architecture/24-activity-ui-and-adapters.md`,
  "Messages, safe projections, and journal", "Notifications and
  acknowledgement").
- **Blast radius (human-days)**: the daily-use surface (one session, streaming
  text, transcript) waits behind four contract families whose value at v1 is
  mostly observability for multi-agent trees that do not exist yet. The
  deliverables themselves are well bounded and additive (no second listener, no
  new authority) — the cost is sequencing, not design quality.
- **Simplest alternative**: ship the minimum consumer contract of §2.6 first;
  keep activity/notification as an additive family after the single-session UI
  works. "Needs you" can start as a derived flag over run status.
- **Disposition**: re-scope (sequence, do not delete). **Effort**: n/a
  (planning). **Confidence**: medium — the slice is documentation-only, and a
  strong team could implement it faster than the text implies; the risk is
  demonstrated by the deliverable list, not by code.
  **Cost of delay**: months of distance between the current state and any
  daily-use surface.

### Z3-F08 — `session.subscribe` duplicates `session.snapshot`, and its mandatory `requested_mode` is never read

- **ID / stage / type / severity**: Z3-F08, `shipped`, BUREAUCRACY, **S3**.
- **Trigger**: An adapter engineer choosing between two reads for the same
  data.
- **Mechanism**: `session.subscribe` answers snapshot + empty tail
  (`intention/src/lib.rs:811-860`); `session.snapshot` answers the same
  `SessionSnapshotDto` (`lib.rs:789-803`). The only differences are a resync
  variant that fires on failure and a mandatory `requested_mode: RunModeDto`
  field on every request (`intention-protocol/src/lib.rs:199-277`) that no
  production code reads (`rg requested_mode` matches only the DTO definition
  and accessor). `after_sequence` is accepted and ignored beyond an
  out-of-range check.
- **Blast radius**: duplicated client paths and a wire field every adapter must
  populate to be accepted, which cannot affect behavior. Small, but it is the
  first thing a consumer reads and it teaches the wrong model of the protocol.
- **Simplest alternative**: keep `session.snapshot`; make `session.subscribe`
  the live feed it will have to become (Z3-F04), or delete it.
- **Disposition**: collapse. **Effort**: S (<1 day). **Confidence**: high.
  **Cost of delay**: low; the duplication will be copied into M6.

### Z3-F09 — The client opens a new connection for every request, and session-level state has no push path

- **ID / stage / type / severity**: Z3-F09, `shipped`, EXCESSIVE, **S3**.
- **Trigger**: A UI that must know when a queued turn starts, when a run ends,
  or when another client changes the session — all of which are invisible
  without polling.
- **Mechanism**: `IntentionClient::request` creates a fresh `LocalConnection`,
  negotiates `hello`, sends one request and drops the connection
  (`intention-client/src/lib.rs:236-257`); `connect_ready` adds a `daemon.health`
  round trip (`lib.rs:199-234`). There is no session-level notification
  (`run.frame` is the only notification method,
  `intention-protocol/src/lib.rs:1260`; architecture 03 states plainly "there
  is no session push channel", `03-daemon-transport-and-adapters.md:68`).
- **Blast radius (perf, UX)**: a 1 Hz snapshot poll is 1 socket + 1 hello + 1
  query per second; run/queue transitions are only visible on the poll
  boundary, so the UI lags and the daemon carries connection churn. Cheap
  locally, but structural: it is the only supported way to observe
  session-level change.
- **Simplest alternative**: one reusable request connection in the client plus
  a session-level `session.frame` notification (which the timeline feed of
  §2.6 gives for free).
- **Disposition**: simplify. **Effort**: M (~2-3 days including reconnect).
  **Confidence**: high. **Cost of delay**: low individually; it compounds with
  Z3-F01/F02.

### Z3-F10 — One connection carries one run subscription, silently replaced; eviction and reconnect are not handled by the client

- **ID / stage / type / severity**: Z3-F10, `shipped`, LANDMINE (edge), **S3**.
- **Trigger**: Two live sessions in one window; a laptop sleep; a long reasoning
  burst.
- **Mechanism**: The serve loop keeps `registered: Option<(RunKey, u64)>` and
  replaces it, removing the previous subscriber with no frame or error to the
  peer (`intention-daemon/src/lib.rs:1051,1095-1112`). The per-subscriber queue
  holds 64 messages and an overflowing peer is evicted with a
  `SubscriberTooSlow` resync (`lib.rs:45,630-663`). `intention-client` contains
  no reconnect, backoff, or re-subscription code beyond the 3-second startup
  retry loop (`intention-client/src/lib.rs:259-273`), although its module doc
  and architecture 03 claim reconnect behavior as client-owned
  (`lib.rs:1-5`; `03-daemon-transport-and-adapters.md:162-171`). The e2e fixture
  must treat a reasoning burst eviction as an expected observation
  (`intention-daemon/tests/real_api_e2e.rs:993-1006`).
- **Blast radius (UX)**: a background run's output stops without a typed signal
  the adapter can rely on (the connection simply closes on eviction paths);
  multi-pane layouts need one connection per run, undocumented in the client
  contract.
- **Simplest alternative**: document and support N subscriptions per connection
  (or an explicit "replaces previous" error), and put redial + resubscribe in
  the client, as the docs already promise.
- **Disposition**: simplify + document. **Effort**: M (~2-4 days).
  **Confidence**: medium-high (replacement path is explicit in code; eviction
  frequency is inferred, not measured).

### Z3-F11 — Declared-but-unreachable consumer states and tools (`WaitingInput`, `ask_user`, plan/question/permission surfaces)

- **ID / stage / type / severity**: Z3-F11, `shipped`, BUREAUCRACY, **S4**.
- **Trigger**: A UI team implementing the "waiting for user" screen.
- **Mechanism**: `RunStatusDto::WaitingInput` is a first-class status with
  validated transitions and storage mapping
  (`intention-domain/src/lib.rs:43,84-92`;
  `intention-storage-sqlite/src/lib.rs:1845,1859`) but nothing produces it
  (grep finds only the domain and storage occurrences). `ToolId::AskUser`
  exists as a reserved descriptor with no schema
  (`intention-tools/src/lib.rs:1141-1154`) and the daemon explicitly refuses to
  decode it (`intention-daemon/src/lib.rs:913-920`). There is no protocol
  method to answer a question or a permission prompt, although the legacy
  baseline has both plus plan approval
  (`legacy-baseline/01-frontend-surface.csv`, rows `question.answer`,
  `permission.respond`, `plan.confirm`).
- **Blast radius**: dead UI states and a false sense of completeness: the enum
  invites a screen the product cannot produce or resolve. Cost is one wasted
  screen and one confusing status per adapter — small, but it is the same class
  of "declared surface with no producer" that Z3-F04 shows at protocol scale.
- **Simplest alternative**: mark reserved states as reserved in the DTO doc
  comments and in architecture 04/07, and add the question/permission contract
  together with their producer in M6/M7.
- **Disposition**: document-as-reserved (keep the value, remove the implication).
  **Effort**: S (<1 day). **Confidence**: high.

### Z3-F12 — Error text and the presentation layer: no message keys, no closed code list, no projection DTO

- **ID / stage / type / severity**: Z3-F12, `shipped` (code) + `doc-only`
  (direction), BUREAUCRACY/EXCESSIVE risk, **S4**.
- **Trigger**: A desktop UI with locale switching, as the legacy baseline has
  (`legacy-baseline/01-frontend-surface.csv`, `appearance.locale`).
- **Mechanism**: `ErrorDto` carries a safe English `message` plus a free-form
  `code` (`intention-types/src/lib.rs:443-453,491-506`); production source
  contains 162 distinct codes with no enumeration anywhere, and architecture 02
  names a presentation DTO, `SessionViewDto`, that does not exist in the tree
  (`02-dto-and-contract-policy.md:27`; `grep -rn SessionViewDto crates` → no
  matches). `ToolInvocationDto`, `ToolResultDto`, `PersistedRunDto` are named
  the same way and are equally absent.
- **Blast radius**: the UI either shows daemon English or maintains its own
  code→text map that no contract guarantees. Adding `SessionViewDto` later
  means a second boundary (transport DTO → presentation DTO) that the docs
  already anticipate but nothing implements.
- **Simplest alternative**: decide now — either the daemon returns stable
  message keys, or the UI's text layer is explicitly adapter-owned. Either way,
  publish the code list (or a code family prefix rule) rather than leaving it
  open.
- **Disposition**: decide + document. **Effort**: S (decision) / M (keys).
  **Confidence**: medium (no localization requirement is stated in the current
  architecture, but the baseline surface exists).

### 3.1 Finding counts

| Type | Count | IDs |
| --- | --- | --- |
| LANDMINE | 6 | F01, F02, F03, F05, F06, F10 |
| THEATER | 1 | F04 |
| BUREAUCRACY | 2 | F08, F11 |
| EXCESSIVE | 2 | F07, F09 |
| MIXED / other | 1 | F12 |
| JUSTIFIED (keep-list) | — | §4 |

By stage: `shipped` 11 (F01-F06, F08-F11); `committed` 1 (F07); `doc-only`
inside F12. Severity: S1 ×3 (F01, F02, F03), S2 ×2 (F04, F05), S3 ×5 (F06-F10),
S4 ×2 (F11, F12).

---

## 4. Keep-list (justified, with why)

- **K1. JSON-RPC 2.0 over NDJSON with exact-version `hello`**
  (`intention-protocol/src/lib.rs:1257-1260,1571-1644`;
  `03-daemon-transport-and-adapters.md:73-90`). Standard framing, one line per
  message, greppable with `socat`/`nc` during development, no custom codec. The
  envelope layer is thin relative to the DTO layer. Keep.
- **K2. Typed DTOs at the boundary with an explicit `schema_version` and no
  `serde_json::Value`** (`02-dto-and-contract-policy.md:111`). It costs
  boilerplate constructors (every adapter writes `SchemaVersionDto::new(1,1)`),
  but a single-version local product genuinely benefits from fail-closed
  decoding. Keep the policy; consider a `CURRENT`-defaulting constructor to cut
  the boilerplate.
- **K3. `ErrorDto` with code + category + retry + safe message**
  (`intention-types/src/lib.rs:289-334,443-453`). The four-field shape is
  exactly what a UI needs to decide "retry / tell the user / stop". Keep; only
  the code namespace needs a decision (Z3-F12).
- **K4. Run-fact cursors, contiguous live batches, and typed resync reasons**
  (`intention-protocol/src/lib.rs:332-491`;
  `intention-client/src/lib.rs:522-628`). Cursor-based dedup, gap detection and
  explicit resync are the right primitives for a flaky local attach, and the
  client-side reducer logic for stale/duplicate/gapped batches is sound
  (`run_stream_contract.rs:134-226`). The defect is what the API exposes, not
  the semantics. Keep the wire model.
- **K5. `SessionProjectionDto` as the "now" anchor** (`active_run`,
  `queued_turns` with content, `at_sequence`;
  `intention-domain/src/lib.rs:412-425`). This single read is what makes
  reconnect recovery possible today, and it is the right seed for the timeline
  projection. Keep and extend.
- **K6. Bootstrap coordination: startup lock + readiness via `hello` +
  `daemon.health`** (`intention-client/src/lib.rs:130-147,677-723`). Prevents
  duplicate daemons and "process exists" false readiness; small and earned.
- **K7. Adapter dependency boundary enforced by policy**
  (`quality/architecture.toml:114-128`: adapters may depend only on client,
  protocol, transport, types; no tauri/rusqlite/application/runtime/storage).
  The rule is checkable and it is checked. Keep (note that it also sanctions
  protocol/transport use from adapters, which is the current escape hatch for
  Z3-F03).
- **K8. Durable per-run fact model** (`ModelRunFactDto` with per-run cursors,
  ≤4 KiB text chunks, workspace-relative bounded tool results;
  `intention-domain/src/model_facts.rs:33-50,456-513`;
  `intention-daemon/src/lib.rs:934-969`). It already records everything a
  transcript needs; the gap is exposure, not data.
- **K9. 1 MiB transport message cap** (`intention-transport/src/lib.rs:33`).
  A liveness/allocation safeguard with a demonstrated failure mode, consistent
  with the limits policy; the problem is the unbounded projection that trips it
  (Z3-F06), not the cap.
- **K10. Fixture-daemon adapter tests as the acceptance form for UI crates**
  (`03-daemon-transport-and-adapters.md:364-377`;
  `intention-tui/tests/tui_contract.rs`). Testing adapters against a real
  fixture daemon instead of a line-coverage target is the right call. Keep the
  form; extend it to the actions the adapters cannot yet perform (Z3-F03).

---

## 5. Predictions

### 5.1 Cut

- `session.subscribe` as a read duplicate, or its `requested_mode` field
  (Z3-F08). Signal: the first UI that uses `session.snapshot` only; confidence
  high.
- The always-empty replay tails in their current form: either implemented or
  deleted within the first week of real client work (Z3-F04). Signal: any
  attempt to write the documented reconnect flow; confidence high.
- `WaitingInput` screens until an `ask_user` producer exists (Z3-F11). Signal:
  the first M6 UI task list; confidence medium-high.

### 5.2 Rewrite

- The `intention-client` subscription API: from "apply and hide" to "surface
  typed frames", plus commands, plus reconnect (Z3-F01, Z3-F03, Z3-F10).
  Signal: an M6 smoke test asserting visible text within ~1 s of the first
  token cannot be made to pass on today's API without `request_replay` polling;
  confidence high.
- The session-event tail: from "always empty" to a live frame feed, or to
  deletion with a merged run stream (Z3-F02, Z3-F04). Signal: a second window
  or an app restart showing a transcript; confidence medium-high.
- The cumulative run snapshot: from "whole text, rebuilt per commit" to a
  bounded/pageable projection (Z3-F06). Signal: the first run whose text
  approaches 1 MiB, or a profiling pass on the commit path; confidence medium.

### 5.3 Survive

- JSON-RPC 2.0 NDJSON, typed DTOs, `ErrorDto`, run cursors + resync, the
  adapter dependency boundary, and the session snapshot anchor (K1-K10).
  Confidence high: they are cheap, already standard, and used by every path
  that works today.
- The `RunId`/`TurnId` duality: unless a `run.list`/timeline read lands
  (Z3-F02), adapters will encode the hidden `run_id == turn_id` derivation
  (`intention/src/lib.rs:900-906`) into their own stores. Confidence medium.
  This is the single most dangerous shadow contract in the zone: it works
  today, is undocumented, and architecture 02 forbids reconstructing identity
  (`02-dto-and-contract-policy.md` section "UUID roles and non-conversion").

### 5.4 Signals to watch (ordered)

1. The first UI pull request that imports `intention_transport` or
   `intention_protocol` directly — confirms Z3-F03 became a bypass.
2. A client-side transcript store appearing in an adapter — confirms Z3-F02.
3. A sleep/benching test on a live run stream — confirms Z3-F10.
4. A commit-path timing test with a long assistant answer — confirms Z3-F06.
5. An M6 task list where "activity/notification projections" precedes
   "transcript" — confirms Z3-F07.

---

## 6. Metrics and commands used

All read-only; no build or test was run.

- `find crates -name main.rs` → exactly one binary
  (`crates/intention-daemon/src/main.rs`), confirming there is no client
  executable.
- `wc -l` over the zone crates: protocol 1919 + 849 (jsonrpc) + 1176 across its
  three test targets; client 794 + 1153 across two test targets; tui 46 + 81;
  tauri 5.
- Method inventory read directly from `ProtocolMethodDto::ALL`
  (`intention-protocol/src/lib.rs:1289-1298`) and from the query/command
  enums (`lib.rs:613-634`).
- Client surface inventory by reading `IntentionClient`/`RunStreamClient` in
  full (`intention-client/src/lib.rs`).
- `rg "DomainEventDto::" crates --glob '*.rs'` (excluding the domain crate and
  test files) to find producerless variants: `ConfigurationRevisionAccepted`,
  `PlanStatusChanged`, `ToolResultRecorded` have no production constructor.
- `rg "DomainEventDto::ToolResultRecorded" crates` → 0 matches; the M5
  tool-result path persists to the `tool_results` table instead
  (`intention-storage-sqlite/src/lib.rs:582-600`).
- `rg "load_tool_result" crates` → only `mod tests` callers (module starts at
  `intention-storage-sqlite/src/lib.rs:2018`).
- `rg "requested_mode" crates` → definition + accessor only, no production
  read.
- Distinct error codes: `rg -o -U 'ErrorDto::(new|validation|unavailable|conflict|policy|not_found|internal)\(\s*"[a-z0-9_]+"' crates/*/src`
  → 169 raw matches, minus 7 fixture/injected/placeholder tokens
  (`blocked`, `code`, `failed`, `fixture`, `fixture_storage_unavailable`,
  `injected_storage_fault`, `injected_terminalizer_failure`) = **162** distinct
  production codes. Note: this counts only codes passed as a literal first
  argument; codes built from variables (for example the tool-failure forwarding
  at `intention-daemon/src/lib.rs:879-883`) are not counted, so 162 is a lower
  bound.
- Stage attribution by reading the code paths directly, plus
  `git log --oneline -8 -- crates/intention-client crates/intention-protocol crates/intention-tui`
  to confirm the zone was last touched by M5-era and ADR 0044-0049 commits.
- Line-level claims: every `file:line` in this draft was read, not inferred.

---

## 7. Uncertainty and open questions

1. **No execution.** All findings are static reads. I did not run the daemon,
   the client tests, or a UI; latency and eviction-frequency claims (Z3-F06,
   Z3-F09, Z3-F10) are derived from code paths and labeled medium where they
   depend on runtime frequency.
2. **Intent of the reasoning behavior.** `apply_live_batch` only accumulates
   reasoning from facts at or below the snapshot cursor
   (`intention-client/src/lib.rs:569-585`), and no test covers a reasoning delta
   with a cursor above the snapshot. I read this as a defect (the "tail-only"
   wording in architecture 22 refers to storage, not to the live path), but a
   maintainer could have intended `reasoning_content` to be catch-up-only. The
   UI consequence is the same: live reasoning is invisible.
3. **How much of the client surface M5+ slice 4 will actually deliver.**
   The roadmap says the slice delivers "the exact typed client/protocol surface
   that M6 consumes" (`11-implementation-roadmap.md:496-497`) but does not name
   the methods. If the slice lands `session.list`/`session.timeline`, Z3-F02 is
   partly answered by plan, not by omission.
4. **Whether adapters are allowed to bypass the client.** The automated
   boundary permits `intention-protocol` and `intention-transport` from adapters
   (`quality/architecture.toml:116`), which contradicts the prose rule that
   `intention-client` is the only ingress
   (`03-daemon-transport-and-adapters.md:162-171`). Resolving that ambiguity
   changes the size of Z3-F03 but not the underlying gap.
5. **Multi-window/multi-client intent.** Architecture 24's planned
   notification/acknowledgement layer implies careful multi-observer semantics
   for a single-user, single-machine product; I did not find a requirement that
   justifies the acknowledgement aggregate specifically. That is an open
   question for the M6 activating specification, not a proven defect.
6. **Localization is not a stated requirement** in the current architecture.
   Z3-F12 is phrased conditionally for that reason; the legacy baseline's
   locale switch is the only evidence that it matters.


---

# Appendix D — Zone 4: LLM-agency fit

Baseline: `main` @ `3291e50`, working tree clean except the untracked
`pr-44-45-review-report.md` and the `audit-drafts/` directory (other auditors'
files, not read, not touched). Audit date 2026-10-02. Read-only audit; this file
is the only artifact produced.

Question: does the design's model of agency — determinism, guarantees,
verification, authority, failure, recovery — fit how LLM-driven agents actually
behave and how working systems in the industry are built? Judged by fitness for
real daily use, cost versus benefit under this project's actual conditions
(one local user, no deployed consumers), and delayed-action failure modes.

---

## 1. Scope and method

### 1.1 What I examined

Repo sources read in full or in the cited parts: `AGENTS.md`,
`docs/intention-relay/README.md`; architecture 07, 08, 10, 14, 15, 17, 21, 22
(parts), 23, 26, 27, 28, 30, 11 (roadmap milestones 5/5+), 12 (structure);
ADRs 0003, 0017, 0018, 0022, 0025, 0031, 0040, 0048, 0049 plus the ADR index;
the reconciliation contradiction register (CON-001..089). Code: the whole
`intention-runtime` and `intention-tools` crates, the daemon host and tool port,
`intention-application` request construction, `intention-model` request
validation, `intention-domain` model facts, `intention-storage-sqlite` run
context/transition/recovery/promotion paths, `intention-config` provider
execution policy, `intention-hooks`, `intention` composition root, and the
test/fixture layout and quality policy (`quality/*.toml`,
`quality/architecture.toml`).

Method per construct: (1) reconstruct the model of agency; (2) ask which
observable quality or safety outcome the construct buys, and how that outcome
would be measured; (3) stress the model with real model behaviour — variance,
failures, latency, cost; (4) apply the simplest-sufficient-world test — would a
competent team building the minimal working local agent keep it; (5) compare
public practice and mark anything not sourced as "general practice"; (6) audit
the rationale for why the construct exists. Every repo claim below carries a
`file:line`; every number shows its derivation; external claims carry a URL or
are marked general practice; estimates are labelled.

### 1.2 The reconstructed model of agency

Seven axioms, reconstructed from the sources (not stated as such anywhere):

| # | Axiom | Where it comes from | Outcome it claims to buy | How that outcome would be measured | Verdict |
| --- | --- | --- | --- | --- | --- |
| A1 | A model step is a bounded, typed, durable exchange: request → ordered stream facts → exactly one terminal fact | arch 08 §"Canonical model contract"; `intention-model` lifecycle validation | replayable, corruption-proof history | replay fixtures (present, hermetic, cheap) | JUSTIFIED — cheap and it holds |
| A2 | The agent is not allowed to be wrong in a way the machine cannot classify: every failure is a closed typed outcome | arch 10 evidence matrix; `ErrorDto` families across 13-28 | diagnosable failure | count of unclassified failure paths, and MTTD on real failures | MIXED — closure is real, diagnosability is not (F15) |
| A3 | Any started effect whose terminal result cannot be proven is `ExternalEffectUnknown`, and an unknown effect forbids the next model step | arch 15 §"Effect evidence"; ADR 0025 inv. 5 | never build on unknown state | frequency of this class under normal use | over-applied → LANDMINE (F02) |
| A4 | Authority is explicit, per-call, and revocable; nothing except a user issuance carries it | arch 27; arch 17 verifier authority; arch 28 goals | a local agent cannot do something the user did not authorize | confirmations per completed task | BUREAUCRACY at single-user scale (F06, F08) |
| A5 | The model is a stateless function re-fed reconstructed local history; nothing provider-side persists | arch 15 §"Model-to-tool-to-model lifecycle"; arch 22 §"Reasoning" | no hidden remote state, exact replay | provider-independence fixtures | JUSTIFIED as a boundary; EXPENSIVE as the only history strategy (F05, F14) |
| A6 | Verification is an evidence/authority problem, not a "run the tests" problem | arch 28 §"Verification gates and evidence"; arch 17 | completion is proven, not asserted | share of daily completions that satisfy a declared gate | EXCESSIVE and partially unusable for daily work (F07, F08) |
| A7 | Context is governance: immutable selection, manifests, omissions, audience | arch 21; arch 30; arch 23 | the model only sees what was selected | context bugs, leakage incidents | over-built where it matters least, absent where it matters most (F05, F13) |

The model is coherent and unusually honest about its limits (trusted-local, not
a sandbox; `WorkspaceRoot` not a boundary; Plan `execute` not contained —
arch 15 §"Ownership and one capability path", arch 14 §"What remains"). Its
weakness is not rigor; it is that the constructs answer *auditability*
questions, while daily use asks *liveness* questions: does the agent get
feedback, does anything run long, does the context fit, does the work resume.

### 1.3 Core questions, answered directly

**Q1 Daily use — first obstacles.** In shipped order of encounter: (1) any
model-side tool mistake ends the whole run and the model never learns of it
(F01); (2) no command may run longer than 30 s, and a timeout is classed as an
unknown effect that also ends the run, so the standard edit → test → fix loop
cannot execute at all (F02); (3) the first `grep`/`glob` on a real workspace
reads the whole tree including build output — 194 GB / 617,711 files in this
repository (F03); (4) `grep` is not a regex engine, so the model silently gets
zero matches for regex-shaped patterns (F04); (5) sessions grow without any
window accounting and eventually every turn fails the same way (F05);
(6) restart with a queued successor leaves a `Starting` run nothing schedules
(F09).

**Q2 Full trace.** Measured from code, 2026-10-02. One user turn today:
client → daemon JSON-RPC request → `SendUserTurn` → storage transaction
(turn + run `Starting`) → host `schedule_if_starting` (daemon
`src/lib.rs:1180`) → `load_starting_run_model_context` reads every prior turn
(`intention-storage-sqlite/src/lib.rs:1185-1244`) → one `ModelRequestDto`
(`intention-application/src/lib.rs:1411-1418`) → provider round → per tool call:
`ToolCallRecorded` commit, hook pipeline, `spawn_blocking` execution,
`ToolResultRecorded` commit, publication (`intention-runtime/src/lib.rs:704-790`)
→ continuation request rebuilt from local history → `Finished` → `Completing` →
`Completed`. For a 6-step task that is 6 provider round-trips, ~12 durable
transactions, ~12 publications, and 6 blocking tool executions. Manual
interventions today: none are possible in normal use — no interactive client
ships (`intention-tui` (46 lines) exposes only `connect`/`subscribe`;
`intention-tauri` is a 5-line stub; the only binary is the daemon, and the
protocol is drivable only by tests). Under the committed future direction,
interventions become one exact confirmation per `write`/`edit`/`execute` call
(arch 27 lines 215, 232), i.e. ~3-6 confirmations per task. Wall-clock is
dominated by provider latency; the 30 s cap (F02) bounds every command, so the
realistic task length is "a few short commands", not "a build and a test run".
Human-days to reach even that: M6 (desktop UI) is doc-only; the M5+ slices 3-5
and M6-M9 are unimplemented, i.e. the daily-use surface is not a matter of
polish but of several milestones.

**Q3 Simplest-sufficient-world.** A minimal working local agent for one user
keeps: typed DTO boundaries; a durable run/fact/cursor store; `WorkspaceRoot` as
an addressing anchor; the six shipped tools; a rule that tool errors are fed
back to the model; a per-command timeout parameter with a progress-based
watchdog; ignore-aware search; a token budget with compaction; a
confirmation-free Build mode. It cuts, on day one: reserved slots (F10), the
confirmation-per-call layer (F06), the verifier-authority algebra (F08), the
gate-template library (F07), the read-and-delegate harness (F11), immutable
instruction-profile revisions (F13), and the fork rate/depth ceilings (F14).
Everything it keeps is already in the repo or is one constant away.

**Q4 Comparative practice.** Claude Code and the Claude platform document
compaction triggered by a token threshold
(https://code.claude.com/docs/en/context-window,
https://platform.claude.com/docs/en/build-with-claude/compaction); aider feeds
lint/test failures back to the model for repair
(https://aider.chat/docs/usage/lint-test.html); ripgrep — the de-facto search
engine of agent CLIs — "respects your gitignore" by default
(https://github.com/BurntSushi/ripgrep, https://ripgrep.dev/docs/guide/).
Long-running build/test commands and streaming text are general practice, as is
retrying an interrupted stream with backoff. Reasoning-model latency is
variable and can exceed the shipped 30 s attempt cap
(https://platform.openai.com/docs/guides/reasoning, general practice).

**Q5 Future amputation.** See §5 (Predictions): the surfaces I expect to be cut
or rewritten, with the signals that will confirm it and confidence levels.

---

## 2. Vectors examined → verdicts

| # | Vector | Construct | Verdict | Severity | Stage |
| --- | --- | --- | --- | --- | --- |
| V1 | Failure semantics: tool error path | typed tool result → run failure | LANDMINE | S1 | shipped |
| V2 | Boundedness of `execute` | 30 s wall clock, non-configurable | LANDMINE | S1 | shipped |
| V3 | Workspace search | unignored full-tree traversal, full-file reads | LANDMINE | S2 | shipped |
| V4 | Search semantics | literal substring under a `grep` name | LANDMINE | S2 | shipped |
| V5 | Context growth | no token accounting, no trimming, no compaction | LANDMINE | S2 | shipped |
| V6 | Authority model for ordinary runs | exact confirmation per effect vs Autopilot | MIXED | S1 | committed |
| V7 | Verification | typed gate templates, no raw shell | BUREAUCRACY | S2 | committed |
| V8 | Verification authority | target sets, baselines, mutation matrix, verdicts | EXCESSIVE | S2 | committed |
| V9 | Restart/recovery | interrupt-only recovery; no startup scheduling | LANDMINE | S2 | shipped |
| V10 | Tool surface | 14-slot registry, 8 reserved | MIXED/THEATER | S2 | shipped + committed |
| V11 | Autonomous work | read-and-delegate only harness | EXCESSIVE | S2 | committed |
| V12 | Document/decision consistency | 16-call, 4 MiB, page bounds still asserted | MIXED | S3 | doc-only |
| V13 | Instructions | fail-closed 16 KiB `AGENTS.md`; no dynamic context | EXCESSIVE | S3 | committed |
| V14 | Branching/regeneration | text-only fork context | EXCESSIVE | S3 | committed |
| V15 | Provider failure visibility | code-only errors, 2 attempts, 250 ms, no retry after first byte | MIXED | S3 | shipped |
| V16 | Streaming granularity | 4 KiB durable batch = client update interval | EXCESSIVE | S4 | shipped |
| V17 | Hook pipeline | one no-op production hook | THEATER | S3 | shipped |

Finding cards follow, ranked by severity, then by cost of delay.

---

## 3. Findings

### Z4-F01 — A tool failure or a malformed tool call ends the run; the model never gets the error

- **Stage:** shipped. **Type:** LANDMINE. **Severity:** S1.
- **Trigger (real-world):** the model calls `read` on a path that does not
  exist; `edit` with an `old` string that no longer matches; sends
  `{"file_path": ...}` instead of `{"path": ...}`; hallucinates a tool name
  (`bash`, `ls`) that was never advertised. All of these are ordinary,
  hourly-scale events for LLM agents.
- **Mechanism:** `crates/intention-runtime/src/lib.rs:770-783` — a
  `ToolResultOutcomeDto::Failed` appends a `failed` fact and terminalizes the
  run with `RunStatusDto::Failed`. Port-level errors take the same exit at
  `crates/intention-runtime/src/lib.rs:725-745`. The daemon converts every
  tool-level error into such an outcome
  (`crates/intention-daemon/src/lib.rs:879-882`) and every undecodable
  call/argument into an error before execution
  (`crates/intention-daemon/src/lib.rs:861`, `899-931`). Behaviour is asserted
  by tests, so it is intended, not accidental
  (`crates/intention-runtime/tests/m5_tool_loop.rs:1363`, `1864`).
- **Blast radius (UX):** the whole turn is destroyed, the user must re-send, and
  the model never learns why. Cost: one human re-drive per model mistake; the
  agent cannot self-correct, which is where most of an agent's perceived
  competence comes from. Money: every wasted provider round-trip and every
  re-read after the re-drive.
- **Simplest alternative:** map a failed/refused tool call to a `tool`-role
  message carrying the typed error code and let the loop continue; keep a
  liveness threshold (e.g. N consecutive identical failures) rather than a
  single-shot kill. Reserve run-terminality for `ExternalEffectUnknown` and for
  cancellation, which is what the design actually reasons about.
- **Disposition:** collapse (tool failure → model feedback; unknown effect
  stays terminal). **Effort:** M (4-8 days: runtime change, contract + arch 15/10
  edits, fixtures, coverage). **Confidence:** high (code + tests read).
- **Cost of delay:** every day of dogfooding before the fix trains the user that
  the agent is fragile; retrofitting feedback loops after M6 UI exists means
  re-doing the run terminal-outcome taxonomy.

### Z4-F02 — `execute` cannot run anything that takes longer than 30 seconds, and timing out kills the run as an "unknown effect"

- **Stage:** shipped. **Type:** LANDMINE. **Severity:** S1.
- **Trigger:** `cargo test`, `cargo build`, `npm install`, `pytest`, `make`,
  `docker build`, a repo-wide format — the commands that constitute most of a
  coding agent's real work.
- **Mechanism:** `EXECUTE_TIMEOUT = 30 s` happens to be the whole deadline
  (`crates/intention-tools/src/lib.rs:146`, used at `:475-530`); it is not
  reachable from configuration (only the provider attempt timeout is
  configurable: `crates/intention-config/src/lib.rs:616-630`); the timeout path
  returns `Err("tool_execute_external_effect_unknown")`
  (`crates/intention-tools/src/lib.rs:477`, `508`, `524`), which the daemon maps
  to a failed outcome (`crates/intention-daemon/src/lib.rs:879`) and the runtime
  to a failed run (Z4-F01 mechanism). Collected stdout/stderr is discarded on
  that path, so the model cannot even see partial output. ADR 0048 keeps this
  bound as a liveness safeguard whose stated purpose is "bounds a child process
  that stops producing progress or never exits"
  (`docs/intention-relay/decisions/0048-limits-by-precedent-and-no-content-scanning.md:58`).
- **Blast radius (UX + money):** the product cannot complete the ordinary
  edit → test → fix loop, which makes the agent look useless next to any
  terminal. Each timeout also burns a provider round and destroys the turn.
  A 20-minute refactor becomes impossible without leaving the tool.
- **Simplest alternative:** (a) make the deadline a per-call parameter with a
  multi-minute default and a configured ceiling; (b) use the design's own
  progress-based deadline (`model_stream_progress_timeout_v1`,
  `docs/intention-relay/architecture/15-tool-registry-and-mandate-tool-loop.md:299-315`, §"Model progress deadline")
  instead of wall clock, since the stated precedent is a *stalled* process, not
  a slow one; (c) classify a timeout the executor itself caused — it kills the
  process group and reaps it (`crates/intention-tools/src/lib.rs:531-560`) — as
  a known failed tool result with partial output, not as
  `ExternalEffectUnknown`.
- **Disposition:** re-scope the bound; keep the drain window. Note also that the
  shipped contract already diverges from the owning document: arch 15 specifies
  `execute` as one `ShellCommandTextDto` with shell semantics
  (`docs/intention-relay/architecture/15-tool-registry-and-mandate-tool-loop.md:109`), while the shipped input is
  `{program, args}` "run directly, without shell interpretation"
  (`crates/intention-tools/src/lib.rs:978-990`, `:1409-1413`), so pipelines and
  redirects are unavailable and every compound command needs a self-invented
  `sh -c` wrapper. Whichever contract survives, fix the divergence here.
  **Effort:** S
  (1-3 days: parameter + config + classification + tests). **Confidence:** high
  on mechanism; medium on the classification change being accepted, since it
  deliberately weakens a stated invariant.
- **Cost of delay:** this is the first thing a daily user meets; if it is not
  fixed before M6, the UI's first impression is an agent that cannot build.

### Z4-F03 — `grep`/`glob` traverse the entire workspace with no ignore rules, reading every file to EOF

- **Stage:** shipped. **Type:** LANDMINE. **Severity:** S2.
- **Trigger:** any workspace-scope search on a real project with a build
  directory, `.git`, `node_modules`, caches, or virtualenvs. In *this*
  repository: `target/` is 194 GB across 617,711 files (`du -sh target`,
  `find target -type f | wc -l`).
- **Mechanism:** `glob` resolves `root.join(pattern)` and walks the `glob` crate
  unfiltered (`crates/intention-tools/src/lib.rs:1881-1930`); `grep_scoped`
  walks directories itself, skipping only symlinks, with no ignore list
  (`crates/intention-tools/src/lib.rs:2011-2100`); every file is read through
  `read_bounded`, which *drains the remainder to EOF* even after recording the
  64 KiB window (`crates/intention-tools/src/lib.rs:763-800`). There is no
  `.gitignore`, `.ignore`, or default-exclusion handling anywhere in the crate
  (`rg 'gitignore|\.ignore|target/' crates/intention-tools/src/lib.rs` → no
  match). `read` has the same drain-to-EOF behaviour on a single large file.
- **Derivation of cost:** ~617k files × (open + full read). Even at 10k
  files/s that is ≥60 s of metadata work; with 194 GB of content to read it is
  minutes to tens of minutes of pure I/O, on the most common first action of a
  session. The result is capped at 128 KiB
  (`MAX_GREP_AGGREGATE_BYTES`, `:152`), so the cost buys almost nothing.
- **Simplest alternative:** skip VCS-ignored paths (ripgrep's default:
  https://github.com/BurntSushi/ripgrep) plus a small default exclusion set
  (`.git`, `target`, `node_modules`, `dist`, `.venv`), add a search deadline,
  and report visited/skipped counts so truncation is visible. Do not read past
  the retained window unless a match needs it.
- **Disposition:** simplify (fix in place). **Effort:** S-M (2-6 days
  including a minimal ignore-file reader or an explicit default list).
  **Confidence:** high (code read; sizes measured).
- **Cost of delay:** agents grep constantly; an unusable grep pushes every
  session onto `execute`, which is capped at 30 s (F02), so the two landmines
  compound into "search is slow, and the workaround cannot run either".

### Z4-F04 — `grep` is a literal substring search, so regex-shaped requests silently return nothing

- **Stage:** shipped. **Type:** LANDMINE. **Severity:** S2.
- **Trigger:** the model sends `fn \w+\(`, `TODO|FIXME`, `\.unwrap\(\)`, or any
  anchoring, alternation, or escaping — standard behaviour for a tool named
  "Grep" (there is no regex dependency: `crates/intention-tools/Cargo.toml`
  lists only `glob`).
- **Mechanism:** both paths use `line.find(input.pattern.as_str())`
  (`crates/intention-tools/src/lib.rs:1981`, `:2098`); the descriptor is named
  `grep` (`:1092` region, `ToolId::Grep`) and the model-facing schema says only
  "Text pattern to search for inside workspace files"
  (`GREP_MODEL_PARAMETERS_SCHEMA`).
- **Blast radius (UX + correctness):** silent false negatives. The model
  concludes "there are no call sites" and edits code on that basis. This is the
  worst failure class for an agent: a confident wrong answer with no error.
- **Simplest alternative:** cheapest correct fix is naming/description truth —
  rename to `search_text` or state "literal substring, not a regular
  expression" in the schema description; keeping the `grep` name implies regex
  to any model trained on it.
- **Disposition:** simplify. **Effort:** S (<1 day for the honest description;
  2-4 days for a regex implementation). **Confidence:** high.
- **Cost of delay:** every day of use silently degrades the agent's factual
  grounding; wrong edits found later cost far more than the fix.

### Z4-F05 — No context-window accounting and no compaction: sessions grow until every turn fails

- **Stage:** shipped (absent mechanism) + committed (compaction design without
  a trigger). **Type:** LANDMINE. **Severity:** S2.
- **Trigger:** a normal long session — 30-60 turns, or a handful of large tool
  results in one agentic run.
- **Mechanism:** each new run's context is *all* prior turns' user text plus
  completed assistant text (`crates/intention-storage-sqlite/src/lib.rs:1185-1244`)
  with no count or size bound; within one run the loop appends every tool
  exchange to `messages` and never trims (`crates/intention-runtime/src/lib.rs:656-790`).
  Nothing anywhere accounts for a context window or token budget: `rg -i
  'context window|token budget|max_tokens'` over `architecture/` and
  `decisions/` returns nothing; in code, `UsageDto` records tokens as evidence
  only (`crates/intention-types/src/model.rs:87-150`). The design's compaction
  (`docs/intention-relay/architecture/21-goals-skills-context-memory-and-compaction.md:382-420`,
  `docs/intention-relay/architecture/28-goal-domain-and-verification.md:387-400`) is documentation-only,
  has no numeric trigger (28 says only "if the selected model-context bound
  would be exceeded"), and shipped code has no "selected model-context bound".
  When the provider rejects an over-long request, the run fails permanently
  (permanent 4xx classification, arch 08 §"Retry and timeout ownership") and the
  next turn rebuilds the same context plus one message — the session is bricked
  with no local remedy (no trimming, no message deletion, no compaction, and
  fork refuses a context over 1 MiB: arch 23 field-limit table).
- **Blast radius:** whole sessions abandoned; the user loses the agent's
  working memory and must start a new session with a hand-written summary.
  Provider rejection is the most common hard failure in real agent use.
- **Simplest alternative:** count what is already reported (`UsageDto` input
  tokens), compare with a configured window size, warn at a fraction, and
  compact on threshold — exactly the industry pattern (Claude Code auto-compact
  at a token threshold: https://code.claude.com/docs/en/context-window;
  https://platform.claude.com/docs/en/build-with-claude/compaction). Secondary:
  give the model an `execute`-free way to drop or shrink old tool results, or
  permit them to be elided (which contradicts arch 15's "never truncated" —
  see F12).
- **Disposition:** add the missing mechanism (this is the single highest-value
  un-owned capability in the design). **Effort:** M (5-10 days for a first
  budget + warn + summarize path with tests). **Confidence:** high on the
  absence; medium on how soon it bites, which depends on the window size of the
  user's chosen model.
- **Cost of delay:** every accumulated session makes the missing mechanism more
  expensive to add, because history format and projections are the hard part.

### Z4-F06 — Two accepted directions disagree on whether an ordinary run may write or execute without a confirmation

- **Stage:** committed. **Type:** MIXED (BUREAUCRACY with a LANDMINE edge).
  **Severity:** S1 (product-level).
- **Trigger:** the default configuration, one user, one Build session, the model
  wants to write a file or run a command.
- **Mechanism:** arch 27 gives an `InteractiveUser` root only the
  `DirectLocalRead` baseline ("Every other call requires an exact confirmation",
  `docs/intention-relay/architecture/27-programmatic-caller-policy-and-admission.md:215`, "`execute` therefore never
  receives `DirectLocalRead`", `:232`), with one durable decision bound to one
  `ToolCallId`. ADR 0017/0018 and arch 07 say the opposite for the same actor:
  Build Autopilot is "admitted without per-action confirmation"
  (`docs/intention-relay/architecture/07-plan-and-build-modes.md:37`), "Build Autopilot has no
  per-action confirmation barrier for configured active tools"
  (`decisions/0017-build-autopilot-and-plan-focus-continuity.md:51`); arch 15 says "No Mandate call may enter
  `AwaitingConfirmation`" and "not gated by confirmation, risk selector,
  root-origin, parent, Goal, Skill, provider, MCP, prompt, model, quota"
  (`architecture/15-tool-registry-and-mandate-tool-loop.md`, §"Mandate direct admission"). The contradiction
  register resolves only the Mandate side (CON-002, CON-071); ordinary Build
  runs are left with both directions live.
- **Blast radius:** if arch 27 wins, every write and every command costs a human
  decision: a 20-step refactor becomes ~20 confirmations, and the product is a
  worse `git` than a plain shell. Human-days of pure waiting, plus the loss of
  the autonomy that is the product's reason to exist.
- **Simplest alternative:** pick one rule and state it in one place: the mode
  the user selected is the authorization (Plan denies `write`/`edit`; Build
  Autopilot admits the configured tool surface), with the existing durable
  evidence, cancellation, and unknown-effect rules untouched. Keep the
  programmatic-caller policy for the future bridge/MCP/harness roots, where a
  second actor genuinely exists.
- **Disposition:** collapse (one rule; delete the ordinary-run confirmation
  requirement). **Effort:** S (documentation decision, <1 day) but it must be
  made before arch 27 is implemented, or the implementation will need unwinding.
  **Confidence:** high that the texts conflict; medium on which the maintainer
  intends to keep.
- **Cost of delay:** this decides whether the product is usable; discovering it
  after M6 ships a confirmation UI costs a UX rewrite plus a storage model for
  confirmations that should never have existed.

### Z4-F07 — Verification requires user-authored typed gate templates; the edit → test → fix loop has no direct path

- **Stage:** committed. **Type:** BUREAUCRACY. **Severity:** S2.
- **Trigger:** the user asks the agent to fix a failing test.
- **Mechanism:** `ExecutableGate { template_id, template_revision }`
  (`docs/intention-relay/architecture/28-goal-domain-and-verification.md:303-310`); gates are
  user-created templates ("no raw shell text, arbitrary path/URL/header map,
  executable code, opaque JSON"), and a model "may only prepare a template
  proposal" that the user accepts. Combined with F06, an ordinary run cannot run
  `cargo test` at all unless a template exists and each call is confirmed.
- **Blast radius:** the agent's most valuable loop (change code, run the
  project's checks, read the failure, fix it) is only reachable after the user
  builds and maintains a template library for their own repository. That is
  ceremony for the one check that every developer already has a command for;
  industry practice is the inverse — aider runs configured lint/test commands
  after every edit and feeds failures back
  (https://aider.chat/docs/usage/lint-test.html).
- **Simplest alternative:** one configured project check command per project
  (`checks.test = "cargo test"`), runnable by the ordinary Build agent as a
  normal typed tool call with a real timeout, with the raw exit status and
  bounded output as the evidence; keep typed gate templates for evidence
  records and for the future delegated-verifier path where a *second* actor
  needs bounded authority.
- **Disposition:** re-scope (project check command now; template authority
  later). **Effort:** S-M (3-6 days including config plumbing and outcome
  tests). **Confidence:** medium-high (the doc is explicit; the simplification
  is a design judgement).
- **Cost of delay:** without it, the agent cannot verify anything it does,
  which is precisely the property the whole verification apparatus exists to
  provide.

### Z4-F08 — Verification is modelled as an authority algebra (targets, baselines, verdicts, mutations) for a system with exactly one authority: the user

- **Stage:** committed. **Type:** EXCESSIVE. **Severity:** S2.
- **Trigger:** any attempt to use the goal/verification model in normal work.
- **Mechanism:** arch 17 owns immutable verifier authority, target sets,
  audit baselines with a stale tuple, a closed operation matrix
  (`MarkComplete`, `MarkNeedsRework`, `ReviseFull`, `ResolveUnknownEffect`, …),
  verdict mutation transactions and reconciliation, plus a delegated child
  graph with per-edge controls and messages
  (`docs/intention-relay/architecture/17-mandate-child-graph-and-delegated-verifier-authority.md` §"Separately issued delegated
  verifier authority"…§"Audit, mutations, conflicts, and reconciliation");
  arch 28 adds goal trees, readiness, user-decision states, exception sets, and
  proposal coalescing. All of it is enforced against a single local user, who
  is also the only issuer of authority and the only reader of evidence.
- **Blast radius (human-days):** this is the largest single block of
  unimplemented design in the corpus (two of the longest documents, ~30k
  words combined) whose daily-use output is "a durable verdict record". A solo
  developer decides by reading a diff and a test result, not by having a
  VerifierMandate with a target-scoped authority revision mutate a baseline.
  The genuine industry need — *a second, independent context checking the
  first* (evaluator-optimizer / review subagent, general practice) — is
  satisfied by an ordinary second run with read tools, not by an authority
  algebra.
- **Simplest alternative:** keep `Goal` as a durable acceptance note; keep
  evidence (test output, diff hash, exit status) attached to the run; let the
  user accept/stop. Add a plain "verifier run" (fresh run, read-only tools,
  produces a verdict note) for independence of context; delete the target-set/
  baseline/mutation/authority machinery until a second actor or a compliance
  requirement exists.
- **Disposition:** defer/re-scope (keep the goal+evidence core, cut the
  authority plane). **Effort:** the saving, not the work: ~XL (weeks) of
  planned implementation and fixtures avoided. **Confidence:** medium-high
  (judgement about product need; the documents are read as written).
- **Cost of delay:** the cost is incurred at implementation time, so delay is
  cheap — which is exactly why the decision should be taken now, before M10-M12
  build it.

### Z4-F09 — After a restart, a promoted queued run stays `Starting` with nothing scheduled to run it

- **Stage:** shipped. **Type:** LANDMINE. **Severity:** S2.
- **Trigger:** daemon restarts (crash, upgrade, machine reboot) while a run is
  active and a second user turn is queued.
- **Mechanism:** recovery marks every unfinished run `Interrupted`
  (`crates/intention-storage-sqlite/src/lib.rs:1337-1371`); the terminal
  transition *atomically promotes the oldest queued turn* and inserts its run
  with status `starting`
  (`crates/intention-storage-sqlite/src/lib.rs:976-982`, `:461-486`). The only
  code that schedules a `Starting` run is the host's terminal side effect
  (`crates/intention-daemon/src/lib.rs:518-524`) and the acceptance path for a
  *newly started* turn (`:1174-1181`). Startup runs recovery before serving
  (`crates/intention/src/lib.rs:772`, called from `open_platform`,
  `crates/intention-daemon/src/lib.rs:995`) and then accepts connections with no
  sweep for `Starting` runs (`crates/intention-daemon/src/lib.rs:1024-1032`); a
  new turn arriving behind a `Starting` run is queued, not started, so it does
  not trigger scheduling either. The session stalls until the user stops the
  stale run — the stop path does handle unregistered runs
  (`crates/intention-daemon/src/lib.rs:282-300`) — or restarts again.
- **Blast radius (UX):** the session silently stops responding, looking like a
  hang, for a run the user did not start. Recovering means knowing to cancel an
  invisible run. This fires on the *first* ordinary restart for any user with a
  queue, and it is the kind of defect that destroys trust in unattended use.
- **Simplest alternative:** after recovery, enumerate sessions with a `Starting`
  run and schedule them once (the same call the terminal path already makes).
  This does not resume interrupted external work; it starts fresh work the
  design already admitted.
- **Disposition:** fix. **Effort:** S (1-3 days: startup sweep + fixture for
  restart-with-queue). **Confidence:** high on the code paths; medium on whether
  a client-driven path I did not find also schedules it (I found none).
- **Cost of delay:** user-visible reliability defect that will be attributed to
  the agent, not to the queue.

### Z4-F10 — The advertised registry has 14 slots and 6 live tools; the missing ones are exactly the agency affordances

- **Stage:** shipped (registry/reservations) + committed (the owners).
  **Type:** MIXED (THEATER until activated, LANDMINE through F01).
  **Severity:** S2.
- **Trigger:** the model wants to fetch a URL, ask the user a clarifying
  question, keep a task list, retrieve compressed content, or submit a plan.
- **Mechanism:** `registry()` declares 14 descriptors, 8 `Reserved` with no
  schema and no model visibility (`crates/intention-tools/src/lib.rs:1041-1240`;
  active list at `:1245-1253`); reserved names are rejected at decode
  (`crates/intention-daemon/src/lib.rs:899-931`, `unknown_tool`) and — via F01 —
  a model that calls one ends the run. `intention-vfr`, `intention-headroom`,
  and `intention-plans` are 5-line placeholders.
- **Blast radius:** no web access at all; no mid-run clarification, so ambiguity
  is resolved by guessing (and a wrong guess is a failed run, F01); no task list,
  so long multi-step work has no externalized plan; `retrieve`/`expand` absent
  means the VFR/Headroom compression story has no consumer.
- **Simplest alternative:** implement `todo` and `ask_user` first (both are
  cheap, both are pure model-affordance value), then `fetch_url`; keep the rest
  reserved but stop treating the 14-slot list as the product surface.
- **Disposition:** re-scope the order (do not activate the full list).
  **Effort:** S-M for `todo`+`ask_user` (3-6 days). **Confidence:** high.
- **Cost of delay:** every session run without clarification affordance
  produces avoidable failed runs; each one is now a lost turn (F01).

### Z4-F11 — The continual harness can only read and delegate, so its machinery buys very little

- **Stage:** committed. **Type:** EXCESSIVE. **Severity:** S2.
- **Trigger:** a user configures scheduled work (nightly review, weekly
  triage).
- **Mechanism:** "The direct run and its whole descendant subtree are
  read-and-delegate only… Direct write, edit, process start, network retrieval,
  user interaction, and model-created rule changes are outside this first
  scope" (`docs/intention-relay/architecture/26-continual-harness.md:177-186`), with class
  resolution, rules, triggers, coalescing, DST handling, dossiers, verified
  checkpoints, 15 closed failures, and document/limit tables around it
  (`architecture/26-continual-harness.md` §"Bounds", §"Selection record").
- **Blast radius (human-days):** scheduled autonomous work that can neither run
  the tests nor write the report to disk nor ask a question is limited to
  "summarize/review and delegate". The design cost is high (schedule engine,
  DST, coalescing, checkpoint validation, evidence) and the reachable daily
  value is low; the first thing any user will do is ask the harness to run
  `cargo test`, which fails closed.
- **Simplest alternative:** either (a) let a harness rule name one user-chosen
  allowed capability (including a project check command) as its class, so a
  scheduled run can do a bounded, useful job, or (b) defer the harness until
  write/execute classes exist and start with user-launched "recurring prompt"
  semantics and no new trigger engine.
- **Disposition:** defer/re-scope. **Effort:** avoided, not spent: L (weeks).
  **Confidence:** medium-high.
- **Cost of delay:** low if deferred now; high if implemented as specified and
  then re-scoped, because triggers, checkpoints, and journals are storage
  format.

### Z4-F12 — ADR 0048 removed tool-loop bounds that the owning architecture still asserts, and one shipped comment describes a validator that no longer exists

- **Stage:** doc-only (contradiction), shipped (stale comment).
  **Type:** MIXED. **Severity:** S3.
- **Trigger:** the next implementer follows the owner document, or a reviewer
  trusts the code comment.
- **Mechanism:** ADR 0048 decision 2 removes "the 16-call tool-group maximum and
  its `provider_tool_group_invalid` outcome", the 512 KiB/4 MiB first-scope
  bounds, `tool_output_limit_exceeded`, and the 256-fact/512 KiB page bounds
  (`docs/intention-relay/decisions/0048-limits-by-precedent-and-no-content-scanning.md:22`,
  `:39`), and the contradiction register
  records the removal (CON-081). Architecture 15 still asserts all of them
  (`docs/intention-relay/architecture/15-tool-registry-and-mandate-tool-loop.md:200`, `:247-257`, `:281`), and
  architecture 22 asserts "the existing 512 KiB individual-fact bound" and a
  fixed 4 MiB combined reasoning bound
  (`docs/intention-relay/architecture/22-provider-evolution-profiles-and-reasoning.md:391-395`). In code,
  `crates/intention-runtime/src/lib.rs:951-953` documents enforcement by
  `intention_domain::validate_reasoning_fact_output_bound` and
  `reasoning_aggregate_bytes`; `rg` finds both names *only* in that comment —
  the validator and the accounting do not exist.
- **Blast radius:** a future tool loop that ships a 4 MiB per-group budget with
  no truncation and a per-call `OutputLimitExceeded` terminal outcome is a
  daily-use regression (a test suite's output alone can cross it), introduced by
  following the authoritative document. The stale comment misleads a reader
  about the guarantees of the current reasoning path.
- **Simplest alternative:** make arch 15/22 match ADR 0048 (bounds only by
  precedent) and delete the stale comment; no behaviour change.
- **Disposition:** collapse (documentation). **Effort:** S (<1 day).
  **Confidence:** high (textual).
- **Cost of delay:** the cost is realised at slice-3 implementation time; the
  fix is nearly free now.

### Z4-F13 — The instruction channel fails closed on ordinary project instructions and forbids the context injections models actually need

- **Stage:** committed. **Type:** EXCESSIVE. **Severity:** S3.
- **Trigger:** a project whose `AGENTS.md` exceeds 16,384 characters; or the
  model needing the date, the workspace path, or the platform.
- **Mechanism:** bounds are "one fragment at most 16,384 characters",
  "workspace project instructions at most 16,384 characters", "total assembled
  projection at most 100,000 characters"
  (`docs/intention-relay/architecture/30-instruction-sources-and-system-context.md:254-258`); failure behaviour is
  closed and total — "No fallback revision, partial projection, silent
  omission, truncation, sampling… is permitted"
  (`docs/intention-relay/architecture/30-instruction-sources-and-system-context.md:274-292`), so an oversized
  `AGENTS.md` rejects admission outright. Non-goals explicitly exclude
  "date/time/Git/session-derived context injection"
  (`docs/intention-relay/architecture/30-instruction-sources-and-system-context.md` §"Dependencies and non-goals").
  For scale: this repository's own `AGENTS.md` is 8,144 bytes (`wc -c`), i.e.
  half the cap; real-world `CLAUDE.md`/`AGENTS.md` files regularly exceed 16 KB.
- **Blast radius (UX):** a project can be un-runnable because it documented
  itself too well; the fix (split the file) is not discoverable from a typed
  error. Missing date/cwd context produces wrong dates in reports and wrong
  paths in commands — cheap fidelity the design has decided against.
- **Simplest alternative:** warn and clamp instead of failing (declaring the
  clamp in the projection identity), or raise the cap to a size that is
  unreachable in practice; add the standard dynamic context sections (date,
  platform, workspace path) as declared, non-authorizing sources.
- **Disposition:** simplify. **Effort:** S (1-2 days in the slice-5 spec).
  **Confidence:** high.
- **Cost of delay:** cheap now, annoying later once the projection identity and
  fork materialisation depend on the exact byte policy.

### Z4-F14 — A fork or regeneration loses the agent's working memory: tool calls, results, and reasoning are excluded from fork context

- **Stage:** committed. **Type:** EXCESSIVE. **Severity:** S3.
- **Trigger:** "regenerate this answer", or branch a session to try another
  approach, in a session where the agent read files and ran commands.
- **Mechanism:** `fork-model-context-v1` includes "validated user messages and
  only eligible final nonblank assistant messages" and excludes "reasoning
  text/summaries, attempts, usage, tool calls/results, questions, permissions,
  child results, raw provider data" (`docs/intention-relay/architecture/23-non-destructive-session-branching-and-regeneration.md:94-100`).
  Regeneration is that projection plus a fresh run — it does not re-execute the
  tools.
- **Blast radius:** the regenerated/branched agent is strictly less informed
  than the original: it does not know which files were read, what they
  contained, or what the test output was. It will re-read (extra tokens and
  latency) or, worse, answer from the user messages alone and hallucinate file
  contents. Industry practice keeps the full transcript across resume/fork and
  lets compaction summarise it (https://code.claude.com/docs/en/context-window).
- **Simplest alternative:** include a bounded, summarised form of tool activity
  (call + result digest) in the fork context, or require the materialised
  summary of architecture 28 as the fork context instead of the text-only
  projection.
- **Disposition:** re-scope. **Effort:** S in spec, M in implementation.
  **Confidence:** medium (it is a deliberate, documented choice; my judgement is
  that it trades away the feature's value).
- **Cost of delay:** the cost lands when branching ships; changing the frozen
  context schema later means a versioned projection — which the project's
  single-version policy makes cheap, but the fork base snapshot is storage.

### Z4-F15 — Provider failures are opaque error codes, the retry budget is two attempts and 250 ms, and a stream that broke after the first byte is never retried

- **Stage:** shipped. **Type:** MIXED. **Severity:** S3.
- **Trigger:** an HTTP 400 from the provider (context too long, unsupported
  parameter, gateway policy), a mid-stream disconnect, a transient 502 after
  some text has been produced.
- **Mechanism:** `ProviderErrorDto` carries a code, a retry class, and an
  optional correlation id — no status, no provider message
  (`crates/intention-types/src/model.rs:174-178`); the generic adapter derives
  retryability from the HTTP status but exposes only the normalized code
  (`generic_chat_provider_unavailable` / `generic_chat_provider_request_rejected`,
  arch 08 §"Retry and timeout ownership"), and the daemon maps to a
  `RunFailureDto(code, retry, correlation)`. The runtime retries at most once,
  after a fixed 250 ms (`RETRY_DELAY`, `crates/intention-runtime/src/lib.rs:179`)
  and only while `!durable_output && pending_text.is_empty()`
  (`:588-591`) — so any drop after the first flushed 4 KiB (or any
  reasoning delta) is terminal.
- **Blast radius (UX):** the most frequent real failure ("the provider said no")
  arrives as a code with no reason: the user cannot tell an oversized context
  from a bad parameter from a transient gateway rejection, which turns a
  5-minute diagnosis into guesswork. The no-retry-after-first-byte rule is
  stricter than practice (CLI agents retry interrupted streams with backoff;
  general practice) and costs a full turn on a network blip.
- **Simplest alternative:** carry a bounded, redacted provider status and error
  type (never a body or credential) in the safe failure; allow bounded retry
  with backoff when the stream broke before completion, using the durable
  cursor to avoid duplicate content.
- **Disposition:** simplify/add. **Effort:** S-M (2-5 days).
  **Confidence:** high on the mechanism; medium on the retry change (it
  interacts with the durable-output invariant).
- **Cost of delay:** diagnosability is the difference between a tool a user can
  operate and one they cannot.

### Z4-F16 — A storage liveness bound (4 KiB assistant batch) is also the client's text-update granularity

- **Stage:** shipped. **Type:** EXCESSIVE. **Severity:** S4.
- **Trigger:** any assistant answer longer than 4 KiB.
- **Mechanism:** `MAX_ASSISTANT_CONTENT_BYTES = 4 KiB`
  (`crates/intention-runtime/src/lib.rs:178`) and `flush_full_text` only emits a
  durable fact once the pending buffer reaches that size
  (`crates/intention-runtime/src/lib.rs:1238-1257`); the client's live view is
  driven by those facts. There is no separate, smaller live-notification path.
- **Blast radius (UX):** the user waits until ~1,000 tokens have accumulated
  before seeing anything, then sees text in chunks — a worse perceived latency
  than token streaming for no storage reason.
- **Simplest alternative:** publish transient deltas for the live view while
  keeping the durable batch bound (the publication path already separates
  durable facts from delivery).
- **Disposition:** simplify (UX-only change, later). **Effort:** S (1-3 days).
  **Confidence:** high on mechanism; medium on whether the UI can render
  sub-batch deltas without a new event class.
- **Cost of delay:** slight, but it is the kind of detail that decides whether
  the product "feels" like a modern agent.

### Z4-F17 — The only production hook is a no-op, so the 8-phase hook pipeline has no reachable effect today

- **Stage:** shipped (pipeline + no-op hook) / committed (consumers).
  **Type:** THEATER. **Severity:** S3 (cheap, but it is dead weight sold as a
  feature).
- **Trigger:** none in daily use.
- **Mechanism:** `production_hooks()` registers exactly one hook,
  `SafeObserverHook`, which returns `HookOutcome::Continue`
  (`crates/intention/src/lib.rs:209-241`); the consumers (VFR, Headroom, plans)
  are 5-line stubs; M8 is doc-only. Eight phases and a dispatcher exist around
  it (`crates/intention-hooks/src/lib.rs:103-140`).
- **Blast radius:** none today; the risk is documentation and coverage budget
  spent on a mechanism with no behavioural consumer, and a reader assuming hook
  ordering matters in production.
- **Simplest alternative:** keep the pipeline (it is small and the extension
  point is real) but do not count it as delivered behaviour; the note belongs in
  the M5 closure evidence rather than in architecture claims.
- **Disposition:** keep, re-label. **Effort:** none. **Confidence:** high.
- **Cost of delay:** none — but leaving the claim unqualified means the next
  reader budgets implementation and gate time for a mechanism that currently
  changes no behaviour.

---

## 4. Keep-list (justified — why these survive)

1. **Durable typed facts with one live schema version, commit-then-publish**
   (`architecture/04`, ADR 0038/0046). Cost is measurable milliseconds;
   it buys exact replay, crash-consistency, and testability. Simplest-world
   keeps this.
2. **`WorkspaceRoot` as an addressing anchor, explicitly not a security
   boundary** (ADR 0047, `architecture/14` §"What remains"). The earlier
   containment machinery was already amputated; what remains is what tools
   actually need (relative base, `execute` cwd, search scope) and the honest
   non-claim prevents false assurance. Keep.
3. **DTO-first boundaries, typed errors, adapter isolation.** Cheap in Rust,
   prevents a real class of drift, and is what makes the hermetic test suite
   possible. Keep.
4. **The limits-by-precedent policy and its liveness keep-list** (ADR 0048,
   `docs/intention-relay/production-ceiling-removal.md`). The frame is right and
   the retained safeguards are real; this audit's disagreements are about two
   values (execute timeout, and the removed-vs-still-documented tool-loop
   bounds), not about the policy.
5. **`ExternalEffectUnknown` as a distinct terminal class.** Genuinely valuable
   for a started effect whose result was lost. Keep the class; stop using it for
   a process the executor itself killed (F02).
6. **No-resume-after-restart.** Correct for external effects. The bug is the
   missing startup scheduling of *fresh* admitted work (F09), not the no-resume
   rule.
7. **Honest trusted-local modelling**: "no agent sandbox, no privilege
   separation… the capability plane is logical product/safety policy, not a
   security boundary" (`architecture/15`), "Plan `execute`… not technically
   contained" (`architecture/07`). This is rarer and more valuable than any of
   the removed machinery. Keep and defend.
8. **The model-progress deadline** (`model_stream_progress_timeout_v1`,
   `architecture/15` §"Model progress deadline"). Right mechanism, right
   granularity; it should replace the shipped absolute attempt deadline.
9. **Retry-once-before-any-durable-output.** Minimal and sound; the only
   criticism is that it is thinner than practice after the first byte (F15).
10. **The quality gate** (`Makefile`, `quality/*.toml`): strict lints, three
    feature profiles, per-crate coverage (80% base, ADR 0049), declared test
    targets, architecture policy. Expensive but it is the only thing keeping 20
    crates coherent, and the M5+ revert (ADR 0044) shows the gate works. Keep —
    with one caveat: it verifies the machine, not the agent. There is no
    evaluation harness for the model-facing surface (tool descriptions, prompt
    assembly, loop behaviour); the only real-model exercise is the ignored,
    manual `real_api_e2e` target (`architecture/10` scenario J, ADR 0040). The
    cheapest useful addition is 5-10 scripted scenario runs against a real
    provider, recorded as evidence, not a gate.
11. **Fake-secret structural hygiene and the ban on runtime content scanning**
    (ADR 0048 §"No runtime content scanning"). Scanning was correctly identified
    as theater; structural redaction is not. Keep.
12. **No synthetic historical state / no compatibility layers** (ADR 0038).
    Correct under the project's actual conditions and it keeps the schema
    single-versioned; the cost of the rule is one document-review discipline
    (see F12 for the one place it slipped).

---

## 5. Predictions (cut / rewrite / survive)

| Prediction | Signal that confirms it | Confidence |
| --- | --- | --- |
| **CUT — per-call exact confirmation for ordinary runs** (F06). Once a user runs the agent for a day, the click-per-command cost is unbearable, and the project's own Autopilot decision already says so. | First attempt to dogfood a multi-file change; a user-written wrapper script that batches confirmations; or the M6 UI growing a "don't ask again" toggle (which is the cut in disguise). | high |
| **CUT — verifier authority algebra and (probably) gate templates** (F07, F08). Implemented eagerly, then re-scoped to "run the project check, attach evidence". | M10-M12 planning arguing about baselines larger than the work they gate; the first user-authored gate template being a one-liner that just runs the tests. | medium |
| **CUT — reserved-slot registry framing and read-and-delegate harness classes** (F10, F11). | First request to schedule something that must write or run; the first `todo`/`ask_user` demand. | medium-high |
| **REWRITE — the tool layer** (F01-F04, F10): tool errors become model feedback; `execute` gets a shell-text or command-template contract with a real timeout; search becomes ignore-aware and regex-backed (or honestly named); `read` gains an offset/limit window. The shipped contract already diverges from arch 15's `ShellCommandTextDto` (`architecture/15:109` vs `crates/intention-tools/src/lib.rs:978-990`, `:1409-1413`) — the rewrite resolves that divergence. | Any real repository-wide search; any build longer than 30 s; the first model hallucinated tool name; the first honest "grep found nothing" that is false. | high |
| **REWRITE — context management** (F05): adds a window budget, a threshold trigger, and compaction; the existing immutable-summary design survives as the durable representation if it gains a trigger. | Session length growth; first provider rejection for over-long context; user complaint "it forgot what it did". | high |
| **REWRITE — fork/regeneration context** (F14) to include bounded tool evidence, or to reuse the compaction summary. | First branch that has to re-read files it already read. | medium |
| **SURVIVE — durable facts/cursors, DTO boundaries, single-version schema, `WorkspaceRoot` anchor, honest non-security claims, unknown-effect class, no-resume, quality gate.** | These are load-bearing for every other feature and are already paid for. | high |
| **SURVIVE with a fix — restart recovery** (F09): startup scheduling of admitted fresh work, no resumption of effects. | Any restart with a queue. | high |
| **DELAYED SURPRISE — provider-definition drift.** Nothing in the design pins model behaviour (tool-calling reliability, schema adherence, reasoning availability) to a recorded expectation, so a provider update silently changes agent quality with no local signal; the only detector is user frustration. | A model version change that breaks the tool loop on a provider the user did not change; the same task suddenly needing more rounds. | medium |

---

## 6. Metrics and commands used

Repo measurements (all 2026-10-02, `main` @ `3291e50`):

- Production source: 28,357 lines across 25 crates' `src`
  (`find crates -name '*.rs' -path '*/src/*' -exec cat {} + | wc -l`).
- Test source: 24,962 lines (`... -path '*/tests/*' ...`), plus inline
  `#[cfg(test)]` modules; ~0.88 test lines per production line.
- Architecture documents: 13,549 lines / 97,755 words; ADRs: 7,522 lines across
  49 files. Ratio to production code: ~0.48 doc lines per source line for
  `architecture/` alone, ~0.75 including decisions.
- 19 of 30 architecture documents declare themselves "documentation-only" /
  unimplemented (grep `documentation-only` per file).
- `target/`: 194 GB, 617,711 files (`du -sh target`; `find target -type f | wc -l`).
- `AGENTS.md`: 8,144 bytes vs. the committed 16,384-character instruction cap.
- Shipped constants that shape daily use: 30 s `execute` deadline
  (`intention-tools/src/lib.rs:146`); 64 KiB tool read/output window (`:145`);
  128 KiB shared search window (`:152`); 1 MiB edit target (`:156`); 4 KiB
  assistant durable batch (`intention-runtime/src/lib.rs:178`); 512 KiB
  canonical fact (`intention-storage-sqlite/src/lib.rs:32`); 256 tail facts
  (`intention-domain/src/model_facts.rs:12`); 250 ms retry delay
  (`intention-runtime/src/lib.rs:179`); 2 attempts, attempt timeout default
  30 s within 1..=60 (`intention-config/src/lib.rs:616-630`).
- Registry: 14 slots, 6 active (`intention-tools/src/lib.rs:1041-1253`);
  production hooks: 1 no-op (`intention/src/lib.rs:209-241`).
- Daemon restart/recovery path: `intention/src/lib.rs:772`,
  `intention-storage-sqlite/src/lib.rs:461-486`, `:976-982`, `:1337-1371`,
  `intention-daemon/src/lib.rs:518-524`, `:995`, `:1174-1181`.

Commands (read-only): `rg`, `wc`, `ls`, `du`, `find`, `git log/status/rev-parse`,
`sed`/`head` via the Read tool. No build, no test run, no `cargo`, no writes
other than this file. External sources: the URLs cited in §1.3/§3 (Claude Code
context-window docs, Claude platform compaction docs, aider lint/test docs,
ripgrep repository/guide, OpenAI reasoning guide); all other external statements
are marked "general practice".

---

## 7. Uncertainty and open questions

1. **Intent on the confirmation contradiction (F06).** The contradiction
   register resolves the Mandate side only. I cannot tell whether the maintainer
   intends arch 27's per-call confirmation to govern ordinary Build runs; if it
   does, F06 is the single most product-defining finding here, and if it does
   not, arch 27 needs a scope clause. Medium confidence on which side wins.
2. **Whether a second scheduling path exists (F09).** I read the daemon's
   startup, the acceptance path, and the terminal observer, and found no sweep
   for `Starting` runs; a client-driven path I did not read (e.g. a future
   subscription-triggered resume) could mask the stall. Medium confidence.
3. **Model-behaviour variance.** I did not run a real provider (prohibited and
   out of scope). Claims about timeout frequency, tool-name hallucination rates,
   and context overflow are labelled as general practice or as mechanism-plus-
   judgement, not measured on this system. A five-scenario live run
   (`make e2e-real-api` path exists) would settle them.
4. **`target/` numbers are this machine's.** The magnitude (hundreds of GB,
   hundreds of thousands of files) is typical for a Rust workspace of this size
   with `opt-level = 2` dev profiles, but the exact figures belong to this
   checkout.
5. **Effort estimates** are engineering judgement for a competent single
   implementer already familiar with the codebase, including tests and doc
   updates required by the project's own gates; they exclude review latency.
6. **Not audited here** (other zones): storage/transport internals beyond the
   paths cited, the quality harness's own implementation, the UI milestones'
   design, and the legacy-baseline fidelity questions. My verdicts on
   architectures 21/23/26/27/28/30 are judgements about daily benefit, not
   reviews of those documents' internal consistency (except F12).


---

# Appendix E — Zone 5: the unbuilt platform, block by block

Baseline: `main` @ `3291e50`, working tree clean except the untracked files the
audit was told to leave alone. Read-only audit: no tracked file was modified,
nothing was staged, no build or test was run.

---

## 1. Scope and method

### 1.1 Zone

Zone 5 is everything the repository commits to or contemplates but has not
built: the post-M5 foundation (Milestone 5+ slices 1-5), the ordinary-track
milestones M6-M9, the Mandate-track milestones M10-M12, and the Mandate-track
owner documents (architectures 13-30) with their decision records. The question
is not "is this well engineered" but "is this worth building, in this project,
under these conditions, and what happens the day it is used instead of
tested".

Project conditions, from `AGENTS.md` and the architecture itself: one local
user, one machine, one daemon, SQLite, no deployed users, no externally
persisted data, no third-party consumers, backward compatibility explicitly not
required. The only existence proof of the product is the legacy Antibusy
baseline in `docs/intention-relay/legacy-baseline/`, which is a desktop
coding-agent UI with sessions, projects, Plan/Build, model choice, plans,
`ask_user`/confirmation, tools, MCP, VFR, Headroom, Skills, and sub-agents.

### 1.2 Sources read (primary owner documents, not summaries)

- `docs/intention-relay/architecture/00`-`30` (all 31 owner documents; 13-30
  read in full).
- `docs/intention-relay/decisions/0001`-`0035`, `0037`, `0042`-`0049`
  (0001-0035 and 0042/0043 read in full; 0037/0044/0045/0046/0048 read in full;
  0036 skimmed as a superseded ledger).
- `docs/intention-relay/reconciliation/` (`README`, `deferred-excluded-register`,
  `contradiction-register`, `evidence-register` head).
- `docs/intention-relay/legacy-baseline/` (manifest, capability catalogue, user
  flows) as the only user-facing evidence of what the product does.
- Root `m5plus-slice1-security-theater-audit.md` (used for measured facts about
  the shipped Slice 1 surface and for the project's own unconsumed-surface bar;
  its conclusions are not the basis of this audit's judgments).
- Shipped code where the future platform depends on it: `intention-tools`,
  `intention-application`, `intention-domain`, `intention-tauri`,
  `intention-tui`, `quality/architecture.toml`, `Makefile`, `Cargo.toml`.

### 1.3 Stage vocabulary

| Stage | Meaning | Blocks |
| --- | --- | --- |
| `shipped` | M0-M5 code on `main` | 24 crates, 53,319 Rust lines |
| `committed` | M5+ slices 3-5 and M6-M9 approved in the roadmap | harness, caller policy, Goal domain, UI foundation, instruction channel, Tauri UI, Plan/Build, VFR/Headroom, hardening |
| `doc-only` | M10-M12 Mandate track and unactivated directions | architectures 13-22, 25-29; ADR 0020-0034 directions |

### 1.4 Method

1. Block inventory built from owner documents, not from register summaries:
   for each block I recorded owners, documentation volume, contract count
   (`*Dto`, `*V1`), closed failure codes, numeric bounds, activation gate, and
   stage.
2. Sizing: documentation lines per owner document; distinct contract names;
   closed failure codes counted from the code-fence identifier lines of
   architectures 13-30 (137 unique identifiers); numeric bounds counted from
   the limit tables.
3. Each block was tested against the four core questions (section 2.6) and
   against the simplest-sufficient-world test: what does a competent team
   building the minimal working local coding agent keep?
4. Cost model for effort estimates, derived by observation rather than
   intuition: the repository's Rust delivery is 53,319 lines across
   2026-07-31 to 2026-10-02 (63 days, `git log --reverse`), i.e. a net rate
   near 850 lines/day including tests and incidental documentation. Scales
   used in the cards: S = up to 5 days, M = 5-20, L = 20-60, XL = 60+.
   Effort numbers are the auditor's estimates and are labelled as such.
5. Every repo claim cites `file:line`; every number shows its derivation;
   external claims cite a URL or are marked "general practice".

### 1.5 What this audit is not

It is not a verdict on the shipped M0-M5 code (zones 1-4 territory), not a
re-review of ADRs 0044-0048 (those are already removals in the right
direction), and not an implementation plan. It does not speculate about other
auditors' conclusions.

---

## 2. Vectors examined -> verdicts

### 2.1 Block inventory

Documentation volume is measured with `wc -l` on the named owner documents;
contract counts are distinct `*Dto`/`*V1` names appearing in the owner
documents; failure codes are unique `snake_case` code identifiers in the
owner documents' code fences.

| # | Block | Owner docs (lines) | Contracts | Closed failures | Numbers | Gate | Stage | Verdict |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| B1 | Slice 1 contracts/versions | arch 02 (324), 14 (185); ADR 0036/0045/0046 | n/a | n/a | few | delivered | `shipped` | JUSTIFIED (post-cleanup shape) |
| B2 | Slice 2 control plane | arch 25 (281), 29 (259); ADR 0020/0024/0037/0044 | ~11 | 0 live | catalog tables, 30-min candidate | reverted | `doc-only` | EXCESSIVE as built; see F19 |
| B3 | Slice 3 harness | arch 26 (315), 27 (466), 28 (540), 13-continuation | 4 + 16 + 22 | 15 + 15 + 15 | 11 + 12 | M5+ | `committed` | EXCESSIVE (F07/F08/F16) |
| B4 | Slice 4 UI foundation | arch 23 (416), 24 (446) | 16 + 16 | 17 + 16 | 6 + 7 | M5+ | `committed` | MIXED (F10/F17) |
| B5 | Slice 5 instruction channel | arch 30 (364); ADR 0043 (291) | 3 | 3 | 4 | M5+ | `committed` | JUSTIFIED with simplification (F13) |
| B6 | M6 Tauri bridge + UI | roadmap M6 (~35), arch 24 adapter section | - | - | - | M5+ | `committed` | JUSTIFIED and under-planned (F02) |
| B7 | M7 Plan/Build + Autopilot | arch 07 (229); ADR 0017/0018 | shipped policy | - | - | M5+ | `committed` | JUSTIFIED |
| B8 | M8 VFR + Headroom | arch 06 (138) | shipped design | - | - | M5+ | `committed` | JUSTIFIED (cheap, product-proven) |
| B9 | M9 hardening | roadmap M9 (~50) | - | - | - | M6-M8 | `committed` | JUSTIFIED |
| B10 | M10 Mandate core | arch 13 (476), 14 (185), 16 (310); ADR 0001/2/3/6/8/31 | ~11 | 0 (0 vs. legacy) | 0 | M5+ | `doc-only` | EXCESSIVE (F05) |
| B11 | M11 execution plane | arch 15 (382), 17 (453), 19 (394); ADR 0004/7/9/11/25/27 | 26 | 28 | 9 + 16 | M10 | `doc-only` | EXCESSIVE/THEATER (F06/F09) |
| B12 | M12 capabilities/context | arch 18 (269), 20 (438), 21 (492), 22 (771) | 43 | 15 | 3 + 9 | M11 | `doc-only` | EXCESSIVE (F11/F18) |
| B13 | Cross-cutting directions | ADR 0033 (13 items), ADR 0034 (4 items) | - | - | - | M5+ | `doc-only` | MIXED (F18) |
| B14 | Traceability apparatus | reconciliation/ (3,617) | - | - | - | continuous | `committed` | BUREAUCRACY (F20) |

Totals for the unbuilt surface: 18 owner documents, 7,257 lines; 37 decision
records, 3,908 lines; 11,165 lines of normative text for zero lines of code.
Against that, every line of the architecture actually built (00-12) is 5,886
lines; the shipped code is 53,319 Rust lines.

### 2.2 Vectors examined, one verdict each

1. **M5+ slice 1 (contracts/versions)** — the cleanup (ADR 0044-0048) removed
   the canonical codec, digest layer, tag registry, capability plane, and the
   whole control plane; JSON-RPC 2.0 + typed serde JSON + one live schema
   remain. This is the one place where the project correctly diagnosed its own
   over-build and cut. Verdict: keep.
2. **M5+ slice 2 (control plane)** — built, shipped, and reverted within a
   month (ADR 0037 -> ADR 0044): 15 SQLite tables, 12 integration test targets,
   6 canonical goldens, catalog lifecycle with pending-removal/degraded modes,
   30-minute candidate expiry, held-run admission. Verdict: the direction is
   legitimate; the shape was not. See F19.
3. **M5+ slice 3 (harness + caller policy + Goal domain + autonomous
   continuation + tool/bridge/MCP contract selections)** — seven subsystems in
   one "pre-approved" slice. Verdict: EXCESSIVE (F07, F08, F16).
4. **M5+ slice 4 (forks + activity/notification + reasoning delivery + adapter
   boundaries + retention/GC)** — forks and reasoning delivery are real; the
   activity/notification/acknowledgement machinery models a multi-client
   distributed system inside one local process. Verdict: split; cut the
   messaging layer (F10, F17).
5. **M5+ slice 5 (instruction sources and system context)** — the legacy
   product has a static prompt set, `AGENTS.md`, and a memory file; the new
   product has no instruction channel at all until this slice. Verdict:
   JUSTIFIED, needs de-ceremonialising (F13).
6. **M6 (Tauri bridge + desktop UI)** — the only block with a user-visible
   surface; `crates/intention-tauri` is a 5-line skeleton with zero
   dependencies and no frontend directory exists. Verdict: highest-value
   block, least-planned block (F02).
7. **M7 (Plan/Build, physical plans, Autopilot, handoff)** — matches legacy
   capability and ADR 0017/0018 is an honest simplification. Verdict: keep.
8. **M8 (VFR + Headroom + retrieve)** — small, hook-shaped, proven in legacy.
   Verdict: keep.
9. **M9 (hardening)** — the correct place to close open decisions. Verdict:
   keep, but it is declared a gate for M10-M12 as well, which makes the
   Mandate track's acceptance depend on a milestone that cannot start until
   the Mandate track lands (roadmap dependency `N --> I`).
10. **M10 (Mandate lifecycle, execution meaning, scheduler)** — 971 lines of
    owner documents for a durable work-authority plane with 9 lifecycle
    states, 12 transaction classes, trigger reasons/coalescing, dispositions,
    mandatory uncertainty quarantine, and a scheduler that may only
    "reevaluate". Verdict: EXCESSIVE (F05), with one real idea worth keeping
    (fresh-run continuation, no-resume).
11. **M11 (registry, child graph, verifier authority, bridge)** — 1,229 lines
    of owner documents; the registry idea is needed, the child graph and
    verifier authority plane are not (F06), and the bridge is enforcement
    theatre by the architecture's own admission (F09).
12. **M12 (MCP, kernel, Goals/Skills/context, provider work)** — 1,970 lines
    of owner documents; the MCP capability path and the IPython kernel are
    real product needs; dynamic per-run acquisition, epoch/checkpoint
    machinery, and the Goal aggregate are not (F11, F18).
13. **ADR 0033 directions (13 items)** — most are reasonable product
    follow-ups (export, clone/rebind, reasoning history); two are autonomy
    directions whose cost exceeds the benefit (F04).
14. **ADR 0034 directions (4 remain)** — rich MIME kernel output and physical
    deletion/GC are small; worker/process supervision topology is a
    non-problem for a single-user daemon (F18).
15. **Cross-cutting doctrine (limits by precedent, no-content-scanning,
    no-backward-compatibility, no second X)** — the *doctrine* is the most
    valuable thing in the future corpus; its *application* is inconsistent
    (F12).
16. **Traceability apparatus** — seven registers, a 490-line source-of-truth
    matrix, an evidence register, and per-change hand-maintained status copies
    (F20). Verdict: keep the evidence anchors, cut the duplication.

### 2.3 Documentation-to-code proportionality

- Shipped: 24 crates, 53,319 Rust lines (tests included), architecture
  00-12 = 5,886 lines, so ~9.1 lines of Rust per line of v1 architecture.
- Unbuilt: architecture 13-30 = 7,257 lines and decisions 0001-0035/0042/0043
  = 3,908 lines, i.e. 11,165 lines of normative text with no code.
- The unbuilt platform therefore already carries **1.9x** the architectural
  prose of everything that exists, and specifies 137 closed failure codes and
  30+ code-owned numeric bounds before any of it is written.
- If the unbuilt stack reached the same code density as v1 (~9 lines of Rust
  per line of owner document), it would be ~100,000 Rust lines, i.e. roughly
  **two more full v1-sized deliveries** at the observed ~850 net lines/day
  (~120 working days of pure implementation), before counting M6-M9 work that
  does not yet exist either. This is a heuristic, not a plan; it is stated to
  show the order of magnitude the roadmap is committing to without saying so.

### 2.4 Core question 1 - Daily use: the first obstacle

Ordered by the moment a user meets them:

1. **No UI exists.** `crates/intention-tauri/src/lib.rs` is a 5-line skeleton;
   `crates/intention-tui/src/lib.rs` exposes only `connect` and `subscribe`;
   there is no `package.json` or `.svelte` file in the repository. The product
   cannot be used daily at all until M6, which is gated behind all of M5+
   (roadmap dependency `K[M5+] --> F[M6]`, `architecture/11-implementation-roadmap.md:30-42`).
2. **A long shell command during a Mandate run quarantines the work.** The
   shipped `execute` has an absolute 30-second deadline
   (`crates/intention-tools/src/lib.rs:146`, `:461`, `:477`) and the kill is
   classified `ExternalEffectUnknown`
   (`crates/intention-application/src/lib.rs:1095-1096`), which the Mandate
   track turns into `PausedAwaitingDecision` (architecture 13:368-372) and
   which "never permits another model step" (architecture 15:265). This is
   F01 and it is the single most likely first failure of the future platform.
3. **Autonomy requires per-call confirmation where it is available at all.**
   `execute` "never receives `DirectLocalRead`"
   (`architecture/27-programmatic-caller-policy-and-admission.md:196-204`), so
   on the ordinary path every shell command needs its own exact confirmation
   bound to one `ToolCallId`; the continual harness is read-and-delegate only
   (`architecture/26-continual-harness.md:180-193`). The unattended,
   write-capable agent therefore exists only on the Mandate track - the track
   with the least delivery evidence and the F01 problem. This is F04.
4. **Instructional content is fail-closed at admission.** A workspace
   `AGENTS.md` over 16,384 characters, or a fragment set over 100,000
   characters, rejects the run
   (`architecture/30-instruction-sources-and-system-context.md:254-258`,
   `:284-292`). This is F13.

### 2.5 Core question 2 - Full trace (user, data, request)

Trace for the promised headline scenario: "user states an objective, the agent
works autonomously, continues, and reports".

User side, per objective:
1. create Mandate (draft), 2. write objective/scope/mode/trigger configuration,
3. activate, 4. (for `execute` work on the ordinary path) confirm each call
exactly, 5. reconcile every `ExternalEffectUnknown` by hand, 6. accept or stop
at each disposition that needs a decision.

Data/request side, per accepted trigger:
trigger capture -> durable reason -> scheduler reevaluation -> readiness
observation -> candidate selection -> architecture-13 admission transaction
(new `RunId`, frozen selection, reason consumption, projections, events,
snapshots, idempotency) -> independent durable reread -> publication ->
model step -> tool group -> per call: admission transaction -> `ToolCallStarted`
-> each output fragment committed as its own durable fact + independent reread
+ publication -> terminal result -> next model step; plus a bridge grant per
model step (architecture 19), a kernel epoch per run (architecture 20), and an
MCP acquisition per run (architecture 18); then disposition -> continuation
reason -> a completely fresh run with a new `RunId`.

Counted steps, for one six-call group with 4 MiB of combined output (the
design's own ceiling, architecture 15:247-255):
- 6 admission transactions, 6 start facts, ~64 fragment commits at the 512 KiB
  fact bound, ~64 independent rereads, 1 group-completion transaction: about
  **140 durable transactions plus ~64 rereads per model step group**, where a
  conventional local agent design performs one process execution and one
  append.
- Human interventions: 1 per objective, plus 1 per exact confirmation on the
  ordinary path, plus 1 per uncertainty quarantine, plus 1 per user-decision
  disposition. For a shell-heavy task on the ordinary path this is O(number of
  `execute` calls), i.e. the opposite of autonomy; on the Mandate path it is
  1 + (number of >30 s commands).
- Latency: model-bound (seconds to minutes per step). The transaction
  multiplication is not the latency problem - local SQLite commits and rereads
  are microseconds to low milliseconds - it is the *implementation* cost: each
  of those ~140 transactions needs a fault-injection fixture, and the roadmap
  requires exactly that (architecture 15 "Required evidence", architecture 13
  "Required evidence before implementation").
- Human-days: see the cost column in section 3; the summary is that M10-M12 as
  specified is an XL program (60+ days each) for capabilities the user cannot
  exercise until M6 exists.

### 2.6 Core question 3 - Simplest-sufficient-world

A minimal working local coding agent for one user on one machine needs:
a daemon with durable sessions/runs (exists), a model driver (exists), tools
`read`/`write`/`edit`/`execute`/`glob`/`grep` (exists), a shell command that is
allowed to take longer than 30 seconds, a loop with tool results (exists), a
UI, Plan/Build, an instruction file, plan artifacts, VFR/Headroom, session
fork/regenerate, MCP as a configured list of servers, a sub-agent tool that
spawns a nested run and returns a summary, and a provider/model choice.

Everything else in architectures 13-30 is a mechanism for a problem the
minimal system does not have: durable cross-restart work authority (a single
user restarts when they choose), delegated verifier authority (the user reads
the diff), goal DAGs with obligatory children (a todo list), delegation
snapshots and graph epochs (one process owns everything), per-run MCP
re-acquisition (a configured server list), kernel epochs and namespace
checkpoints (a Python process), programmatic-caller policies (the OS user is
the caller), activity/notification/acknowledgement aggregates (one client, one
screen).

The surviving set from the unbuilt platform: M6 (UI), M7 (Plan/Build),
M8 (VFR/Headroom), M9 (hardening), slice 5 (instruction channel), a stripped
slice 4 (fork/regenerate with no rate limit, reasoning delivery), a stripped
`sub_agent` slot, a simple MCP capability path, and one honest autonomy
mechanism (accept a run, let it finish, do not resume anything after restart).

### 2.7 Core question 4 - Comparative practice

| Problem this project solves with a subsystem | How comparable systems do it | Source |
| --- | --- | --- |
| Sub-agents: immutable child Mandate edges, delegation snapshots, graph epochs, direct-parent control families, closed `sub_agent_*` failures (17 codes), tree bounds, classes | Claude Code subagents are Markdown files with YAML frontmatter (`name`, `description`, `tools`, `model`); the model picks one from its description and the subagent returns a summary; scope is resolved by directory precedence (`.claude/agents`, `~/.claude/agents`, plugins) | https://code.claude.com/docs/en/sub-agents |
| Autonomy and system safety: logical "trusted-local" policies that the architecture states are not security boundaries (`architecture/15-...:136-144`) | Codex CLI runs every spawned command inside an OS sandbox (Seatbelt on macOS, bubblewrap/Landlock on Linux) with `workspace-write` + `on-request` approvals as the default low-friction mode | https://developers.openai.com/codex/concepts/sandboxing |
| Compaction: immutable summaries with provenance, suffix, omission evidence, and a "working form" (architecture 21/28) | Claude auto-compacts conversation history at a token threshold and on demand; the summary replaces older context while the session continues | https://platform.claude.com/docs/en/build-with-claude/compaction |
| Scheduled/recurring agent work: a durable harness model with rules, trigger capture, coalescing, catch-up, DST semantics, dossiers, checkpoints, classes, and 11 bounds (architecture 26) | General practice is external scheduling: cron / CI / a wrapper CLI that starts a fresh agent session per schedule; vendors and users wrap the agent rather than build a scheduler inside it | https://codex.danielvaughan.com/2026/03/27/codex-cli-automations-scheduled-tasks/ (general practice; see section 7 uncertainty) |
| Approval policy: durable policy revisions, three scopes, root origins, narrowing intersections, exact per-call confirmations | General practice: a per-session permission mode plus an "always allow this tool" toggle, or an OS sandbox that removes the need to ask | general practice |

The pattern: successful systems put the boundary in the operating system or in
a one-line configuration, and spend their complexity budget on the model loop.
This project puts the boundary in a product-level authority ontology and leaves
the OS boundary explicitly unclaimed.

### 2.8 Core question 5 - Future amputation

The evidence for what will be cut is already in the repository: the
project shipped a control plane and reverted it inside a month
(ADR 0037 -> ADR 0044), deleted an entire canonical codec and contract ledger
(ADR 0045/0046), and removed a limit/corridor/period engine that never reached
code (ADR 0048). Every one of those cuts removed *contract-ahead-of-consumer*
material - exactly the shape of slices 3-5 and the whole Mandate track. The
prediction section (section 5) names the signals.

---

## 3. Findings (ranked cards)

Severity: S1 breaks daily use; S2 rewrite or major drag; S3 tolerable tax;
S4 minor. Effort: S <= 5 days, M = 5-20, L = 20-60, XL = 60+ (auditor
estimates, anchored on the observed ~850 net Rust lines/day, section 1.4).
"Cost of delay" is what it costs to discover the problem later instead of now.

---

### Z5-F01 - A 31-second command quarantines autonomous work

- **Stage:** `shipped` (mechanism) -> fires in `doc-only` (M10-M12 consumer).
- **Type:** LANDMINE.
- **Severity:** S1.
- **Trigger:** during a Mandate run (M10-M12) the agent runs `cargo test`,
  `cargo build`, `npm install`, `pytest`, or any command that takes longer than
  30 seconds. The command is killed and classified unknown; the Mandate moves
  to mandatory uncertainty quarantine; no further model step is permitted;
  automatic continuation is blocked until the user performs an authorized
  reconciliation. A coding agent that cannot run a test suite for more than
  30 seconds is unusable as an autonomous worker.
- **Mechanism:** `crates/intention-tools/src/lib.rs:146` (`EXECUTE_TIMEOUT =
  Duration::from_secs(30)`), `:461` (absolute `deadline = Instant::now() +
  timeout`), `:477` (kill at deadline, `Err("tool_execute_external_effect_unknown")`),
  `:508`, `:524` (same classification on drain stall and wait error);
  `crates/intention-application/src/lib.rs:1095-1096` maps that code to
  `ToolLifecycleStatusDto::ExternalEffectUnknown`;
  `docs/intention-relay/architecture/13-...:368-372` turns an unknown started
  effect into `PausedAwaitingDecision`;
  `docs/intention-relay/architecture/15-...:333` and `:265` state that a
  started action without proven terminal effect is `ExternalEffectUnknown` and
  that such a result "never permits another model step". Architecture 05, the
  owner document for tools, never mentions the 30-second bound
  (`rg 'timeout' docs/intention-relay/architecture/05-tools-workspace-and-hooks.md`
  returns nothing), so the deadline is an undocumented product ceiling in
  shipped code - the exact class ADR 0048 forbids.
- **Blast radius:** UX: the agent stops mid-task and demands manual
  reconciliation. Human-days: every quarantine is a user operation; a
  test-driven task can produce several per hour. Perf: none. Money: none
  directly, but a stalled Mandate holds a `Working` slot and blocks
  continuation.
- **Simplest alternative:** a locally killed, reaped child with a known
  deadline is a *known* terminal timeout for its owning call - classify it as
  `ToolCallTimedOut` (a known pre-effect-equivalent outcome for the loop), and
  reserve `ExternalEffectUnknown` for genuine loss of the process or of durable
  evidence. If the risk is escaped grandchildren, make the deadline an
  *idle/progress* timeout (no output for N seconds), which is what the retained
  liveness-safeguard list already describes ("a child process that stops
  producing progress or never exits", ADR 0048 keep-list row), or raise the
  bound with a recorded precedent. Any of the three is a few lines plus tests.
- **Disposition:** rewrite the classification before M10 activates; document
  the bound or make it progress-based.
- **Effort:** M (3-5 days: classification, runtime status mapping, fixtures,
  doc update).
- **Confidence:** high (code and documents read directly; the only uncertainty
  is whether M10 activation changes the tool contract first - nothing in the
  roadmap says it does).
- **Cost of delay:** the first Mandate implementation will build an autonomous
  loop on top of a primitive that cannot run a real build, discover this in
  integration, and then have to change a shipped, versioned, coverage-gated
  tool contract while the Mandate track already depends on it.

---

### Z5-F02 - The daily-use surface is the least-planned block, and it is gated behind everything else

- **Stage:** `committed` (M6) with `doc-only` pretensions (Mandate track).
- **Type:** EXCESSIVE (scope allocation).
- **Severity:** S2.
- **Trigger:** a user tries to use the product. There is no UI: no frontend
  directory, no `package.json`, no `.svelte` file;
  `crates/intention-tauri/src/lib.rs` is a 5-line compile-only skeleton with an
  empty `Cargo.toml` dependency list; `crates/intention-tui/src/lib.rs` (46
  lines) exposes `connect` and `subscribe` only. M6 is the only block that
  produces a usable surface, and the roadmap makes it depend on the completion
  of all of M5+ (`architecture/11-implementation-roadmap.md:30-42`, ADR 0035
  invariant 1). M5+ slice 3 alone is seven subsystems
  (`architecture/11-implementation-roadmap.md:480-500`).
- **Blast radius:** human-days: the product's time-to-first-daily-use is
  measured in the M5+ slice sequence plus M6, and the plan spends its
  specification budget on the Mandate track instead (7,257 lines for 13-30
  versus ~35 lines for M6 in the roadmap). UX: nothing exists to judge.
- **Simplest alternative:** reorder. Slice the foundation to what M6/M7/M8
  actually consume (instruction fragment editing, fork/regenerate, activity
  projections for the one client, reasoning delivery), ship the desktop UI, and
  activate the harness/Goal/Mandate packages only when a user has asked for
  them in daily use. The roadmap's own stack already assumes a consumer exists;
  build the consumer first.
- **Disposition:** re-scope and reorder; keep M5+ as a dependency of M6-M9 but
  cut it down to M6-M9's real contract needs.
- **Effort:** L to a usable daily surface (M6 + M7 minimum: 40-80 days by the
  same rate model); the re-scoping itself is S (1-2 days of roadmap edits).
- **Confidence:** high on the facts (no UI exists, M6 depends on M5+), medium
  on the remedy (reordering is a product decision).
- **Cost of delay:** every further month of Mandate-track specification is a
  month in which the only daily-usable artifact remains the legacy Antibusy
  application the rewrite is meant to replace.

---

### Z5-F03 - "No retroactive contract change" is a guarantee the project has already broken once

- **Stage:** `committed` (M5+ exit criteria) with `doc-only` consumers.
- **Type:** LANDMINE.
- **Severity:** S2.
- **Trigger:** the first real M6/M7 consumer arrives. M5+ acceptance requires
  proof that "M6, M7, M8, and M9 can each begin against stable contracts
  without any retroactive protocol, DTO, schema, crate-boundary, migration, or
  quality-policy change" (`architecture/11-implementation-roadmap.md:575-577`;
  ADR 0035 "Rationale" and invariant 5). Slice 1 attempted exactly this and
  failed: the project's own audit measured 3,646 production lines and 4,698
  test lines across the four Slice 1 files with **zero non-test consumers**
  (`m5plus-slice1-security-theater-audit.md:63-67`), and ADRs 0044-0048 then
  deleted the whole surface (capabilities, canonical codec, contract families,
  six Slice-2 tags returning to reserved status -
  `decisions/0044-...md` "Reverted surfaces", `decisions/0046-...md`).
- **Blast radius:** human-days: a mid-milestone contract rewrite of the size
  the project has already performed once (the Slice 1 removal touched domain,
  protocol, storage, config, daemon, client, twelve test targets, six goldens,
  and a dependency). UX: none directly.
- **Simplest alternative:** invert the guarantee. Author contracts *with* their
  first consumer inside the same slice, and accept that a slice may revise its
  own contracts before exit; keep the "no second runtime / no second registry"
  invariants, drop the "no retroactive DTO change" exit criterion.
- **Disposition:** relax the exit criterion; keep the invariant that a *shipped*
  contract is versioned and fail-closed.
- **Effort:** S to change the policy (1-2 days); the avoided rework is L
  (20-60 days).
- **Confidence:** high (the failure already happened and is measured in-repo).
- **Cost of delay:** the next slice authored ahead of its consumer repeats the
  measured failure; each repetition costs the full build-plus-delete cycle.

---

### Z5-F04 - Autonomy is split across two incompatible planes, and neither one works unattended

- **Stage:** `committed` (harness, slice 3) and `doc-only` (Mandate, M10-M12).
- **Type:** MIXED.
- **Severity:** S2.
- **Trigger:** a user asks for the headline behaviour - "work on this while I
  am away". On the ordinary/harness plane the run is read-and-delegate only
  (`architecture/26-continual-harness.md:180-193`: allowed tools are `read`,
  `glob`, `grep`, `expand`, `retrieve`, `sub_agent`), `ask_user` is forbidden
  to a harness (`:186-193`), and harness `sub_agent` use requires an exact
  confirmation that must already exist for the call
  (`architecture/27-...md:236-246`). On the Mandate plane everything is
  directly admitted with no confirmation (`architecture/15-...md:180-190`),
  but every long command quarantines the work (F01) and the whole plane is
  unbuilt. The user therefore has a read-only robot that may be allowed to
  spawn a confirmed child, and a write-capable robot that stops at the first
  31-second command.
- **Blast radius:** UX: the capability the platform is named for (relaying
  intent into autonomous work) is not reachable on either plane. Human-days:
  the confirmation-per-call model on the ordinary plane is O(shell calls).
- **Simplest alternative:** one autonomy plane. Take the Mandate track's one
  good idea - "continuation is a fresh admitted run, never a resume" - and
  apply it to ordinary runs: an explicit opt-in "continue" flag per session
  with the existing confirmation policy, a per-session spend/step budget shown
  in the UI, and the F01 fix. Then delete the separately-specified Mandate
  aggregate, scheduler, and quarantine law.
- **Disposition:** collapse the two planes; keep fresh-run continuation
  semantics.
- **Effort:** L as specified per plane; M (10-20 days) for the collapsed
  design plus the F01 fix.
- **Confidence:** medium-high (the constraint conflict is explicit in the
  documents; the remedy is a design judgment).
- **Cost of delay:** building both planes costs the sum and delivers neither.

---

### Z5-F05 - The Mandate authority plane is a governance system for a single user

- **Stage:** `doc-only` (M10).
- **Type:** EXCESSIVE.
- **Severity:** S2.
- **Trigger:** routine autonomous work. The user must author a durable
  revisioned work authority (objective, scope, mode, trigger configuration,
  continuation configuration, stop conditions), then live with a closed
  9-state lifecycle with explicit transitions
  (`architecture/13-...md:120-160`), 12 transaction classes (`:280-300`),
  trigger reasons with idempotency and coalescing, deterministic total
  ordering, disposition records, mandatory uncertainty quarantine, and exact
  reconciliation. Money blast radius: ADR 0048 forbids any product ceiling on
  Mandate admission ("no product quota, count, calendar cap, lifetime cap,
  output cap, concurrency cap" - `architecture/13-...md:322-329`), so
  autonomous continuation can loop and spend provider money with only a
  user-authored `stop_conditions` field as a guard.
- **Blast radius:** human-days: XL to build and to test (the "Required
  evidence" list alone has 11 fixture families, `architecture/13-...md:440-460`);
  UX: the user must learn an authority vocabulary before the agent will work.
- **Simplest alternative:** runs and a queue already exist and work. Add an
  explicit "autonomous continuation" boolean plus a user-visible budget, and
  drop the aggregate, scheduler, and quarantine states to a single rule
  ("never resume old external work after restart; start a fresh run").
- **Disposition:** re-scope to the continuation rule; defer the authority
  plane indefinitely.
- **Effort:** XL as specified (60+ days); M (10-15) simplified.
- **Confidence:** high on cost and complexity, medium on the user's eventual
  preference.
- **Cost of delay:** M10 is a prerequisite of M11 and M12; committing to it
  commits the whole Mandate track to its vocabulary.

---

### Z5-F06 - Child graph and delegated verifier authority: a distributed-systems identity plane for one process

- **Stage:** `doc-only` (M11).
- **Type:** EXCESSIVE.
- **Severity:** S2.
- **Trigger:** the agent spawns a helper. Required machinery: an immutable
  direct edge, a delegation snapshot, a root-graph identity, a parent revision,
  a graph epoch, directed control messages with a shared monotonic order,
  terminalization closure, a daemon-owned safety cascade, and - for review - a
  separately user-issued target-scoped verifier authority with immutable target
  sets, audit contracts, frozen baselines, and a verdict/mutation matrix
  (`architecture/17-...md:50-160`, `:230-300`). 17 closed `sub_agent_*` failure
  codes and 3 classes with fixed step limits (64/256/1,024) and a 360-minute
  lifetime (`:304-345`).
- **Blast radius:** UX: a user who asks for a second opinion gets `sub_agent_
  depth_limit_exceeded` at grandchildren or a 360-minute lifetime expiry; the
  verifier path cannot be used at all without first issuing an authority
  document. Human-days: L.
- **Simplest alternative:** Claude Code's model - a subagent is a named
  instruction file with a tool subset; the parent spawns it, gets a summary
  back, and can cancel it. Authority is the user's, because the parent is
  already running as the user and the session is the same process. Comparable
  systems do not model verifier authority at all; the human reviews the diff.
- **Disposition:** replace with a nested-run `sub_agent` tool and a summary
  result; delete the verifier authority plane.
- **Effort:** L (25-40 days) as specified; S-M (5-10) for the nested-run tool.
- **Confidence:** high.
- **Cost of delay:** M11's exit criteria require child-edge, delegation,
  verifier-authority, target-mutation, and conflict-precedence fixtures: five
  test families for a capability most users never invoke.

---

### Z5-F07 - The Goal aggregate: acceptance, gates, memory, roles, templates, proposals, and compaction for a personal project

- **Stage:** `doc-only` (slice 3 detail, M5+).
- **Type:** EXCESSIVE.
- **Severity:** S2.
- **Trigger:** the user wants the agent to remember what "done" means. The
  design provides a Goal DAG with obligatory children, project/session scope
  with explicit applicability links, five lifecycle states, three readiness
  states, three user-decision states including `AcceptedWithException`,
  reference and executable gates with user-created typed templates, four memory
  kinds across three scopes, roles as reusable `sub_agent` templates, model
  proposals with acceptance flows, a cumulative conversation compaction working
  form, 28 closed failure codes, and a 12-row bounds table up to 256 goals per
  project (`architecture/28-...md:54-160`, `:214-300`, `:437-460`).
- **Blast radius:** UX: the user must create and maintain goal trees, gates,
  templates, roles, and memory records before receiving value; the design
  itself concedes Goals "are acceptance/evidence records, not the
  work-authorization plane" (`:16-20`), i.e. the whole system grants nothing.
  Human-days: XL.
- **Simplest alternative:** the instruction file plus a todo tool (the legacy
  product already had todos) plus the plan artifact (M7). Memory, when it
  arrives, is one append-only markdown file the user can read and edit.
- **Disposition:** cut; keep the plan artifact and todos.
- **Effort:** XL (60+ days); S (3-5 days) for todos + instruction file.
- **Confidence:** high.
- **Cost of delay:** slice 3 cannot ship until this block is ready, and slice 3
  gates M6.

---

### Z5-F08 - Programmatic-caller policy: an authorization engine where the caller is the user

- **Stage:** `doc-only` (slice 3 direction, arch 27).
- **Type:** BUREAUCRACY.
- **Severity:** S2.
- **Trigger:** any tool call from any path. Every call resolves root origin,
  durable provenance, applicable policies across three scopes with inheritance
  and narrowing, an immutable effective-policy snapshot, a closed decision
  vocabulary, exact per-call confirmations bound to one `ToolCallId`, policy
  lifecycle (suspend/revoke/archive with cancellation cascades), and model-drafted
  policies with user acceptance (`architecture/27-...md:80-180`, `:215-300`),
  with 15 closed `programmatic_policy_*` failures.
- **Blast radius:** UX: for a local single-user tool the caller is always the
  same OS user; the policy adds a vocabulary without adding a boundary. The
  document says so itself: "This policy is logical product control and audit
  evidence, not an operating-system security boundary"
  (`architecture/27-...md:14-16`), and the `excluded` register refuses sandbox
  authority entirely (EXC-006,
  `reconciliation/deferred-excluded-register.md`).
- **Simplest alternative:** the existing confirmation prompt plus a per-session
  "allow this tool" toggle, and, if the risk matters, an OS sandbox (Codex
  model) instead of a product-level policy engine.
- **Disposition:** cut to a confirmation policy; keep provenance as a log line.
- **Effort:** L (20-30 days) as specified; S (3-5) simplified.
- **Confidence:** high.
- **Cost of delay:** slice 3 must implement policy snapshots before any
  ordinary run can record its selection, coupling every later feature to it.

---

### Z5-F09 - The Gateway/RLM bridge is enforcement theatre by the architecture's own admission

- **Stage:** `doc-only` (M11/M12).
- **Type:** THEATER.
- **Severity:** S2.
- **Trigger:** the IPython kernel wants to call a tool. The design routes it
  through an ephemeral grant, an immutable bridge-contract selection, durable
  operation correlation, a `BridgeOperationId`, idempotent replay rules,
  epoch fencing, a concurrency bound of 16 unfinished operations, a 64-frame
  10-second slow-peer path, and six closed `bridge_*` failures
  (`architecture/19-...md:60-160`, `:230-320`) - while the same corpus states
  that the kernel "can bypass the facade via `pathlib`, `os`, and
  `subprocess`; this is an accepted property" and that the capability plane is
  "logical product/safety policies, not security boundaries"
  (`architecture/15-...md:136-144`). The bridge therefore cannot prevent
  anything it is designed to make auditable; it only decides whether the agent
  uses the front door.
- **Blast radius:** human-days: L to build plus a permanent coupling of every
  kernel feature to bridge grant lifetime; UX: kernel calls fail after grant
  expiry and are "never queued" (`architecture/20-...md:270-280`), so a
  background task that outlives its foreground cell silently loses its tools.
- **Simplest alternative:** because the kernel runs as the user anyway, give it
  the same local capability path the daemon uses (an in-process call or a
  single local socket method) and record provenance. If a real boundary is
  wanted, use an OS sandbox (bubblewrap/Landlock, as Codex does) rather than a
  grant protocol the sandboxed process can walk around.
- **Disposition:** simplify to an in-process/typed invocation path; delete
  grants, operation correlation, and replay machinery.
- **Effort:** L (30-50) as specified; M (10-15) simplified.
- **Confidence:** high (the bypass is stated in the owner document).
- **Cost of delay:** the bridge's semantics are referenced by kernel, MCP,
  child, and activity documents; changing it later touches all of them.

---

### Z5-F10 - Activity, notification, and acknowledgement: a multi-client messaging layer inside one local process

- **Stage:** `committed` (slice 4, M6).
- **Type:** THEATER.
- **Severity:** S2.
- **Trigger:** the user watches the agent work. The design introduces an
  `AgentActivityTreeId` distinct from session and conversation trees
  (`architecture/24-...md:60-90`), a direct-pair message exchange with 16-slot
  and 512 KiB per-direction queues and reserved clarification slots
  (`:120-140`), an append-only activity journal with 17 closed record kinds
  (`:150-190`), a separate notification journal with its own cursor, urgent
  dedup by `(AgentActivityTreeId, cancellation reason)`, and a durable
  acknowledgement aggregate whose only function is to mark presentation state
  without rewriting notification records (`:250-300`).
- **Blast radius:** UX: none of this is visible as a capability; it is
  plumbing whose observable output is "what the agent is doing", which the
  shipped session/run event stream already provides. Human-days: L; and every
  future feature that has something to report must choose among three
  sequences plus provenance references.
- **Simplest alternative:** one client, one screen: render session/run events
  plus a small typed activity list; keep notifications as in-app summaries
  derived from the same stream; drop the acknowledgement aggregate and the
  cross-tree identity model.
- **Disposition:** collapse to a projection over existing session/run facts.
- **Effort:** L (20-30) as specified; S (5-8) as a projection.
- **Confidence:** high.
- **Cost of delay:** M6 is specified to consume this surface; the UI will be
  built against a model that has no user-facing justification.

---

### Z5-F11 - MCP: per-run acquisition instead of a configured server list

- **Stage:** `doc-only` (M12).
- **Type:** EXCESSIVE.
- **Severity:** S3.
- **Trigger:** the user connects an MCP server. The design acquires
  capabilities at runtime per run under the fixed `mcp` slot, normalizes
  schemas into closed families, freezes an accumulated selection revision per
  model step, disposes local stdio resources when the run ends, and fails a
  schema mismatch closed (`architecture/18-...md:60-160`, `:200-260`). The
  legacy product instead exposes MCP definitions in configuration, and the
  deferred register explicitly *excludes* the user-created catalog that the
  legacy product has (EXC-013, `reconciliation/deferred-excluded-register.md`).
  Practical consequence: every run pays discovery latency again, and a server
  that publishes a changed schema (common with `npx`-style servers) makes the
  running work fail with `mcp_schema_mismatch` instead of degrading.
- **Blast radius:** UX: repeated discovery round-trips and an unstable surface
  across runs; human-days: M.
- **Simplest alternative:** a configured list of servers and methods (typed,
  user-edited, versioned at startup) plus re-validation before each call, which
  is what the legacy product and comparable CLI agents do; keep the fixed `mcp`
  slot and the one capability path.
- **Disposition:** simplify; keep the fail-closed schema check at call time.
- **Effort:** L (15-25) as specified; M (8-12) simplified.
- **Confidence:** medium-high.
- **Cost of delay:** M12 acceptance includes MCP discovery/selection/invocation
  /disposal/safe-projection fixture families.

---

### Z5-F12 - The limits doctrine is stated, then contradicted by 30+ surviving hard numbers

- **Stage:** `committed` and `doc-only`.
- **Type:** BUREAUCRACY.
- **Severity:** S3.
- **Trigger:** implementing any future block. ADR 0048 requires every numeric
  limit to name the failure mode it prevents, its unit, and its behavior at the
  bound (ADR 0048, "Limits only by real precedent", item 4), and CON-003
  records that "speculative contract limits are removed and none may be added
  without a real demonstrated precedent"
  (`reconciliation/contradiction-register.md` CON-003). Yet the future owner
  documents still carry code-owned tables with no precedent: child tree
  16/3/0/2/64/16 and a 360-minute lifetime
  (`architecture/17-...md:304-315`), kernel 60-minute idle / 16 live kernels /
  ten-minute cell (`architecture/20-...md:328-334`), harness 64 rules / 16
  sources / 512 KiB dossier / 256 launches (`architecture/26-...md:195-210`),
  Goal 256/64/16/32/64/32/128/32 and 512 KiB-4 MiB content bounds
  (`architecture/28-...md:437-455`), fork 4,096 depth / 16,384 descendants /
  16 forks per hour (`architecture/23-...md:310-320`). Architecture 24 is the
  only owner document that explicitly reconciles with ADR 0048
  (`architecture/24-...md:1-12`).
  Drift is already visible: architecture 15 still states "A group contains at
  most **16 calls**" and the 512 KiB / 4 MiB / 256-fact bounds
  (`architecture/15-...md:200`, `:247-255`, `:281`), while CON-081 records the
  16-call cap as "removed as a speculative limit"; the roadmap still assigns
  calendar/interval/time-zone/DST semantics to M10
  (`architecture/11-implementation-roadmap.md:846`, `:869`, `:885`, `:1933`,
  `:1940`) while ADR 0048 removed EXC-011 and its register row now reads
  "future scheduler (withdrawn)".
- **Blast radius:** human-days: each surviving number needs classification,
  fixture design, and documentation justification; the drift proves the
  documentation is not self-consistent, so an implementer cannot trust either
  artifact.
- **Simplest alternative:** one pass over architectures 13-30 that either
  deletes each number or adds its precedent sentence in place, and a docs
  check that fails when a numeric bound appears in an owner document without a
  precedent line. The 16 forks/hour rate limit is the clearest candidate for
  deletion: regenerating a response repeatedly is normal user behaviour.
- **Disposition:** reconcile in place; add the check.
- **Effort:** S (2-4 days of documentation work).
- **Confidence:** high (deterministic grep evidence).
- **Cost of delay:** every activation inherits ambiguous limits and copies the
  drift into a new specification.

---

### Z5-F13 - A long `AGENTS.md` blocks the run

- **Stage:** `committed` (slice 5).
- **Type:** LANDMINE.
- **Severity:** S3.
- **Trigger:** the user grows `AGENTS.md` past 16,384 characters, or their
  fragment set past 100,000 characters, or enables more than 64 fragments. Run
  admission fails with `instruction_projection_too_large` or
  `instruction_source_unavailable`; there is "no fallback revision, partial
  projection, silent omission, truncation, sampling, or historical
  substitution" (`architecture/30-...md:254-258`, `:284-292`).
- **Blast radius:** UX: the agent refuses to start, and the error names an
  intrinsic bound rather than telling the user which fragment is too long; a
  monorepo `AGENTS.md` of the size large projects actually use is enough.
- **Simplest alternative:** keep a hard bound at the transport/model boundary,
  but at admission drop the offending source (or truncate with an explicit
  `[truncated]` marker) and record a typed omission in the projection; reserve
  fail-closed for unreadable or non-UTF-8 input.
- **Disposition:** simplify failure behavior.
- **Effort:** S (1-2 days of design plus fixtures).
- **Confidence:** medium-high (bound is explicit; the daily likelihood depends
  on the user's repo).
- **Cost of delay:** the slice-5 fixtures are specified as fail-closed tests,
  so the behavior gets locked in by coverage requirements.

---

### Z5-F14 - Freeze-everything applied to advisory content

- **Stage:** `committed` and `doc-only`.
- **Type:** BUREAUCRACY.
- **Severity:** S3.
- **Trigger:** the user edits configuration expecting the agent to pick it up.
  The design freezes at admission everything downstream of a selection:
  instruction projection (inherited verbatim by forks and handoffs,
  `architecture/30-...md:200-230`), Skill reveals, context manifests and
  model-step projections (`architecture/21-...md:250-290`), Goal/Skill selections,
  and provider selections. Only provider/tool/lifecycle *authority* needs that
  rigor; instructional and context content does not, and the incompatibility
  the freeze does not prevent is "the agent used yesterday's instruction text".
- **Blast radius:** UX: a fork or handoff can run for weeks with stale
  instructions, and the user has no way to refresh it in place (a new fork is
  the only path); human-days: S per incident, but repeated confusion.
- **Simplest alternative:** keep frozen identity/authority selections, but
  re-derive advisory content (instruction fragments, `AGENTS.md`, Skill cards,
  memory cards) at each model step, recording the revision actually used for
  reproducibility. The legacy product already documents this expectation as a
  *limitation* users notice ("Saving this configuration does not rebuild the
  already-created pipeline",
  `legacy-baseline/02-capability-catalog.md:105-120`).
- **Disposition:** split the freeze rule by class; re-derive advisory content.
- **Effort:** M (5-10 days of design/spec work).
- **Confidence:** medium.
- **Cost of delay:** the freeze rule is already referenced by forks, plans,
  handoffs, and the instruction channel; changing it later fans out.

---

### Z5-F15 - Evidence-before-implementation has grown past the point of proportionality

- **Stage:** `committed` / `doc-only`.
- **Type:** BUREAUCRACY.
- **Severity:** S3.
- **Trigger:** starting any activating slice. Each slice must declare crates,
  DTO/wire/storage versions, feature profiles, coverage declarations under
  ADR 0049, fixtures, and outcome evidence *before* code; slices 3-5 alone
  carry 32 planned evidence rows in the evidence register (EVD-013..EVD-035),
  the future corpus defines 137 closed failure codes, and the exit criteria
  require Linux **and** Windows CI plus `make quick`, `make verify`,
  docs-check, architecture-check, and coverage for each slice
  (`architecture/11-implementation-roadmap.md:645-660`).
- **Blast radius:** human-days: every slice pays the whole gate matrix; the
  project's own audit measured the Slice 1 test burden as ~4,698 test lines for
  3,646 unconsumed production lines
  (`m5plus-slice1-security-theater-audit.md:63-70`), i.e. the evidence machine
  can be paid for before value exists. UX: none.
- **Simplest alternative:** bind evidence to the consumer, not to the slice:
  declare contracts when the consumer lands, cap per-slice evidence at the
  scenarios that exercise the delivered behaviour, and let the base 80%
  coverage rule plus the architecture check carry the rest.
- **Disposition:** right-size per slice; keep the base gates.
- **Effort:** S (2-3 days to re-specify the acceptance rule).
- **Confidence:** medium-high.
- **Cost of delay:** the burden is now the default expectation; every new block
  inherits it.

---

### Z5-F16 - Slice 3 bundles seven subsystems into one pre-approved, all-or-nothing slice

- **Stage:** `committed`.
- **Type:** EXCESSIVE.
- **Severity:** S3.
- **Trigger:** attempting slice 3. The deliverable activates the continual
  harness, programmatic-caller policy, Goal domain, autonomous continuation,
  the tool-descriptor/registry/model-tool-loop contracts, the bridge-invocation
  and MCP-method-catalog selections, and the harness-side ADR 0033 directions
  (`architecture/11-implementation-roadmap.md:480-500`), under the rule that
  "No slice may ship in a half-ready state" and that later slices consume only
  contracts activated by earlier ones (ADR 0035 invariants 3-5).
- **Blast radius:** human-days: the slice is weeks-to-months of work that
  cannot ship partially, so the UI (M6) waits for the slowest of seven
  subsystems. UX: delivery delay.
- **Simplest alternative:** split slice 3 by consumer readiness: the two
  contracts M6 actually needs (tool registry/loop, activity projection) ship
  first; the harness, caller policy, and Goal domain ship only when a user asks
  for them.
- **Disposition:** re-slice.
- **Effort:** S (1-2 days of roadmap work) to avoid an L-sized scheduling
  failure.
- **Confidence:** medium-high.
- **Cost of delay:** the slice is the declared hard prerequisite of M6-M9.
- **Note:** this finding overlaps F02; it is recorded separately because the
  remedy (re-slicing) is different from reordering milestones.

---

### Z5-F17 - Fork rate limit and tree ceilings are user-visible caps that no precedent justifies

- **Stage:** `committed` (slice 4).
- **Type:** BUREAUCRACY.
- **Severity:** S3.
- **Trigger:** the user regenerates a response repeatedly (normal behaviour
  when a model answers badly) or branches while exploring: "Forks from one
  source boundary | 16 in a rolling hour | reject with `fork_boundary_rate_limit`"
  (`architecture/23-...md:317-319`), plus depth 4,096 and 16,384 descendants
  (`:310-320`). CON-057 resolves this by declaring fork limits "ordinary
  Session policy only, never Mandate admission or child-graph quotas"
  (`reconciliation/contradiction-register.md` CON-057) - i.e. the precedent
  rule is satisfied by *reclassifying* the number rather than by justifying it.
- **Blast radius:** UX: a user is told to stop regenerating for an hour;
  human-days: minutes per incident, but it is exactly the kind of arbitrary
  refusal that erodes trust.
- **Simplest alternative:** keep only bounds that protect a resource (snapshot
  size, transport frame) and drop the rate limit and fantasy tree ceilings.
- **Disposition:** remove the rate limit; keep a size bound.
- **Effort:** S (1 day of documentation change).
- **Confidence:** medium-high.
- **Cost of delay:** once implemented with fixtures, removing it costs a
  published failure code.

---

### Z5-F18 - Directions with no consumer: supervision topology, deletion/GC, rich MIME

- **Stage:** `doc-only` (ADR 0034 directions, assigned to M11/M12/slice 4).
- **Type:** DEAD.
- **Severity:** S3.
- **Trigger:** building M11/M12. ADR 0034 adopts "worker/process supervision
  topology" for architecture 03 (M11,
  `decisions/0034-...md` item 3), "physical deletion/GC of historical work"
  (slice 4), and "rich MIME/raw kernel output projection" (M12). For a
  single-user daemon that already uses `std::process::Command` with a process
  group kill (`crates/intention-tools/src/lib.rs:425-530`), a supervision
  topology solves no observed problem; physical deletion contradicts the
  archive-only default that no user has asked to change; rich MIME output is
  excluded from the model-facing projection anyway
  (`architecture/20-...md:150-190`).
- **Blast radius:** human-days: each is a distinct implementation plus fixtures;
  UX: none until built.
- **Simplest alternative:** do nothing. These are classic "directions adopted
  to close register rows" (`decisions/0034-...md` "Rationale" explicitly says
  the items were adopted so they would not remain indefinitely deferred).
- **Disposition:** return to `Defer` with a stated reconsideration trigger.
- **Effort:** S (1 day of register work); the avoided cost is M/L.
- **Confidence:** high.
- **Cost of delay:** they inflate the exit criteria of M11/M12 and slice 4.

---

### Z5-F19 - The provider control plane will be rebuilt as specified, because nothing replaced it

- **Stage:** `doc-only` (reverted Slice 2; arch 25/29/22).
- **Type:** EXCESSIVE.
- **Severity:** S4.
- **Trigger:** the user wants to choose a model per session, which the legacy
  product does from the UI (`legacy-baseline/02-capability-catalog.md:43-53`)
  and which the current shipped platform cannot do at all (`provider_profiles_v1`
  and session selection were removed by ADR 0044). The documented
  re-introduction scope is the full Slice 2 shape: catalog lifecycle with
  pending-removal candidates and a 30-minute expiry, degraded read-only
  recovery, activation-recovery states, pricing policy, credential rotation,
  health checks, discovery, raw-TOML editing, 15 SQLite tables
  (`decisions/0044-...md` "Reverted surfaces"), 12 test targets.
- **Blast radius:** human-days: the previous attempt cost a build-plus-revert
  cycle; repeating it costs the same again.
- **Simplest alternative:** a per-session model setting, a per-turn override,
  a health probe on demand, and startup-only configuration - i.e. what the
  legacy product does plus a typed override. Price display can be a per-run
  usage line, not a pricing engine.
- **Disposition:** re-scope before re-introducing; do not re-activate ADR 0037.
- **Effort:** M (5-10 days to write the reduced activating specification); the
  reverted shape was L+.
- **Confidence:** medium-high (the direction is explicitly "accepted ... awaiting
  a new activating specification"; nothing has replaced it).
- **Cost of delay:** the user cannot change model per session today, which is a
  daily inconvenience in the shipped product.

---

### Z5-F20 - Documentation about documentation rivals the architecture it indexes

- **Stage:** `committed` (continuous).
- **Type:** BUREAUCRACY.
- **Severity:** S4.
- **Trigger:** any change. Seven reconciliation registers total 3,617 lines,
  including a 490-line source-of-truth matrix, an evidence register with
  per-slice planned rows, a contradiction register with 89 entries, and a
  1,163-line PR-24 code-review ledger. Several carry hand-maintained copies of
  the same status (e.g. the Slice-2 revert is restated in ADR 0044, the
  deferred register, the conflict register, the roadmap, architectures 25 and
  29, and the architecture README). The shipped analogue of this pattern is the
  tag inventory the previous audit found in ">= 6 hand-maintained copies"
  (`m5plus-slice1-security-theater-audit.md` zone 6).
- **Blast radius:** human-days: every change pays N restatements, and drift is
  already visible (F12).
- **Simplest alternative:** one generated status page from the decision
  records, and registers reduced to the rows that are actually read
  (deferred/excluded, evidence anchors).
- **Disposition:** consolidate; generate the rest.
- **Effort:** M (3-5 days).
- **Confidence:** high.
- **Cost of delay:** drift compounds; the next activation copies stale text
  into a new normative specification.

---

### 3.1 Finding counts

| Type | Count | IDs |
| --- | ---: | --- |
| LANDMINE | 3 | F01, F03, F13 |
| EXCESSIVE | 7 | F02, F05, F06, F07, F11, F16, F19 |
| BUREAUCRACY | 6 | F08, F12, F14, F15, F17, F20 |
| THEATER | 2 | F09, F10 |
| DEAD | 1 | F18 |
| MIXED | 1 | F04 |

| Stage | Count | IDs |
| --- | ---: | --- |
| `shipped` (fires into `doc-only`) | 1 | F01 |
| `committed` | 8 | F02, F03, F10, F12, F13, F15, F16, F17 |
| `doc-only` | 9 | F04(part), F05, F06, F07, F08, F09, F11, F14, F18 |
| cross-cutting (`committed` + `doc-only`) | 2 | F12, F20 |

---

## 4. Keep-list (justified, with why)

| # | Item | Why it survives the simplest-sufficient-world test |
| --- | --- | --- |
| K1 | `WorkspaceRoot` as an addressing anchor, not a boundary (ADR 0047; `crates/intention-workspace/src/lib.rs:1-12`) | Cheap, honest, and it removed a lexical symlink parser that could never be correct. Keep exactly as shipped. |
| K2 | JSON-RPC 2.0 over NDJSON, typed serde JSON, one live schema, single-version policy (ADRs 0045/0046/0038) | The wire and storage are the load-bearing parts of a local agent, and this shape is the minimum that is still typed. Keep. |
| K3 | Fixed tool registry, frozen per-run tool selection, durable `ToolCall`/`ToolResult` facts and a sequential model-tool loop (architecture 15, minus the group cap and the authority prose) | This is the actual mechanism that makes an agent work; the shipped six tools already implement it. Keep; drop the 14-slot reservation ceremony and the 16-call ceiling. |
| K4 | Deterministic instruction channel: declared sources, workspace `AGENTS.md`, deployment/user/project/session fragments, fixed order, frozen per run (architecture 30) | Without it the product has no instructions; the legacy product's prompt set proves the need. Keep, with F13/F14 simplifications. |
| K5 | Plan/Build separation, physical plan artifact with hidden frontmatter, plan approval -> fresh Build run, optional handoff (architecture 07; ADR 0017/0018) | Matches legacy capability and user flows; honest about `execute` not being a sandbox. Keep. |
| K6 | VFR and Headroom as independent hooks with `expand` and `retrieve` (architecture 06) | Small, opt-in, proven in the legacy product, and they reduce token spend. Keep. |
| K7 | Session fork and regenerate with no rollback claim (architecture 23, minus the rate limit) | Regenerate is a daily action in every agent UI; the design is honest that a fork does not rewind the machine. Keep; remove the 16/hour cap (F17). |
| K8 | No-resume recovery: restart marks unfinished work interrupted, nothing retries itself, later work is a fresh run (ADR 0038, architecture 13's one good rule) | This is the correct, cheap, trustworthy behaviour for a local tool and it is already shipped for runs. Keep and promote to the ordinary path. |
| K9 | Durable-commit-before-publication for lifecycle and effect facts (architecture 04/13 transaction law) | Already shipped in M3-M5 and it is the reason a crash cannot show state that was never committed. Keep; do not extend the same ceremony to advisory projections. |
| K10 | A typed MCP capability path under the fixed `mcp` slot | The user needs MCP; the fixed slot and typed boundary are the right minimum. Keep the path, simplify the lifecycle (F11). |
| K11 | Run-scoped IPython with safe text-only output projection | The legacy product has IPython and users want it. Keep the kernel; drop epoch/grant machinery and the ten-minute cell bound (F09/F18). |
| K12 | Evidence anchors per change (roadmap quality rule) and the base 80% coverage rule (ADR 0049) | Cheap insurance that the thing still builds; the problem is the volume of per-slice evidence (F15), not the rule. Keep. |
| K13 | The limits-by-precedent doctrine and the no-content-scanning ban (ADR 0048) | The single most valuable piece of policy in the future corpus. Keep and actually apply it (F12). |

---

## 5. Predictions

Confidence is the auditor's; signals are observations that would confirm the
prediction before the cost is paid.

| Block | Prediction | Signals to watch | Confidence |
| --- | --- | --- | --- |
| Mandate aggregate + scheduler (13/16) | **Rewrite** into one continuation and quarantine rule; the 9-state lifecycle is not implemented as specified | M10's activating specification hedges on states; the first fixture list drops dispositions; a "simplified for first scope" clause appears | high |
| Child graph + verifier authority (17) | **Rewrite** to a nested-run `sub_agent`; verifier authority is **cut** | First implementation defers the verifier record family; `sub_agent` classes collapse to one | high |
| Goal domain (28) | **Cut** or reduce to todos + plan artifact | Slice 3 splits; `GoalUserDecisionState` unused in UI; gates become reference-only | medium-high |
| Programmatic-caller policy (27) | **Collapse** to a confirmation policy | First spec keeps the enum but drops policy scopes/drafts | medium |
| Bridge (19) | **Cut** to a single typed invocation path | Kernel implementation calls tools in-process; grant lifetime becomes a nuisance in tests | medium-high |
| Kernel epoch/checkpoint (20) | **Simplify**; ten-minute cell bound removed or raised | Users hit `kernel_execution_timeout` on real analysis; checkpoint serializer omissions exceed value | medium |
| Activity/notification/ack (24) | **Rewrite** as a projection over session/run events | M6 UI needs one list; urgent-dedup and ack aggregates have no rendered counterpart | medium-high |
| MCP dynamic acquisition (18) | **Simplify** to configured servers with pre-call validation | First setup story needs a persistent server list; per-run discovery shows up as latency | medium |
| Provider control plane (25/29) | **Rewrite** much smaller (per-session model, per-turn override, on-demand health) | Re-introduction spec drops catalog lifecycle tables; pricing reduced to a usage line | medium-high |
| Instruction channel (30) | **Survive** with de-ceremonialised failures | Slice 5 lands; first user hits the 16k `AGENTS.md` bound and asks for warning-not-block | medium |
| Session fork/regenerate (23) | **Survive**; rate limit cut | Users hit `fork_boundary_rate_limit` during retry storms | medium-high |
| M6 UI, M7 Plan/Build, M8 VFR/Headroom | **Survive** and become the real product | They are the only blocks with a user-visible surface; daily use starts there | high |
| M9 as the gate of M10-M12 | **Rewrite** the dependency: hardening cannot gate a track it also depends on | Roadmap edit moving `N --> I` to a per-track closure | medium |
| Entire unbuilt stack as specified | **Cut** by roughly half or more; net surviving scope is closer to M6-M9 plus two or three Mandate ideas | First activating specification after the next audit; revocation of "no retroactive contract change" | medium |
| F01 execute deadline | **Rewrite** before M10 activates, under pressure from the first integration failure | A Mandate integration test with `cargo build`; a "known timeout" status appearing in the tool taxonomy | high |

---

## 6. Metrics and commands used

Measurements (all re-runnable, read-only):

```text
# crate and code size
find crates -name '*.rs' -exec cat {} + | wc -l                  # 53,319 Rust lines
for d in crates/*/; do find "$d" -name '*.rs' | wc -l; done      # per-crate files

# documentation volume
wc -l docs/intention-relay/architecture/*.md                     # 18,000+ lines total
cat docs/intention-relay/architecture/0*.md architecture/1[0-2]*.md | wc -l   # 5,886 (v1)
cat docs/intention-relay/architecture/1[3-9]*.md architecture/2*.md \
    architecture/30*.md | wc -l                                  # 7,257 (unbuilt)
cat docs/intention-relay/reconciliation/*.md | wc -l             # 3,617

# contract and failure-code surface in the unbuilt owner documents
grep -oE '\b[A-Z][A-Za-z0-9]*Dto\b' architecture/13..30.md | sort -u | wc -l
cat architecture/1[3-9]*.md architecture/2*.md architecture/30*.md \
  | grep -oE '^[a-z][a-z0-9_]{5,}$' | sort -u | wc -l            # 137 unique codes

# limit tables and drift checks
grep -n 'code-owned limits\|Code-owned limits' architecture/1[7,9,20,23,26,28]*.md
grep -n '16 calls\|4 MiB\|512 KiB\|256 facts' architecture/15-*.md
grep -n 'calendar' architecture/11-implementation-roadmap.md       # M10 still lists it
grep -n 'EXC-011' reconciliation/deferred-excluded-register.md     # withdrawn by ADR 0048

# shipped substrate behind the Mandate track
grep -n 'EXECUTE_TIMEOUT\|let deadline\|tool_execute_external_effect_unknown' \
  crates/intention-tools/src/lib.rs                                # :146, :461, :477
grep -n 'tool_execute_external_effect_unknown' crates/intention-application/src/lib.rs  # :1095

# repository rate (cost model anchor)
git log --reverse --format='%ad' --date=short | head -1            # 2026-07-31
git log -1 --format='%ad' --date=short                             # 2026-10-02
git rev-list --count HEAD                                          # 166 commits

# UI absence
ls crates/intention-tauri/src/ crates/intention-tui/src/
find . -maxdepth 3 -name '*.svelte' -o -maxdepth 3 -name 'package.json'   # none
cat crates/intention-tauri/Cargo.toml                              # no dependencies
```

Not run (out of scope by the audit constraints): `make`, `cargo`, tests,
`gh`.

Derivations shown in the text: 137 failure codes (unique snake_case code
identifiers in architecture 13-30 code fences); ~165 distinct `*Dto` names;
11,165 lines of future normative text (7,257 + 3,908); 1.9x the v1 architecture
volume (11,165 / 5,886); ~850 net Rust lines/day (53,319 lines over 63 days);
XL = 60+ days, L = 20-60, M = 5-20, S <= 5.

---

## 7. Uncertainty and open questions

1. **Effort estimates are mine.** They are anchored on the repository's own
   observed rate and on the volume of contracts, failure codes, and numeric
   bounds, not on a work breakdown. Treat them as order-of-magnitude.
2. **The roadmap is a moving document.** Several owner documents were edited
   as late as `2026-10-02`; the drift findings (F12) may be closed by the time
   this is read, in which case the finding reduces to "the check that would
   have caught it does not exist".
3. **Not every future subsystem could be traced end to end.** Architecture 22
   (771 lines) was read at heading level plus its key sections (immutable
   selections, catalog lifecycle, reasoning); architectures 13-21, 23-30 were
   read in full. A finding inside architecture 22's reasoning-dialect catalog
   could be missed.
4. **Comparative practice for scheduled agent work** rests on vendor-adjacent
   sources rather than first-party documentation of a "cron for agents"
   feature; the Claude Code and Codex sources cited are first-party. The
   scheduling row is marked general practice.
5. **The user's own preferences are unknown to this audit.** If the user
   actually wants a durable autonomous work-authority system with independent
   verification, F05/F06/F07 change from "excessive" to "premature": the
   question is not whether such a system can be specified, but whether it
   should be specified before a single user can open the application.
6. **Money and latency are secondary here.** Provider spend and model latency
   dominate both, and this audit found no ceiling mechanism on either; whether
   that is acceptable is a product decision recorded nowhere.
7. **No load, soak, or multi-day test evidence exists** for the shipped
   substrate, so statements about how the future platform behaves over a week
   of daily use are inferences from the specifications, not measurements.


---

# Appendix F — Zone 6: shipped code economics (M0–M5)

- Repo: `/home/data/intention-relay`, branch `main` @ `3291e50` ("chore(quality): remove the quality self-test suite (#46)").
- Method: static read-only evidence pass (no builds, no tests, no sub-agents). Every claim below was produced by `rg` / file reads at the cited lines; no runtime measurement was possible.
- Output file: this file only. `pr-44-45-review-report.md`, `audit-drafts/zone-7-plan-process.md`, and all other untracked files were left untouched.
- Bar used for judgment: fitness for real daily use by one developer on their own machine, M0–M5 only; committed M6–M9 claims are marked as such but not treated as shipped value.

---

## 1. Scope and method

### 1.1 What was audited

The whole shipped surface: 24 workspace crates (28,357 `src` lines, 24,962 `tests` lines, 924 `pub` items). Per-crate sizes at audit time:

| Crate | src | tests | Role (declared) |
|---|---:|---:|---|
| intention | 2,247 | 0 | composition facade (used by daemon only) |
| intention-application | 1,629 | 2,683 | tool/command application service |
| intention-client | 794 | 1,153 | sync client + run-stream client for adapters |
| intention-config | 1,093 | 327 | config load, validation, credential presence |
| intention-daemon | 2,183 | 4,398 | process host, wire loop, binary |
| intention-domain | 2,775 | 1,219 | domain DTOs/events/facts |
| intention-hooks | 754 | 569 | hook registry and 8-phase dispatch |
| intention-model | 1,087 | 847 | provider-neutral driver contracts |
| intention-protocol | 2,768 | 1,176 | wire DTOs and method constants |
| intention-provider-generic-chat | 1,658 | 214 | OpenAI-compatible provider |
| intention-provider-openrouter | 901 | 196 | OpenRouter provider |
| intention-runtime | 1,414 | 4,454 | model-run loop, retries, tool rounds |
| intention-storage | 972 | 1,183 | durable repository contracts |
| intention-storage-sqlite | 2,720 | 2,381 | SQLite repository |
| intention-tools | 2,254 | 2,775 | 6 tools + registry + envelopes |
| intention-transport | 1,662 | 637 | sync + async local wire transports |
| intention-types | 1,114 | 278 | shared IDs/DTOs |
| intention-workspace | 78 | 117 | WorkspaceRoot |
| intention-test-support | 188 | 274 | test driver support |
| intention-headroom / plans / vfr | 5 each | 0 | skeletons (ADR 0038-protected) |
| intention-tauri | 5 | 0 | adapter skeleton |
| intention-tui | 46 | 81 | "proof client" wrapper |
| quality/harness | (removed at #46) | — | quality self-tests deleted before this audit |

Only one binary is shipped: `intention-daemon` (`crates/intention-daemon/src/main.rs`).

### 1.2 Vectors and how each was tested

1. **Zero-consumer public API** — per-crate textual reference count of every `pub fn/struct/enum/const/trait` over all crates' `src` and `tests`, then manual verification of every zero/one-hit candidate with `rg -n` and file reads (heuristic refined twice; same-file and same-crate uses counted separately).
2. **Unreachable private code / dead seams** — `rg` for `dead_code`/`unused` allows, `#[cfg(test)]`-only items, no-op shims.
3. **Never-produced DTO fields, enum variants, error codes** — per-variant production counts (constructor call sites in `src`, excluding tests), plus `git log -S` provenance for the introducing commit.
4. **Single-implementation traits / one-instantiation generics** — `rg` for `impl <Trait> for`, trait object use, generic parameter instantiation counts.
5. **Reserved slots** — registry entries and enum arms that exist but deny behavior; compared against the doc claim that justifies them.
6. **Unread config knobs** — every field of every config DTO traced to a reader (`cc`/`rg` on accessor names).
7. **Unused dependencies** — per-manifest reference scan plus manual review of feature lists.
8. **Skeleton / placeholder crates** — declared status in `quality/architecture.toml` vs references from live code.
9. **Ceremonial validation / indirection** — validation that cannot fail, hooks that cannot act, ports with no adapters, forwarder modules.
10. **Doc-vs-code mismatch (landmine vector, added during the pass)** — ADR/architecture claims about shipped limits compared to the actual implementation.

### 1.3 Five core questions (answered in full in §2.5, §4.3, §5.4, §6.3, §7)

1. What stops daily use — §2.5 (two wall-clock landmines, one missing driver).
2. Full trace step counts — §6.3.
3. Simplest sufficient world — §4.3.
4. Comparative practice — §2.5 / §5.4.
5. Future amputation signals — §5.4.

---

## 2. Vectors → verdicts

### 2.1 Vector verdict table

| Vector | Verdict | Representative evidence |
|---|---|---|
| Zero-consumer public API | **Mixed**: ~104 items with no out-of-file consumer; a hard core is genuinely dead (pagination, plan vocabulary, envelope, sync transport), a long tail is same-file internal API | §3 F03, F04, F05, F08, F09 |
| Unreachable private code / dead seams | **Mostly clean**: exactly two `#[expect(dead_code)]` seams, both cited below; `FaultPoint` is `cfg(test)`-only but has a production no-op shim | §3 F11, F13 |
| Never-produced DTO fields/variants | **Confirmed on a small set**: 4 domain DTOs with zero references anywhere outside `intention-domain`, 2 page DTOs, `ErrorDetailDto`/`detail` (deliberate reserve), `DaemonReadinessDto` non-Ready states | §3 F04, F06, F08, F09, F10 |
| Error codes | **217 distinct codes; 175 have ≤1 non-constructor reference** — but almost all are real failure vocabulary used by the paths that generate them; no evidence of invented codes | §6.1 |
| Single-implementation traits | **Two are dead** (`ToolExecutor`, `LocalToolInvocationPort`); the rest are one-impl-but-real boundaries (provider `ModelDriver`, repo traits, `SafeObserverHook`) | §3 F11, F13 |
| One-instantiation generics | **Not a problem found**: generics are on `Result`-style helpers and test clocks; no over-parameterized API was found | — |
| Reserved slots | **Four reserves confirmed**: 8 of 14 tool registry entries, plan/config-revision taxonomy, `ErrorDetailDto`, `DaemonReadinessDto` non-Ready states; 2 are doc-justified (tools, per architecture 05), 2 are not (readiness, details) | §3 F06, F07, F08, F10 |
| Unread config knobs | **None found**: every field of `ProviderExecutionPolicyDto`, `DaemonSocketDto`, config load policy is read; the one dead table is a quality-policy table, not a runtime knob | §3 F12 |
| Unused dependencies | **None proven**: a textual reference scan found no manifest entry with zero references; the real finding is `tokio` `test-util` in production features | §3 F15 |
| Skeleton / placeholder crates | **4 skeletons, ~21 lines total, explicit status, ADR 0038-protected** — cheap and honest | §4 |
| Ceremonial validation / indirection | **Three real cases**: readiness never leaves `Ready`; `PrivateModelRunDispatch` is a production no-op that discards a prebuilt DTO; pure-forwarder tool modules + envelope layer | §3 F03, F06, F14 |
| Doc-vs-code mismatch | **Two landmines**: `EXECUTE_TIMEOUT` and provider round timeout are documented as progress-bounded but implemented as fixed wall-clock deadlines | §3 F01, F02 |
| Slice-1/2 control-plane remnants | **Clean**: greps for capability/negotiation/canonical-codec surface produced only legitimate matches; ADR 0044–0048 removals were effective | §6.1 |

### 2.2 What "shipped" means per item

Lifecycle stаges used below: `shipped` (M0–M5 durable path, reachable by the daemon binary), `committed` (declared by roadmap/ADR for M6–M9, no production consumer today), `doc-only` (only documentation references). Each card states its stаge explicitly.

### 2.3 Zero-consumer API: the two-tier result

- Tier A (dead in production, zero consumers outside the defining crate even in tests): `PageRequestDto`/`PageCursorDto`; `DomainEventDto::ToolResultRecorded` + `ToolResultRecordedEventDto` + `ToolResultStatusDto` + `ToolResultMetadataEntryDto`; `CreatePlanCommandDto`; `PlanStatusChangedEventDto`; `ConfigurationRevisionAcceptedEventDto`; `invoke_enveloped`/`invoke_enveloped_with_cancellation`/`ToolResultEnvelope`/`ToolInvocation`/`ToolContext`/`ToolObservability`/`ToolExecutionMetadata`; `LocalToolInvocationPort`; `ToolExecutor`. Proof counts: `rg -n "ToolResultStatusDto|ToolResultMetadataEntryDto" crates --glob '!crates/intention-domain/**' | wc -l` → 0; `rg ... "PageRequestDto|PageCursorDto" --glob '!crates/intention-types/**' | wc -l` → 0 (all 15 hits are inside `intention-types` itself); `ToolObservability` hits outside `intention-tools` → 0 once the correct glob is used.
- Tier B (used only inside the defining file, so cohesion API rather than dead code): `map_finish_reason` internals, protocol `next_after_sequence` (fully dead even in tests — §3 F17), `RunStreamSubscription` accessors, `AsyncRequestReceiver` type name (tests only), `DispatchResult` (fields read, type never named outside hooks).

### 2.4 Ceremonial validation found (and not found)

Not found: no validation that always passes was discovered in config load, workspace resolution, or storage writes — the TOML load is fail-closed, path containment is enforced (ADR 0047), and DTO constructors reject invalid shapes with real tests. The two cases that look ceremonial are the readiness projection (always `Ready`) and hook dispatch (wired at all 8 phases, exactly one no-op implementation). Both are covered below.

### 2.5 Core-question answers

**Q1 — What obstructs daily use first.**
1. There is no daily driver yet: the only shipped artifact is the daemon process; the TUI is a 46-line proof wrapper and Tauri is a 5-line stub. This is a roadmap fact, not a defect, but it is the first obstacle.
2. `execute` kills any command at 30 seconds of wall clock (§3 F01) — `cargo test`, `npm install`, or a CI-style build simply cannot complete. The model then receives `tool_execute_external_effect_unknown` and cannot distinguish "killed on purpose" from "unknown outcome".
3. A single model round is also bounded by a fixed wall clock, 30 s by default, not reset by progress (§3 F02); a slow provider or a long reasoning response fails the run after one retry.

**Q2 — Full trace cost** is ~8 crate boundaries, 8 hook phases, and 2 durable appends per tool round; see §6.3 for the step-by-step count.

**Q3 — Simplest sufficient world** is sketched in §4.3.

**Q4 — Comparative practice.** General practice among local-first coding agents is a single installed binary that is the daily driver (CLI/TUI), one config file, one transport, and lazy addition of libraries only when a consumer exists. Committed-but-unconsumed stacks of this size (a 794-line client library, a second transport, a TUI proof) are unusual before any consumer milestone; the common pattern is to build the consumer and grow the library inside it. Verification: general practice/industry norm, no source accessible in this environment.

**Q5 — Amputation signals** are listed in §5.4.

---

## 3. Findings

Ranking: S1 blocks daily use; S2 degrades daily use; S3 real cost, no user-visible break; S4 small cost/cosmetic. Each card lists the full rubric requested: stage / type / trigger / mechanism / blast radius / simplest alternative / disposition / effort / confidence / cost of delay.

---

### Z6-F01 — `execute` kills every command at 30 s wall clock and reports an "unknown external effect"

- **Stage:** shipped (tools are reachable from the daemon tool loop).
- **Type:** LANDMINE + doc/code mismatch; excessive limit, not dead code.
- **Severity:** S1.
- **Trigger:** any `execute` command that runs longer than 30 seconds (`cargo test`, `npm install`, large builds, long test suites).
- **Mechanism:** `const EXECUTE_TIMEOUT: Duration = Duration::from_secs(30)` (`crates/intention-tools/src/lib.rs:146`); `bounded_output` hard-codes it into `bounded_output_with_timeout(child, cancellation, EXECUTE_TIMEOUT)` (`:389-393`); the loop polls `try_wait` against a deadline fixed at spawn, then kills the process group and returns `Err("tool_execute_external_effect_unknown")` (`:430-477`, also `:490`, `:508`, `:524`). ADR 0048 lists `Process timeout window (EXECUTE_TIMEOUT, 30 s)` as bounding "a child process that **stops producing progress** or never exits" (`docs/intention-relay/decisions/0048-limits-by-precedent-and-no-content-scanning.md:58`). The implementation has no progress signal: wall clock alone decides.
- **Blast radius:** daily work cannot run long commands at all; the failure is classified as an unknown external effect, which is the most expensive category for the run loop and for a human reading the transcript — it implies unknown side effects where a label such as "timeout" would be accurate. Every subsequent design that relies on the agent verifying its own builds inherits this.
- **Simplest alternative:** make the execute window configurable next to the provider execution policy (same config surface, e.g. `execute_timeout_seconds`, default on the order of minutes), and classify a wall-clock expiry as a distinct documented timeout failure rather than `external_effect_unknown`. Progress-based semantics can come later; not required.
- **Disposition:** change before daily use (fix); if kept, the ADR text must be corrected to say "wall-clock", never "progress".
- **Effort:** S (one constant → config field, one error-category decision, tests).
- **Confidence:** high (mechanism fully read; no runtime measurement).
- **Cost of delay:** the first real user hits this on their first non-trivial command; the fix afterwards must also repair whatever side effects the kill left behind.

---

### Z6-F02 — provider round deadline is a fixed 30 s wall clock, not progress-bounded as documented

- **Stage:** shipped (default config).
- **Type:** LANDMINE + doc/code mismatch.
- **Severity:** S2 (S1 with slow/reasoning providers; ceiling is 60 s even when configured).
- **Trigger:** single provider round whose total wall time exceeds `attempt_timeout_seconds` — common for reasoning models, long code generations, or queued provider capacity — even while the stream is actively producing deltas.
- **Mechanism:** `drive_provider_round` creates one `self.time.sleep(...)` future **before** the event loop (`crates/intention-runtime/src/lib.rs:819-823`) and races it against every `stream.next()` (`:836-859`); it is never re-armed after an event arrives. Expiry returns `RunFailureDto::new("provider_attempt_timed_out", ErrorRetryDto::Delayed, None)` (`:847-857`), which the attempt loop retries at most once (`max_attempts` 1..=2, default 2; `crates/intention-config/src/lib.rs:613-647`), then the run fails. ADR 0048:60 documents this family as "bound a live provider request that stops producing progress". (The client has a third 30 s constant of the same habit — `STREAM_REPLY_TIMEOUT`, `crates/intention-client/src/lib.rs:33` — but that one waits for a bounded reply and is not a landmine.)
- **Blast radius:** an entire model run fails, twice-billed tokens, and the same prompt will fail again deterministically if the provider is consistently slower than the deadline. It also makes the M5+ "long autonomous run" story unusable with slow models.
- **Simplest alternative:** re-arm the timeout future on each received event (true progress timeout), and raise/allow the ceiling beyond 60 s for reasoning models. One `reset()` per event is a few lines.
- **Disposition:** fix before daily use; at minimum correct the ADR wording.
- **Effort:** S.
- **Confidence:** high on mechanism; medium on how often a round exceeds 30 s in practice (not measured).
- **Cost of delay:** silent run failures look like provider instability; users will blame the provider.

---

### Z6-F03 — tool-result envelope layer has zero consumers and contradicts the architecture text

- **Stage:** shipped (declared as the execution surface), actually unused.
- **Type:** dead/EXCESSIVE surface.
- **Severity:** S3.
- **Trigger:** none today; risk taken by the next implementer who reads architecture 05 and wires the daemon to the wrong (bare) path, or who layers a second result model next to the envelope.
- **Mechanism:** `invoke_enveloped` / `invoke_enveloped_with_cancellation` (`crates/intention-tools/src/lib.rs:1675-1718`) and `ToolResultEnvelope` (`:1443-1450`), `ToolObservability` (`:302-307`), `ToolInvocation`/`ToolContext` (`:309-313`, `:1308-1367`), `ToolExecutionMetadata` have no production caller. Production goes `intention-application/src/lib.rs:627` → `ToolService::dispatch_with_cancellation` (`intention-tools/src/lib.rs:1631`) → bare `ToolResult`, and the daemon converts the bare `ToolResult::projection()` to the wire (`intention-daemon/src/lib.rs:941-968`, `normalize_tool_result`). Architecture 05 says "The current M5 execution surface is the cancellation-aware bare-result dispatch and the envelope entry ... that future tool loops may consume" (`docs/intention-relay/architecture/05-tools-workspace-and-hooks.md:156-157`) and later §"Tooling execution API" describes the envelope as present for implementers. All envelope tests live in `intention-tools/tests/tool_contracts.rs` (e.g. `:783`, `:829`, `:844`, `:1855`).
- **Blast radius:** roughly the envelope family is ~250 production lines plus several hundred test lines that assert a contract no production path exercises; more importantly two result models exist simultaneously, and `ToolResult`/`ToolResultEnvelope`/`ToolResultProjection` naming invites wrong-path wiring.
- **Simplest alternative:** delete the envelope entry points and keep bare projection (smallest working world), **or** wire `invoke_enveloped` as the single application entry and delete the bare call — either one, not both.
- **Disposition:** decide at the next tool-touching change; deletion preferred unless a named consumer (hook composition, cancellation metadata) exists.
- **Effort:** M (2–3 days incl. tests) to delete or wire.
- **Confidence:** high (call-site counts).
- **Cost of delay:** grows with each new tool added against the wrong model.

---

### Z6-F04 — domain `ToolResultRecorded` event family is a parallel vocabulary with zero producers

- **Stage:** shipped source; no producer, no consumer (M5 durable path uses a different shape).
- **Type:** dead DTO family.
- **Severity:** S3.
- **Trigger:** none today; a future feature (UI timeline, plan replay) may bind to the wrong tool-result vocabulary.
- **Mechanism:** `DomainEventDto::ToolResultRecorded` (`crates/intention-domain/src/lib.rs:753`) with `ToolResultRecordedEventDto` (`:1030-1173`, ~234 prod lines), `ToolResultStatusDto` (`:944-968`), `ToolResultMetadataEntryDto` (`:971-1023`), plus a 49-pair status test block and `tests/m5_tool_results.rs` (~253 lines). Zero references outside `intention-domain` (`rg` count 0). The durable M5 path is `ModelRunFactInputDto::tool_result_recorded` + `ToolLifecycle` + `ToolResultEvidenceDto` (`crates/intention-domain/src/model_facts.rs`; storage mapping at `crates/intention-storage-sqlite/src/lib.rs:1685`, `:1721-1722`). Provenance: introduced in M5 commit `02b0f13`.
- **Blast radius:** two tool-result models (domain event vs model fact) that will both claim the "one true" representation when M7/plan or a UI consumes tool history; ~234 prod + ~300 test lines carrying the loser.
- **Simplest alternative:** delete the unused `DomainEventDto::ToolResultRecorded` family and keep the fact path; if a UI event is needed later, project facts into a fresh event.
- **Disposition:** delete (M5 path is demonstrably the live one).
- **Effort:** S–M.
- **Confidence:** high.
- **Cost of delay:** the ambiguity cost compounds each milestone that touches tool results.

---

### Z6-F05 — adapter stack (`intention-client`, run-stream, sync transport, TUI proof) has no production consumer

- **Stage:** shipped code; committed for M6 (Tauri), the only consumer claim.
- **Type:** EXCESSIVE / committed-ahead-of-consumer.
- **Severity:** S3.
- **Trigger:** M6 Tauri work starts; the first adapter that calls `IntentionClient` or the sync transport decides whether the stack is adopted, rewritten, or deleted.
- **Mechanism:** `IntentionClient` (sync) (`crates/intention-client/src/lib.rs:94-274`), `RunStreamClient` (`:280-333`), `RunStreamSubscription` (`:336-451`), `RunSubscriptionReducer` (`:454-675`), `StartupLock`/`ProcessDaemonLauncher`; the whole crate (794 src + 1,153 test lines) is referenced only by its own tests, `intention-daemon` dev-deps, and `intention-tui`'s 46-line wrapper. The sync transport (`LocalConnection`/`LocalListener`/`negotiate_*`, `crates/intention-transport/src/lib.rs:161-500, 506-562`) is reachable only through the client and test fixtures; the daemon binds only the async listener (`crates/intention-daemon/src/lib.rs:994`) and its sync `serve_connection` is driven only by `serve_test_connection` (`:1259-1261`) for fixtures. Roadmap M6 declares the Tauri shell as the first consumer (`docs/intention-relay/architecture/11-implementation-roadmap.md:688+`).
- **Blast radius:** coverage/maintenance cost today (~1,950 src lines, ~2,100 test lines), and rework risk: a Tauri shell is an async/native app; a sync client with `StartupLock` and a reducer may or may not survive contact with it. The sync wire tests also pin a client transport the daemon already treats as secondary.
- **Simplest alternative:** freeze (no further investment) and let M6 either adopt it as-is or delete it in favor of the async path; do not grow it speculatively before M6 starts.
- **Disposition:** keep with a decision gate at M6 start; delete the sync path if Tauri drives the async client directly.
- **Effort:** S to gate; M to rewrite/delete.
- **Confidence:** high on unused; medium on how much Tauri will reuse.
- **Cost of delay:** rework risk grows as more tests encode the sync contract.

---

### Z6-F06 — readiness lifecycle exists but the daemon always reports `Ready`

- **Stage:** shipped.
- **Type:** ceremonial state machine / dead branches.
- **Severity:** S3 (user-visible only as startup UX).
- **Trigger:** daemon start while recovery is still running, or shutdown with in-flight work — neither state is ever exposed.
- **Mechanism:** `DaemonApplicationFacade::health()` is `pub const fn` returning `DaemonHealthDto::new(SCHEMA_VERSION, PROTOCOL_VERSION, DaemonReadinessDto::Ready)` (`crates/intention/src/lib.rs:783-785`). `Starting`, `Unavailable`, and `Draining` are never produced by the daemon; the client nevertheless branches on them (`crates/intention-client/src/lib.rs:205-234`) and synthesizes its own "starting" for connection refusal, with a 3-second retry window. Because the daemon binds only after `recover_before_ready()` (`crates/intention/src/lib.rs:772`), the readiness projection carries no information.
- **Blast radius:** the startup story depends on connection errors and retry timing instead of a protocol-visible state; the client's non-Ready branches (~35 lines + tests) can never be exercised by a real daemon, so any bug in them ships unnoticed. Draining is also the natural mechanism for "stop accepting new runs" that the product will need.
- **Simplest alternative:** either produce the real states (bind early, report `Starting`, report `Draining` during shutdown) or collapse the enum to the single value the daemon can actually report.
- **Disposition:** collapse now or wire `Draining`/`Starting` with the process-lifecycle work in M6; do not leave as decoration.
- **Effort:** S to collapse; M to wire honestly.
- **Confidence:** high (const fn body read).
- **Cost of delay:** every adapter written before this decision inherits fake-ready assumptions.

---

### Z6-F07 — 8 of 14 tool-registry entries are reserved names that only add a `"reserved"` literal

- **Stage:** shipped registry; committed for M5+/M6.
- **Type:** reserved slots.
- **Severity:** S4.
- **Trigger:** A reader counts registry entries as live capabilities, or a future M5+/M6 slice claims one of the reserved names.
- **Mechanism:** the registry declares 14 descriptors; 6 are implemented (`read`, `write`, `edit`, `execute`, `search`, `list`-class) and 8 are `Reserved` (`crates/intention-tools/src/lib.rs:1041-1240`). Reserved ids are excluded from `model_visible_descriptors` and rejected in the daemon's `parse_tool_input` arm as `unknown_tool` (`crates/intention-daemon/src/lib.rs:913-920`); the daemon's own test asserts the rejection (`:2125-2137`). Architecture 05 justifies the reserved names as pre-reviewed vocabulary (`docs/intention-relay/architecture/05-tools-workspace-and-hooks.md:22-23`), and the prior project audits already accepted them.
- **Blast radius:** ~40 lines plus tests that assert a `reserved` string. No behavior differs from an unknown tool.
- **Simplest alternative:** keep only the six live descriptors and re-add reserved names when their slice starts; or keep as-is (cheapest to argue).
- **Disposition:** keep (documented, tiny), delete opportunistically when the registry is touched.
- **Effort:** XS.
- **Confidence:** high.
- **Cost of delay:** negligible.

---

### Z6-F08 — plan/config-revision domain taxonomy is declared but never produced

- **Stage:** shipped source; committed for M7 (Plan/Build).
- **Type:** reserved DTO family.
- **Severity:** S4.
- **Trigger:** M7 planning begins; the shapes are either adopted as the starting contract or rewritten.
- **Mechanism:** `PlanStatusDto` (`crates/intention-domain/src/lib.rs:116-131`), `CreatePlanCommandDto` (`:682-708`; sole reference is a domain test at `:1636`), `ConfigurationRevisionAcceptedEventDto` (`:1515-1535`), `PlanStatusChangedEventDto` (`:1539-1562`), transition validator (`:828+`). Architecture 04 lists `ConfigurationRevisionAccepted`/`PlanStatusChanged` as vocabulary (`docs/intention-relay/architecture/04-sessions-runs-events-and-storage.md:183`) and architecture 07 quotes `CreatePlanCommandDto` semantics (`07-plan-and-build-modes.md:77`), i.e. the shapes were specified before M7 was drafted.
- **Blast radius:** ~120 production lines + tests; risk that the M7 implementation (which will be designed against a real UI/flow) rewrites the shapes anyway, making the reserve wasted.
- **Simplest alternative:** delete until M7's spec names actual producers, or keep and treat as M7's starting draft.
- **Disposition:** defer decision to M7 planning; no action now.
- **Effort:** S when M7 lands.
- **Confidence:** high on unused, medium on whether M7 reuses.
- **Cost of delay:** low.

---

### Z6-F09 — pagination DTOs have been dead since M1

- **Stage:** shipped source (M1 era), zero consumers at M5.
- **Type:** dead DTOs.
- **Severity:** S4.
- **Trigger:** A new query needs paging and either reuses or ignores these types.
- **Mechanism:** `PageCursorDto` (`crates/intention-types/src/lib.rs:208-244`) and `PageRequestDto` (`:246-281`); `rg` outside `intention-types` returns 0 hits in `src` and only the crate's own tests use them. The paginated protocol surface that motivated them was retired (ADR 0044 era); no M5 query returns a page.
- **Blast radius:** ~75 lines + tests.
- **Simplest alternative:** delete; re-add a page DTO with the first paged query.
- **Disposition:** delete.
- **Effort:** XS.
- **Confidence:** high.
- **Cost of delay:** negligible.

---

### Z6-F10 — `ErrorDetailDto`/`ErrorDto::detail` is reserved vocabulary with a self-documented absence

- **Stage:** shipped source; documented reserve.
- **Type:** reserved wire field.
- **Severity:** S4.
- **Trigger:** The first file-oriented `not_found` producer ships (M5+), or the reserve is abandoned.
- **Mechanism:** the variant doc itself states: "Reserved vocabulary: no production path produces this variant today; the file-oriented tools answer `tool_read_failed` instead. It is kept as the reviewed not-found detail shape rather than deleted." (`crates/intention-types/src/lib.rs:431-435`). `ErrorDto.detail`, `with_detail`, and `detail()` (`:452`, `:513-527`, `:617-618`) have zero non-test users outside `intention-types` (`rg` count 0). Architecture 05 conditions the shape on M5 implementing a file-oriented `not_found` outcome (`05-tools-workspace-and-hooks.md:124`), which M5 did not.
- **Blast radius:** a serde field and constructor plumbing, ~50 lines plus tests; no behavioral cost.
- **Simplest alternative:** keep (explicitly reviewed reserve) or delete until a producer ships.
- **Disposition:** keep, with a note that the ADR/architecture wording should be updated to M5+ if it survives this audit.
- **Effort:** XS either way.
- **Confidence:** high.
- **Cost of delay:** negligible.

---

### Z6-F11 — `LocalToolInvocationPort` trait has zero implementations and zero callers

- **Stage:** shipped source.
- **Type:** dead trait.
- **Severity:** S3.
- **Trigger:** Any implementer looking for the sanctioned local tool seam finds this trait first.
- **Mechanism:** `pub trait LocalToolInvocationPort` (`crates/intention-application/src/lib.rs:119-125`), 15 lines plus `ToolInvocationOutcome`/`ToolInvocationRecord` types; no `impl`, no generic use, no caller anywhere (`rg`).
- **Blast radius:** small line cost, but it reads as the sanctioned seam for local tool invocation while the real path is `ToolService::dispatch_with_cancellation`; an implementer can waste a day adopting it.
- **Simplest alternative:** delete.
- **Disposition:** delete.
- **Effort:** XS.
- **Confidence:** high.
- **Cost of delay:** low but nonzero (wrong-seam risk).

---

### Z6-F12 — `quality/architecture.toml` `[future_dependencies]` table is read by no checker and is stale

- **Stage:** shipped policy.
- **Type:** dead policy / unreachable config.
- **Severity:** S3 for the policy class (silent non-enforcement).
- **Trigger:** A contributor adds a dependency rule to `[future_dependencies]` expecting enforcement.
- **Mechanism:** `Cargo.toml` (`quality/architecture.toml:91-92`) declares `intention-runtime = ["intention-model"]` under `[future_dependencies]`; `quality/check_architecture.py` reads `future_crates` but never `future_dependencies`; `rg -n "future_dependencies"` matches only the TOML line itself. The entry is also stale — runtime and model are already mutually current dependencies.
- **Blast radius:** the policy file advertises an enforcement that does not exist; future additions to this table would be inert while looking authoritative.
- **Simplest alternative:** delete the table (or wire it into `check_architecture.py` with a real check).
- **Disposition:** delete (dead) or implement; deletion is smaller.
- **Effort:** XS.
- **Confidence:** high (proved absent reader).
- **Cost of delay:** policy drift: contributors trust a file that does nothing.

---

### Z6-F13 — tool submodules are pure forwarders and `ToolExecutor` is a declared-dead trait

- **Stage:** shipped source.
- **Type:** EXCESSIVE indirection / dead seam.
- **Severity:** S4.
- **Trigger:** The next tool or executor change touches these modules and the suppressed trait.
- **Mechanism:** `crates/intention-tools/src/file.rs`, `search.rs`, `execute.rs` are 11/13/10-line modules that only re-export `super::*` behavior (no local logic); `trait ToolExecutor` carries `#[expect(dead_code, reason = "The private executor boundary is activated by the composition slice.")]` (`crates/intention-tools/src/lib.rs:2214-2220`). The composition slice it references shipped without activating it.
- **Blast radius:** ~35 lines + one suppression that hides the absence of an executor boundary; the trait's existence suggests extensibility that no code uses.
- **Simplest alternative:** fold forwarders into `lib.rs` (or keep one module with real content) and delete the trait or implement it; the production path already has a working dispatch.
- **Disposition:** delete trait; fold modules on next touch.
- **Effort:** XS–S.
- **Confidence:** high.
- **Cost of delay:** negligible.

---

### Z6-F14 — `PrivateModelRunDispatch` is a production no-op that discards the DTO it was built for

- **Stage:** shipped.
- **Type:** ceremonial indirection + duplicated work.
- **Severity:** S4.
- **Trigger:** Run-start performance or behavior debugging reaches the schedule path.
- **Mechanism:** `PrivateModelRunDispatch` (`crates/intention/src/lib.rs:160-198`) is a port whose production implementation is `#[cfg(not(test))]`-free but behaviorally a no-op: it accepts a prebuilt `ScheduleModelRunDto` and drops it; the daemon then re-reads durable context and schedules the run itself (`crates/intention-daemon/src/lib.rs:1171-1181`). The facade also rebuilds the schedule per command.
- **Blast radius:** one extra durable read plus DTO construction per run start, and a port that lies about being the dispatch mechanism; tests inject a real dispatcher while production does nothing, so behavior differs by build configuration.
- **Simplest alternative:** delete the port and let the daemon schedule directly (as it already does), or make the port the single scheduler.
- **Disposition:** delete at next touch of the run-start path.
- **Effort:** S.
- **Confidence:** high.
- **Cost of delay:** low.

---

### Z6-F15 — duplicated bound constants across crates can drift

- **Stage:** shipped.
- **Type:** EXCESSIVE duplication.
- **Severity:** S4.
- **Trigger:** One copy of a duplicated bound is changed without the other.
- **Mechanism:** `MAX_ASSISTANT_CONTENT_BYTES = 4096` exists in both `crates/intention-runtime/src/lib.rs:178` and `crates/intention-domain/src/model_facts.rs:11`; `MAX_TAIL_FACTS = 256` in both `crates/intention-storage-sqlite/src/lib.rs:34` and `model_facts.rs:12`; `512 * 1024` canonical size limits in three crates. Each copy is individually justified by ADR 0048, but two sources of truth for one number is a drift hazard.
- **Blast radius:** a future bound change applied to one copy yields conflicting validation between producer and storage.
- **Simplest alternative:** one exported constant per bound, referenced everywhere.
- **Disposition:** consolidate on next bound change.
- **Effort:** XS.
- **Confidence:** high.
- **Cost of delay:** low.

---

### Z6-F16 — production features and API carry test-support weight (provider fixture helpers, `tokio test-util`, `outbound_calls_for_test`)

- **Stage:** shipped.
- **Type:** EXCESSIVE test leakage into production surface.
- **Severity:** S4.
- **Trigger:** Provider API review, compile-time budget review, or a supply-chain/feature audit.
- **Mechanism:** 11 `pub fn map_fixture_*` helpers on the two providers (`crates/intention-provider-openrouter/src/lib.rs:111-171`, `crates/intention-provider-generic-chat/src/lib.rs:120-165`) exist for integration tests yet are always-compiled public API; `outbound_calls_for_test` + `prepared_request_count` are production fields/accessors kept for tests (`openrouter:32,96-104`; generic-chat:39,105-112); `intention-daemon` production `[dependencies]` tokio enables `test-util` (`crates/intention-daemon/Cargo.toml:25`) — the dev-only entry exists for transport (`intention-transport/Cargo.toml:22`) but daemon's is unconditional. Daemon test seams are properly gated by `#[cfg(any(test, feature = "test-support"))]` and off by default, so they are not in this complaint.
- **Blast radius:** provider public API larger than its real contract; `test-util` compiled into the shipped daemon dependency graph; slight compile-time and surface cost only.
- **Simplest alternative:** move fixture helpers behind a `test-support` feature (pattern already exists) and move daemon's tokio `test-util` to dev-dependencies.
- **Disposition:** tidy opportunistically.
- **Effort:** XS.
- **Confidence:** high.
- **Cost of delay:** negligible.

---

### Z6-F17 — `SessionEventTailPageDto::next_after_sequence` is dead even in tests

- **Stage:** shipped.
- **Type:** dead accessor (smallest possible instance of the vector).
- **Severity:** S4.
- **Trigger:** Any protocol-touch change.
- **Mechanism:** `crates/intention-protocol/src/lib.rs:1077-1081`; `rg -n "next_after_sequence"` over `crates/` returns only this definition. The sync subscribe path tracks the last event itself.
- **Blast radius:** 5 lines.
- **Simplest alternative:** delete.
- **Disposition:** delete with any protocol touch.
- **Effort:** XS.
- **Confidence:** high.
- **Cost of delay:** none.

---

## 4. Keep-list

### 4.1 Mechanisms worth their cost (verified live)

- **Hello handshake + exact version gate** (`intention-transport`), and the daemon's `ProtocolAccepted` reply; single live wire version, honest rejection of mismatches.
- **Fail-closed TOML config load**, `provider_material` presence checks, and structural credential absence (`StartupProviderMaterial` never exposes the key; `safe_debug_projection` is a redaction projection consumed at `crates/intention/src/lib.rs:1474`).
- **WorkspaceRoot** single-resolution boundary (ADR 0047) with test coverage; it is 78 lines and carries the entire path security story.
- **SQLite single schema, created on open** (`SCHEMA_SQL`, 14 tables; every table is read or written by production code); repository traits (`SessionRepository`, `ModelRunRepository`, `ToolLifecycleRepository`) each have one real implementation but are consumed as `&dyn` boundaries in the runtime/application, so the trait is doing runtime work, not decoration.
- **The 6 live tools** and the `ToolService` dispatch/cancellation path; `ToolResult::projection` consumption in the daemon is the one result path in use.
- **The 8-phase hook pipeline**: wired at every phase with real contexts (`intention-application/src/lib.rs:423,484,532,582,635,681,770`), one production hook (`SafeObserverHook`, `crates/intention/src/lib.rs:209-237`). The single no-op hook makes the current value zero, but the phases are cheap and the seam is genuinely exercised.
- **The durable model-fact family** (`ModelRunFact*`, `ToolLifecycle`, `ToolResultEvidenceDto`): the live tool-result representation (1 `tool_call_recorded` + 1 `tool_result_recorded` append per round).
- **ADR 0048 caps with demonstrated precedent** (body/context limits, 512 KiB canonical result, tail bounds, PR24-022 file caps), with the two timeout exceptions in F01/F02.
- **Reserved tool slots** (F07): doc-justified, tiny, already accepted by prior audits.
- **Skeleton crates** `intention-headroom`, `intention-plans`, `intention-vfr` (5 lines each) and adapter stubs `intention-tauri` (5 lines), `intention-tui` (46 lines): explicitly declared in `quality/architecture.toml`, protected by ADR 0038; near-zero cost and honest about being placeholders.
- **Daemon test seams** (`serve_test_connection`, `TestHostLifecycle`, first-append gates): properly feature-gated and required for M5 evidence; the only caveat is co-location with production source (F16 note).
- **`ErrorDetailDto` reserve** (F10): explicitly reviewed and self-documented; keep or delete, but it is not accidental.

### 4.2 Clean results worth stating

- 217 error codes, 175 with ≤1 non-constructor reference: near-all are the failure vocabulary of the paths that emit them, with retry semantics; no mass-invented codes were found.
- 20 `DomainEventDto` variants: all but `ToolResultRecorded` are produced (declaration-only variants counted and named).
- Every runtime config field is read; every DB table is used; every manifest dependency has at least one textual reference.
- No remaining Slice-1/Slice-2 control-plane remnants (ADR 0044–0048 removals verified by grep).

### 4.3 Simplest sufficient world (what a single-user daily build actually needs)

Keep: daemon binary + async transport + `intention` facade; SQLite storage; 6 tools; model-fact durable chain; runtime retry loop with progress-based timeouts; hooks collapsed to zero or one phase if nothing registers; readiness collapsed to a single truthful state; reserved tool slots. Cut or defer until a named consumer exists: tool-result envelope (F03), domain `ToolResultRecorded` family (F04), plan/config-revision vocabulary (F08), pagination DTOs (F09), sync transport + client library until M6 adopts them (F05), `LocalToolInvocationPort` (F11), `PrivateModelRunDispatch` (F14), `[future_dependencies]` (F12), `next_after_sequence` (F17). Fix before first daily use: F01 and F02.

---

## 5. Predictions

### 5.1 Predictions with signals (stage: committed, per roadmap)

| Prediction | Signal to watch | Confidence |
|---|---|---|
| `execute` window becomes configurable or minute-scale, and the error is reclassified | First daily-driver build/test command that exceeds 30 s; M6 dogfooding | high |
| Provider round timeout becomes progress-based (re-armed per event) or its ceiling rises | First slow/reasoning-model run failure in real use | high |
| `intention-client` sync stack is either adopted wholesale by M6 Tauri or rewritten against the async client | M6 branch layout, whether `StartupLock` survives | medium |
| Sync transport (`LocalConnection`/negotiate) is deleted once fixtures move to async | Removal of `serve_test_connection` users | medium |
| Domain `ToolResultRecorded` event family is deleted, not used, by the first UI timeline | Any diff adding a producer to it | medium-high (fact path is live) |
| Envelope family is wired by a real tool-loop boundary or deleted | Diffs touching `invoke_enveloped` | medium |
| Plan/config-revision DTOs are rewritten at M7 rather than consumed as-is | M7 spec draft | medium |
| Readiness enum is collapsed or `Draining`/`Starting` get real producers | Daemon lifecycle work | medium |
| Reserved tool slots are either implemented by their claim or deleted at M6 | Registry diffs | medium |
| Pagination DTOs are deleted unless a paged query ships by M7 | `PageRequestDto` diffs | medium-high |

### 5.2 Amputation checklist (fires when the signal fires)

Delete-without-replacement is safe in this repo because AGENTS.md forbids backward compatibility and there are no external consumers. The conditions above are the only prerequisites; none requires a migration.

### 5.3 What must survive any amputation

The M5 durable chain (facts + lifecycle + evidence), the wire version gate, workspace resolution, and config redaction must not be touched by dead-code cleanup.

### 5.4 Core-question answers 4–5 (comparative practice and amputation signals)

Comparative practice (general industry pattern, stated as such): daily-driver-first local agents ship one binary and grow library surfaces with the driver; this repo built the adapter layer before the driver, which is the main economic distortion, and it is already documented as a committed M6 claim rather than an accident. Amputation signals: the moment a consumer milestone starts, every unconsumed stack listed in §4.3 either gets its named consumer in that milestone's spec or is deleted in the same change series; if a milestone closes without naming the consumer, the surface is dead by the project's own bar ("no producer, no consumer, no M6–M9 claim").

---

## 6. Metrics, commands, and trace counts

### 6.1 Counts

- 24 workspace crates; 28,357 `src` lines; 24,962 `tests` lines; 924 `pub` items (`rg -n '^\s*pub (fn|struct|enum|trait|const|type|mod)' crates/*/src | wc -l`).
- One shipped binary (`intention-daemon`); 18 active production crates + 3 skeleton crates + 2 adapter stubs (+ test-support) per `quality/architecture.toml`.
- 217 distinct error-code string literals; 175 with ≤1 reference outside constructor sites.
- 20 `DomainEventDto` variants; 1 unproduced (`ToolResultRecorded`), 2 declaration-only (`ConfigurationRevisionAccepted`, `PlanStatusChanged`), 17 produced.
- Tool registry: 14 descriptors, 6 live, 8 reserved; reserved ids rejected as `unknown_tool` in `crates/intention-daemon/src/lib.rs:913-920`.
- Hook phases: 8 declared, 8 dispatched, 1 production implementation (`SafeObserverHook`), 0 additional registrations.
- Dead-with-zero-references-outside-crate: 4 domain DTOs, 2 page DTOs, 12+ envelope/invocation items, 2 traits, 1 accessor, 1 policy table.
- `EXECUTE_TIMEOUT` = 30 s fixed wall clock; provider `attempt_timeout_seconds` default 30, allowed 1..=60; `max_attempts` default 2, allowed 1..=2.

### 6.2 Reproduction commands (all run read-only)

```
rg -n "invoke_enveloped|ToolResultEnvelope|ToolObservability|ToolInvocation|ToolContext" crates --glob '!crates/intention-tools/**'
rg -n "ToolResultStatusDto|ToolResultMetadataEntryDto" crates --glob '!crates/intention-domain/**' | wc -l
rg -n "PageRequestDto|PageCursorDto" crates --glob '!crates/intention-types/**' | wc -l
rg -n "CreatePlanCommandDto" crates
rg -n "LocalToolInvocationPort" crates
rg -n "next_after_sequence" crates
rg -n "future_dependencies" quality Cargo.toml
rg -n "dead_code" crates/*/src
rg -n "EXECUTE_TIMEOUT|bounded_output_with_timeout" crates/intention-tools/src/lib.rs
rg -n "attempt_timeout_seconds|max_attempts" crates/intention-config/src/lib.rs crates/intention-runtime/src/lib.rs
rg -n "DaemonReadinessDto::" crates/intention/src crates/intention-daemon/src
rg -n "dispatch_with_cancellation" crates/*/src
rg -n "Phase::" crates/intention-application/src/lib.rs
for d in crates/*/; do ... done   # per-crate src/tests line counts (see §1.1 table)
python3 heuristic over pub items for out-of-crate references (session artifacts, not committed)
git log -S "ToolResultRecordedEventDto" / -S "invoke_enveloped" / -S "RunStreamSubscription" / -S "PageRequestDto"
```

### 6.3 Full trace cost of one tool round (step counts)

Model call → durable result: (1) provider delta → runtime appends `ModelRunFactInputDto::tool_call_recorded` (`intention-runtime/src/lib.rs:713`); (2) runtime calls the tool executor; (3) daemon/app parses the call (`parse_tool_input`, `intention-daemon/src/lib.rs:899-928`); (4) application resolves the workspace and dispatches through the hook pipeline — invocation, before/after workspace resolution, before/after execution (`intention-application/src/lib.rs:423,484,532,582,635`); (5) tool executes via `dispatch_with_cancellation` (`:627`); (6) result flows through persist/model-context/published hooks (`:681,770`, plus `:1461-1515` context builders); (7) daemon normalizes `ToolResult::projection()` to the wire (`:941-968`); (8) runtime appends `tool_result_recorded` (`:730`/`:756`) and, on round completion, the assistant/call-result facts. Per tool round ≈ 8 phase dispatches, 8+ crate boundaries, 2 durable appends; the run loop adds a provider round before/after each tool round.

---

## 7. Uncertainty and open questions

1. **No runtime measurement.** Both timeout findings are proven from source, but actual provider latencies and typical command durations were not measured (no builds/tests allowed). Frequency claims (F02) are marked medium-confidence.
2. **`Ready`-always assumption.** F06 reads `health()` as a `const fn`; if any future path mutates the projection before serving, the finding changes. Today no code does.
3. **M7 spec not yet drafted.** F08's disposition depends on whether the M7 planning artifact reuses or rewrites the plan vocabulary; the audit can only note the zero consumers.
4. **Tauri needs unknown.** F05's cost/benefit depends on M6's chosen process model (async runtime vs sync threads). The client's start-on-failure behavior (`ProcessDaemonLauncher::launch` + private `StartupLock`, `crates/intention-client/src/lib.rs:138-146`) also interacts with the readiness question (F06); a dedicated process-lifecycle review is warranted at M6 start.
5. **Unused-dependency scan is textual.** A manifest entry whose crate name never appears in source could still be a macro-only or linker-only dependency; no such entry was found, but `cargo machete`-class tooling was not run (no builds allowed).
6. **Error-code metric is raw.** 175 codes with ≤1 reference outside constructors were not individually triaged to completion; those observed were genuine, but a per-code triage remains open.
7. **Line-savings estimates** for F03/F04/F05 are byte-level approximations from file reads, not diffed removals.
8. **Prior audits' scope.** `m5plus-slice1-security-theater-audit.md` and `test-audit-report.md` were used as context; where they conflict with this pass, this draft's direct `rg`/read evidence supersedes.

— End of draft (Zone 6).


---

# Appendix G — Zone 7: plan, documentation, process, and their economics

Baseline: `main` @ `3291e50`, working tree clean except the untracked
`pr-44-45-review-report.md` (not read, not touched). All findings below are
derived independently from repository evidence, git/`gh` history, and public
sources. Nothing in this audit is read-only-but-assumed: every repo claim
carries `file:line`; every number shows its derivation.

---

## 1. Scope and method

### 1.1 Object

The delivery machinery rather than the product code: the roadmap's structure
and sequencing, the documentation and governance corpus, the decision
machinery, the quality/CI process, and the cost each imposes per change
relative to value delivered. Judged by fitness for daily use by one local
user, under the conditions stated in `AGENTS.md:11-16` (no deployed users, no
external data, no third-party consumers, backward compatibility not required).

### 1.2 Method (what was actually done)

1. **Quantify the corpus** — file counts, line counts, byte counts per
   documentation area; classify architecture documents into
   implemented-relevant vs unimplemented.
2. **Measure per-change cost from history** — for representative merged PRs
   (`#14`, `#21`, `#36`, `#44`, `#45`), count files and lines changed, split by
   `docs/` vs `crates/` vs `quality/` vs `.github/`, and identify which
   documents a single mechanism change forced to change.
3. **Measure rework** — trace files created in one PR to their deletion in
   later PRs; sum insertions vs deletions across the reversal wave.
4. **Measure the gate** — CI wall-clock from `gh run list` (40 runs),
   historical single-job `make ci` durations from closeout evidence, the
   Makefile's serial-execution property, and the required-check count.
5. **Measure decision velocity and churn** — ADR authoring dates, ADR statuses,
   which ADRs unwind earlier accepted ADRs.
6. **Check the claim layer** — attempt to falsify specific claims the corpus
   makes about itself (check counts, milestone baselines, evidence status).
7. **Compare with simpler practice** — public sources with URLs, marked
   honestly where the comparison is general practice rather than documented.

Constraints honored: no builds, no tests, no writes outside this file, no
sub-agents, read-only `git`/`gh`.

### 1.3 Corpus size (measured)

| Area | Files | Lines | Bytes |
| --- | ---: | ---: | ---: |
| `docs/intention-relay/architecture/` | 32 | 13,549 | 787,971 |
| `docs/intention-relay/decisions/` | 50 | 7,522 | 443,407 |
| `docs/intention-relay/reconciliation/` | 9 | 3,617 | 445,072 |
| `docs/intention-relay/closeout/` | 6 | 779 | 64,967 |
| `docs/intention-relay/legacy-baseline/` + `legacy-antibusy-prompts/` | 24 | 746 | (not measured) |
| `docs/intention-relay/m4plus_concept.md` | 1 | 7,731 | 428,233 |
| `docs/intention-relay/m4.md` | 1 | 316 | 20,988 |
| **`docs/` total** | **149** | **39,166** | — |
| Rust in `crates/` | 85 | 53,319 | — |
| — of which files under a `tests` path | — | 24,962 | — |

Derivations: `find docs -name '*.md' | wc -l`; `find … | xargs wc -l`;
`find crates -name '*.rs' | wc -l`; `find crates -name '*.rs' -print0 | xargs -0 cat | wc -l`;
`find crates -name '*.rs' -path '*tests*' -print0 | xargs -0 cat | wc -l`.

Note on the last row: 24,962 lines live in files whose path contains `tests`
(integration tests). Inline `#[cfg(test)] mod tests` blocks inside `src` add
more and were not separately measured, so production Rust is **at most
28,357 lines**. The `docs/intention-relay/` corpus alone (26,213 lines across
121 files, derived from the per-area table) is therefore ~92% of all production
Rust lines in the repository.

### 1.4 Timeline (measured)

First commit 2026-07-31, HEAD 2026-10-02 → **63 days**. 166 commits, 0 merge
commits (squash-merged), 46 PRs. Commits by month: 6 (Jul), 147 (Aug), 10
(Sep), 3 (Oct) — the repository's own history shows the *codeburst* month was
August, and that the last 34 days produced almost no commits and, as shown in
§3 F01, no net user-visible capability.

Commit-type distribution (`git log --format=%s | sed 's/(.*//' | sort | uniq -c`):
49 `docs`, 38 `fix`, 33 `feat`, 18 `chore`, 7 `test`, 4 `refactor`, 3 `ci`,
2 `perf`, 1 `style`, 1 `revert`. **54 of 166 commits (32%) changed nothing but
documentation** (measured by iterating every commit and testing whether all
changed paths match `docs/`, `*.md`; the script counted 54).

---

## 2. Vectors examined → verdicts

| # | Vector | Verdict |
| --- | --- | --- |
| V1 | Roadmap sequencing: M5+ as hard prerequisite of M6-M9 | **Harmful.** Gates the first usable UI behind a retrospective package whose own track record is ~50% reversal |
| V2 | Pre-authorization of contracts before consumers exist | **Harmful.** This is the generative rule behind the deleted Slice-1 surface and the reverted Slice-2 surface |
| V3 | Documentation volume and the mandated reading set | **Disproportionate.** 228 KB of mandated reading before an architecture-affecting edit; more spec lines for non-existent code than for shipped code |
| V4 | Documentation fan-out: how many docs one mechanism change forces | **Disproportionate.** 60 doc files (32 architecture, 20 decisions, 8 registers) for one wire-format change |
| V5 | Milestone planning machinery (`.factory/skills/...`) | **Ceremony.** 352 tracked process lines producing a document, not a running system; the project's own records show the spec-before-code rule is not followed |
| V6 | Evidence/claim layer (registers, closeout, ADRs) | **Partly theater.** 62% of registered evidence is `Planned`; no gate validates any numeric or status claim; a live contradiction and a wrong baseline SHA pass every check |
| V7 | Quality gates + CI | **Justified.** 8.9-minute mean CI wall over 40 runs; 8 parallel matrix jobs; the perf work already paid for itself |
| V8 | Machine-readable policy fan-out | **Disproportionate but small.** One crate name appears in 27 files and in three hand-maintained lists inside two policy files |
| V9 | Retention / archaeology | **Drifting.** 41 files mention "superseded", 226 mentions of "revert", 23 files still reference deleted mechanisms; no retirement rule for a document whose subject no longer exists |
| V10 | Reconciliation registers | **Mixed.** Single-owner-per-topic is right; hand-maintaining 317 rows of it is not |
| V11 | Closeout-evidence ritual | **Ceremony with a real core.** The idea (observable acceptance) is right; the 206-line mixed-veracity record is not, and its central baseline SHA is not on `main` |
| V12 | `../../m4.md` charter and `m4plus_concept.md` | **Retained archaeology.** A closed charter that `AGENTS.md:5` still directs agents to read, and a 7,731-line concept whose core transport/contract families were deleted |

---

## 3. Findings (ranked cards)

Severity: **S1** breaks daily use · **S2** rewrite or major drag · **S3**
tolerable tax · **S4** minor.
Stage: **shipped** (M0-M5 code + the docs/CI that closed them) · **committed**
(M5+/M6-M9 approved) · **doc-only**.

---

### Z7-F01 — M5+ blocks the first usable UI behind a retrospective package that reverses itself

- **Severity** S1 · **Stage** committed · **Type** LANDMINE · **Confidence** high
- **Trigger.** Anyone tries to *use* this product daily. There is no UI binary
  at all (`README.md:47-53`: "No user-facing UI or released product exists
  yet"; `README.md:186-196`: "No desktop (Tauri/M6) or usable terminal
  application"). M6 is the first usable UI and it is gated.
- **Mechanism.** `docs/intention-relay/architecture/11-implementation-roadmap.md:404`
  declares Milestone 5+ the "hard prerequisite for M6-M9";
  `…/decisions/0035-…:109` restates it ("M5+ is the hard prerequisite of M6,
  M7, M8, and M9"); `…/11-…:640` requires "all five slices … complete in
  order; every slice is fully ready before the next slice begins shipping".
  Slices 3, 4, and 5 are not implemented (`README.md:88`).
- **Blast radius.** Time-to-usable-product. Measured: in the 34 days between
  the M5 close (2026-08-29, `254a029` = PR #14) and HEAD (2026-10-02), the
  merged net change to `crates/` was **+13,715 / −10,201 lines** with no new
  crate, no new tool, no UI, and no new user-visible capability
  (`git diff --shortstat 254a029..HEAD -- crates/`). The same window contains
  the largest deletions in the repository's history: PR #43 removed 51,473
  lines, PR #44 removed 14,040, PR #45 removed 5,287, PR #46 removed 2,770 —
  **73,570 deleted vs 13,535 added** (`gh pr list` additions/deletions, summed).
  PR #36 alone had added 71,755 lines 4 days earlier.
- **Simplest alternative.** Invert the dependency: ship a minimal M6 against
  the already-stable M5 contracts (protocol, storage, run loop, six tools all
  closed and green), then run the retrospective slices *behind* a product that
  is in daily use. Delete the "hard prerequisite" clause. Rust's compiler-team
  major-change process is explicitly designed so that "if you have a plan to
  make some 'major change' … it may either simply be approved, but if the
  change proves more controversial or complex, we may escalate towards design
  meetings, longer write-ups, or full RFCs" — escalation is proportional and
  never gates unrelated work (https://rust-lang.github.io/rfcs/2904-compiler-major-change-process.html).
- **Disposition** re-scope · **Effort** M (roadmap + ADR edits, ~1-2 days) for
  the decision; L (actual reordering) · **Cost of delay** every additional week
  spent on slices 3-5 is another week with no product.

---

### Z7-F02 — Pre-authorizing contracts before a consumer exists is the generative defect

- **Severity** S1 · **Stage** committed · **Type** MIXED · **Confidence** high
- **Trigger.** A slice is "approved together as one package" and "No slice may
  ship in a half-ready state" (`…/decisions/0035-…:116`; roadmap `:521-524`),
  so each slice must pre-authorize its full contract surface — DTO versions,
  wire families, tags, limits — before any consumer exists.
- **Mechanism.** The rule itself is in the ADR and the roadmap. Its output is
  measurable in the repository's own retained audit
  (`m5plus-slice1-security-theater-audit.md`, tracked, cited by
  `docs/intention-relay/README.md:14`): ~3,646 production lines with **zero
  non-test consumers**, ~4,698 test lines, 91 inline tests, 13 fixture files
  that "exist substantially because the coverage policy pays for them"
  (`m5plus-slice1-security-theater-audit.md:47-56`).
- **Blast radius.** Human-days. The primary evidence is the commit arithmetic,
  not the audit's opinion: PR #36 added 71,755 lines; PRs #43-#46 then deleted
  73,570 and added 13,535. Of the 43 files *created* in PR #36, **29 (67%) were
  deleted within four days** by PRs #43-#46 (derivation:
  `git show --diff-filter=A --name-only 37bad4e` ∩
  `git show --diff-filter=D --name-only {4bce50e,b99ad42,9b07886,3291e50}`).
  Slice 3's implementation reached 83,493 added lines across 85 files
  (PR #42, `gh pr view 42`) and was **closed unmerged**.
- **Simplest alternative.** A consume-first rule: a contract type ships in the
  same change as its first real consumer, and a type with no producer and no
  consumer in the current tree is deleted (this is, verbatim, the project's own
  bar: `reconciliation/deferred-excluded-register.md` EXC-035/036, quoted at
  `m5plus-slice1-security-theater-audit.md:41-45`). Drop "no half-ready slice"
  and replace it with "no unconsumed surface".
- **Disposition** remove the rule; keep the bar · **Effort** S (one rule) /
  L (already-paid cleanup) · **Cost of delay** every further pre-authored
  slice repeats the pattern at ~1-2k lines per contract family.

---

### Z7-F03 — More architecture is specified for code that does not exist than for code that does

- **Severity** S2 · **Stage** doc-only · **Type** EXCESSIVE · **Confidence** high
- **Trigger.** A developer (human or agent) starts any architecture-affecting
  change and has to work out which of 32 documents bind.
- **Mechanism.** `docs/intention-relay/architecture/06` (VFR/Headroom, M8) and
  `07` (Plan/Build, M7) plus `13`-`30` (Mandate track M10-M12 and post-M5,
  all unimplemented) total **7,624 lines**; the documents describing
  implemented behaviour (00-05, 08-12, README) total **5,925 lines**
  (derived from `wc -l` per file; 13-30 alone are 7,257).
- **Blast radius.** Reading cost on every change, and a wrong scope signal:
  the corpus reads as if all of it is current. Concretely, `AGENTS.md:7`
  requires reading "the relevant documents … completely enough" and
  `AGENTS.md:8` names the quality-gate policy, the TDD policy, and the
  roadmap. Those three plus the architecture index compose a **228,123-byte**
  mandated reading set (≈57k tokens at ~4 bytes/token — `wc -c` on the six
  files; the byte divisor is an estimate, not a measurement).
- **Simplest alternative.** Keep unimplemented specifications out of the
  binding set: move `13`-`30` and `06`/`07` under an `unimplemented/` prefix
  excluded from `AGENTS.md`'s mandated list, and promote each document into
  the binding set in the same change that starts its milestone. GitHub's study
  of 2,500+ agent instruction files found the opposite of this repository's
  approach works: "Start simple. Test it. Add detail when your agent makes
  mistakes. The best agent files grow through iteration, not upfront planning"
  (https://github.blog/ai-and-ml/github-copilot/how-to-write-a-great-agents-md-lessons-from-over-2500-repositories/).
- **Disposition** re-scope · **Effort** M (~2-3 days, mostly link repair) ·
  **Cost of delay** unbounded reading tax per change.

---

### Z7-F04 — One mechanism change forces a corpus-wide documentation sweep

- **Severity** S2 · **Stage** shipped · **Type** EXCESSIVE · **Confidence** high
- **Trigger.** Changing an internal mechanism whose concepts are restated in
  many documents. Worked example: the JSON-RPC 2.0 rebuild (PR #44, `b99ad42`,
  ADRs 0045-0048).
- **Mechanism.** PR #44 touched **120 files: 60 under `docs/` (28 of the 32
  architecture documents, 20 decision records, all 8 reconciliation
  registers), 53 under `crates/`** — i.e. **1.13 documentation files per code
  file** (`git show --name-only b99ad42 | grep -c '^docs/'` = 60;
  `… | grep -c '^crates/'` = 53). Doc churn was +2,684/−1,949 lines against
  code churn +4,669/−11,489 (`git show --numstat b99ad42`). PR #45 (a coverage
  policy change) similarly touched 37 documentation files out of 100.
- **Blast radius.** Human-days of mechanical editing and a review surface
  whose documentation half exceeds its code half; the root cause is that a
  single concept (`execution meaning`, canonical codec, tag registry, fixed
  limits) is restated in owner documents, roadmap, ADRs, and registers rather
  than living in one owner and being linked.
- **Simplest alternative.** One owner per mechanism; every other document links
  to the owner and never restates its table. Registers become machine-generated
  from ADR front matter plus the policies (see F13).
- **Disposition** collapse · **Effort** L · **Cost of delay** the next
  mechanism change pays the same N× edit again.

---

### Z7-F05 — The milestone planning machinery produces documents, and the project's own records show the spec-before-code rule is not what happens

- **Severity** S2 · **Stage** committed · **Type** BUREAUCRACY · **Confidence** high
- **Trigger.** Starting any of M6-M12 or an M5+ slice: the roadmap requires the
  milestone to "begin only after its approved implementation specification
  declares crates, DTO/wire/storage versions, feature profiles, coverage
  declarations …, fixtures, and outcome evidence" (`11-…:439-447`, repeated per
  milestone).
- **Mechanism.** The process is codified in a tracked skill:
  `.factory/skills/plan-intention-relay-milestone/SKILL.md` (195 lines) plus
  `checklists.md` (101) and `references.md` (56) = **352 tracked process
  lines** (added in `1501839 chore(factory): add milestone planning skill`).
  It requires: a discovery todo list, 7 numbered phases, decision briefs with
  five mandatory parts each, at most 4 questions per round, a live pre-draft
  with six sections, a consolidated draft, then a specification whose every
  implementation step names purpose, behaviour, files, tests, validation,
  acceptance evidence, and non-goals, followed by explicit approval.
- **Evidence that this is ceremony rather than control.** Four decision records
  accepted inside the same window are self-described *post-hoc*:
  "Accepted as a Milestone 5+ retrospective completion of ADR 0019"
  (`decisions/0039-…:5-6`); `decisions/0041-…:5` likewise; `0042` at `:8`;
  `0040` at `:1`. So code landed and the record followed — while the roadmap's
  rule says the reverse. Decision velocity is bursty and volatile: 19 ADRs on
  2026-08-16/17, 16 ADRs on 2026-08-30, 7 on 2026-09-26, 5 on 2026-10-02
  (derived from `git log --diff-filter=A --format=%ad -1 -- <adr>`), and of
  ADRs 0035-0049, **7 unwind decisions accepted days or weeks earlier**
  (0038 removal program; 0044 revert of 0037; 0045/0046 removal of 0036's
  surface; 0047 rewriting WorkspaceRoot policy; 0048 removing limits and the
  corridor/period/queue-audit directions; 0049 replacing the coverage tiers).
- **Blast radius.** Human-days before any code exists, per milestone, × up to
  18 scheduled units (M5+ slices 3-5, M6-M12).
- **Simplest alternative.** One design note per milestone written *at the start
  of implementation*, with escalation only on genuine controversy — the Rust
  MCP pattern, whose stated goals include "to avoid a lot of process overhead"
  and which explicitly says a proposal may be "simply … approved" at the
  lightweight tier.
- **Disposition** simplify · **Effort** M · **Cost of delay** each milestone
  pays planning cost before delivery value.

---

### Z7-F06 — The claim layer is unguarded: 62% of registered evidence is planned, and no gate checks any claim

- **Severity** S3 · **Stage** shipped · **Type** THEATER · **Confidence** high
- **Trigger.** A reader — human or agent — trusts a statement in the corpus.
- **Mechanism and falsification attempts.**
  - Evidence register status counts: **9 `Verified`, 38 `Planned`, 14 `Not
    applicable`** (`rg -o '| (Verified|Planned|…) |' reconciliation/evidence-register.md`).
    66 EVD ids exist; 38/61 of the classified rows are promises.
  - The documentation gate cannot catch a claim error:
    `quality/check_docs.py:20-84` checks unbalanced fences, Mermaid diagram
    type, Markdown link resolution under `docs/`, and a secret-shaped-assignment
    regex. Nothing else. (Note also `check_docs.py:84` prints "navigation …
    checks are valid" although the script contains no navigation check.)
  - Live demonstration: `architecture/12-quality-gates-and-makefile.md:44`
    says "branch protection requires the **eight** resulting status checks",
    while the same file at `:329` says "the **nine** required status checks",
    and `decisions/0040-…:108` says "The nine required status checks". The
    actual matrix contains **8** jobs (`grep -cE '^ +- os:' .github/workflows/quality.yml`).
    The stale 9 dates from the `selftest` job removed in PR #46 on the same day
    the "eight" sentence was written. Both survive every gate.
  - Live demonstration 2: `closeout/m5-closure-evidence.md:5,25,152` call
    `bf40567` "the immutable merged baseline … (PR #14, **merged to `main`**
    2026-08-29)". `git merge-base --is-ancestor bf40567 HEAD` → **NO**;
    `git branch -a --contains bf40567` lists only feature branches
    (`docs/m4plus-concept-cleanup`, `feat/production-ceiling-removal`,
    `impl/m5plus-slice1-contracts`, `test-ssh-sign`). The commit actually merged
    to `main` for PR #14 is `254a029`. CI run 33265408980 really did run on
    `bf40567` (`gh run view 33265408980 --json headSha`) — its PR-branch head —
    so the evidence is real but attached to a commit that is not in the
    delivered history. The same record admits, in the same table, "no CI run was
    recorded against the pre-merge tree" and "the prior full `make docs-check`
    result remains a 2026-08-26 run on an earlier tree".
  - The register already names the missing control: **EVD-011 "Full
    Markdown/link/status/claim inventory checks" — Planned**
    (`reconciliation/evidence-register.md:33`).
- **Blast radius.** The corpus's entire value proposition is traceability; a
  reader cannot separate verified from asserted, and the wrong baseline costs
  real time whenever someone tries to reproduce M5.
- **Simplest alternative.** Machine-check every number that appears in more than
  one place (check counts, coverage values, milestone SHAs are all derivable);
  render a machine-generated evidence table keyed to a commit reachable from
  `main`; and mark inline text as verified/unverified instead of writing both
  in the same voice.
- **Disposition** simplify · **Effort** M · **Cost of delay** scales with every
  milestone closed on top of it (M9's own plan already schedules "documentation
  reconciliation", `11-…:703-707`).

---

### Z7-F07 — Declaration fan-out: one entity named in 27 files and three hand-maintained lists

- **Severity** S3 · **Stage** shipped · **Type** EXCESSIVE · **Confidence** high
- **Trigger.** Adding, renaming, or removing a production crate.
- **Mechanism.** `rg -l 'intention-tools'` over the repository (excluding its
  own crate directory) returns **27 files**, including `quality/architecture.toml`
  (repeated across `active_production_crates`, `[dependencies]`,
  `[external_dependencies]`, and `[[future_crates]]` with `responsibility` +
  `test_targets`), `quality/coverage.toml` (twice: `coverage_crates` and
  `production_crates`), `quality/check_architecture.py`, `Cargo.toml`, five
  consumer `Cargo.toml`s, the root `README.md` crate map, `architecture/01`,
  `architecture/05`, `architecture/12`, `architecture/15`, a closeout record,
  five ADRs, and four reconciliation registers.
  `quality/architecture.toml:8-31` and `quality/coverage.toml:14-49` are two
  independently hand-maintained copies of the same crate list; nothing derives
  one from the other.
- **Blast radius.** Silent drift between policies; a crate can satisfy one gate
  and not another. This is the same class of defect the project already
  recognises in Slice 1 ("tag inventory in ≥6 hand-maintained copies",
  `m5plus-slice1-security-theater-audit.md:70`) but has not generalised.
- **Simplest alternative.** One machine-readable crate manifest (the workspace
  `Cargo.toml` is already authoritative) and generate the coverage/architecture
  lists inside `check_architecture.py` from it; documentation links to the
  policy rather than restating it.
- **Disposition** collapse · **Effort** S (~1 day) · **Cost of delay** every
  crate addition or rename pays it.

---

### Z7-F08 — A 2,183-line review ledger for a PR that was never merged and whose content was reverted

- **Severity** S4 · **Stage** doc-only · **Type** DEAD · **Confidence** high
- **Trigger.** Anyone browsing `reconciliation/` for the current state.
- **Mechanism.** `docs/intention-relay/reconciliation/pr24-code-review-ledger.md`
  is **2,183 of the directory's 3,617 lines (60%)**. It records "three
  read-only review waves over the closed PR #24 branch"
  (`reconciliation/README.md:146`). PR #24 was **closed, not merged**
  (`gh pr view 24` → `"state":"CLOSED"`, `impl/m5plus-slice2-control-plane`,
  +52,522/−6,305), and the surface it implemented was later removed by ADR 0044
  (`4bce50e`, −51,473 lines).
- **Blast radius.** Search and reading cost; a stale-looking normative document
  inside the authoritative directory.
- **Simplest alternative.** Review findings belong in the PR conversation of
  the PR under review. The project has already retired one such register —
  PR #39 is titled "… retire the PR #36 review register" and deleted 4,812
  lines (`0cd2540`) — so the pattern is known and cheap to end.
- **Disposition** remove · **Effort** S (delete + 1 link fix) · **Cost of delay** negligible.

---

### Z7-F09 — Milestone closure is a 206-line mixed-veracity ritual

- **Severity** S3 · **Stage** shipped · **Type** BUREAUCRACY · **Confidence** high
- **Trigger.** Closing a milestone.
- **Mechanism.** `closeout/m5-closure-evidence.md` is 206 lines carrying a
  4-column evidence table (`:25-42`) that interleaves: post-merge CI results at
  two heads; historical pre-merge worktree rows after a commit (`299d922`) that
  is not the baseline; an admitted unrun gate ("Unrun gates for this dirty
  baseline — Historical: no CI run was recorded against the pre-merge tree");
  four separate explanatory paragraphs about which worktree was clean when; a
  criterion matrix whose right-hand column is "Focused tests pass" or "Evidenced
  by listed tests" (`:60-88`); and a 6-row persistence table quoting exact
  sequence numbers (`:101-117`). The pinned "immutable merged baseline" is not
  an ancestor of `main` (F06).
- **Blast radius.** Human-days per milestone; and the record is authoritative
  for the *next* milestone's predecessor checks, so its ambiguity propagates.
  M9 already carries "documentation reconciliation against implemented public
  contracts" as closing work (`11-…:703-707`).
- **Simplest alternative.** A closeout is a pointer plus a machine-generated
  table: the merge commit (`254a029`), the CI run URL, coverage JSON from
  `quality/reports/`, the test count. Retire the prose that explains which tree
  was clean on which day.
- **Disposition** simplify · **Effort** S · **Cost of delay** small per
  milestone, compounding.

---

### Z7-F10 — The full local gate is serial and 20-40 minutes; the doc-coupling rule makes every change two changes

- **Severity** S3 · **Stage** shipped · **Type** EXCESSIVE · **Confidence** high
- **Trigger.** Handing off any implementation work.
- **Mechanism.** `AGENTS.md:72` requires `make quick` while iterating and
  `make verify` before handoff; `Makefile:7` sets `.NOTPARALLEL:`, so every
  prerequisite of `check`, `coverage`, and `deps` (`Makefile:115-118`) runs
  serially across three feature profiles. The measured single-job cost is in
  the project's own record: `make ci` "PASS (21m37s)" on Ubuntu and
  "PASS (39m40s)" on Windows (`closeout/m5-closure-evidence.md:196-199`) —
  that is the pre-parallel-era serial gate, which is the shape of a local
  `make verify` today.
- **Blast radius.** 20-40 minutes per handoff, and the doc-coupling rule in
  `AGENTS.md:73` ("Changes to Makefile, linting, coverage, feature profiles, or
  supply-chain policy must update the relevant machine-readable policy **and
  architecture documentation** in the same change") is what produced 60/37 doc
  files in PRs #44/#45 (F04).
- **Simplest alternative.** Keep `make quick` as the developer gate and let CI
  run the full matrix (it already does, in parallel, in a **mean 8.9 minutes**
  over 40 runs — `gh run list --workflow=quality.yml --limit 40`, computed as
  `updatedAt − createdAt`). Narrow the doc-coupling rule to public contracts;
  internal mechanisms change code and one owner document.
- **Disposition** simplify · **Effort** S · **Cost of delay** per-change friction.

---

### Z7-F11 — No retirement rule: the corpus keeps archaeology it can no longer verify

- **Severity** S3 · **Stage** doc-only · **Type** DEAD · **Confidence** medium-high
- **Trigger.** Reading or searching the corpus for current truth.
- **Mechanism.** 41 documentation files contain "superseded"; 226 occurrences of
  "revert"-family words across `docs/` and `README.md`; 23 documentation files
  still reference `typed-tlv`/`run-execution-meaning-v4`, deleted mechanisms
  (`rg -l`). `m4plus_concept.md` is retained read-only at **7,731 lines** while
  its core transport and typed-record families were explicitly superseded
  (`docs/intention-relay/README.md:16`); `m4.md` (316 lines) is a closed charter
  that `AGENTS.md:5` still directs agents to read.
- **Blast radius.** Monotonically increasing search/read cost and a real risk of
  an agent acting on a superseded rule; the corpus currently has no document
  whose subject is gone and which leaves the reading path.
- **Simplest alternative.** A retirement rule: when a mechanism is deleted, its
  document moves under `docs/reference/archive/` (already exists,
  `docs/README.md` already separates active from preserved material) and drops
  out of `AGENTS.md`'s list and out of `check_docs.py:65`'s active scope. Keep
  one ADR per removal as the history.
- **Disposition** re-scope · **Effort** M · **Cost of delay** grows with every
  reversal (and this project reverses a lot: 7 of the last 15 ADRs).

---

### Z7-F12 — The roadmap is a 2,004-line monolith where history, plan, and speculation are interleaved

- **Severity** S3 · **Stage** doc-only · **Type** EXCESSIVE · **Confidence** high
- **Trigger.** Any question of the form "what is next?".
- **Mechanism.** `11-implementation-roadmap.md`: 2,004 lines, 48 `##` sections,
  109 `###` sections. **27 of those sections are labelled "Documentation-only
  package"**, 22 restate "activates no crate, schema, migration, protocol
  implementation, feature profile, quality-policy target, or implementation
  milestone", and 14 repeat "activation remains excluded pending a later M5+
  specification" (all counts by `grep -c`). Closed-milestone records, the
  mandatory-reading "Quality rule for every milestone", the M5+ slice list, the
  M6-M9 plan, and ~20 future direction packages share one file.
- **Blast radius.** Planning errors. Example of the resulting confusion: the
  document simultaneously says M5+ "does not renumber, replace, or claim
  delivery of Milestones 6-9" (`:410-414`) and that no M6-M9 boundary behavior
  may be implemented until all five slices ship (`:404`, `:640`) — which is a
  delivery claim in practice, and the one that gates the product (F01).
- **Simplest alternative.** Split into three files: `roadmap.md` (current
  forward plan, target ~150 lines), `milestone-records.md` (closed milestones),
  and `deferred-directions.md` (the direction packages, kept as appendix and
  removed from the mandated reading set per F03).
- **Disposition** collapse · **Effort** M · **Cost of delay** per planning session.

---

### Z7-F13 — 317 hand-maintained reconciliation rows

- **Severity** S4 · **Stage** doc-only · **Type** MIXED · **Confidence** high
- **Trigger.** Adding or changing a direction; answering "who owns this topic?".
- **Mechanism.** `reconciliation/source-of-truth-matrix.md`: 490 lines,
  **317 topic rows**, of which 299 are disposition `Adopt`
  (`rg -c '^\| [A-Z]{3}-[0-9]+'` = 317; disposition counts by `rg -o`). Each row
  carries topic ID, proposition, applicability, disposition, owner,
  compatibility rule, delivery bucket, and evidence status. The "Coverage
  ledger" section itself says topic ranges "do not claim claim-level
  completeness" (`:418-420`).
- **Blast radius.** Low individually, but it is a fourth copy of rules already
  stated in owner documents, an ADR, and the roadmap; three of the four copies
  can drift while every gate stays green.
- **Simplest alternative.** Keep the *idea* (one owner per topic) and generate
  the table from ADR front matter (topic ID, owner document, status), deleting
  the hand-written prose column. A 317-row table that no consumer reads is
  cheaper generated than maintained.
- **Disposition** collapse · **Effort** M · **Cost of delay** small.

---

### Z7-F14 — The instruction channel will inherit this corpus's reading cost as a runtime cost

- **Severity** S2 · **Stage** doc-only (Slice 5, ADR 0043) · **Type** LANDMINE · **Confidence** medium
- **Trigger.** The product becomes self-hosted: an Intention Relay session runs
  inside a repository that has an `AGENTS.md` like this one. Per
  `architecture/30-instruction-sources-and-system-context.md` (ADR 0043), the
  instruction channel reads workspace `AGENTS.md` through the `WorkspaceRoot`
  anchor, assembles it deterministically, and **freezes the effective
  instruction projection at admission**.
- **Mechanism.** This repository's own `AGENTS.md` is 8,144 bytes and, at
  `:7-8`, instructs the agent to read the architecture documents "completely
  enough" and to consult arch 10, arch 11, and arch 12. That resolves to the
  measured 228,123-byte mandated set. `architecture/30:…` additionally makes
  the projection immutable per run, so the cost cannot be amortised across
  steps that legitimately need different subsets.
- **Blast radius.** Context exhaustion and silent quality loss on every
  architecture-affecting run; a truncated or partially-followed instruction set
  is invisible because the projection is frozen and (by design) advisory-only.
  The failure is not an error message, it is quietly worse work.
- **Simplest alternative.** Cap the instruction channel at the small, authoritative
  file and move the deep material behind the tool surface the agent already has
  (`read`/`grep` on demand). Decide this at Slice-5 activation, when the
  projection budget is designed, rather than discovering it at M6.
- **Disposition** re-scope (design the budget at Slice-5 activation) ·
  **Effort** S · **Cost of delay** discovered late = rediscovery cost at M6.
- **Uncertainty.** Slice 5 is doc-only; if the channel ships with a projection
  budget, the finding is absorbed by design. Confidence is medium for that
  reason.

---

## 4. Keep-list (justified, with why)

| # | Mechanism | Why it survives the minimal-world test |
| --- | --- | --- |
| K1 | **CI as a parallel matrix with pinned tools, rust-cache, sccache, and cache cleanup** (`quality.yml`, 221 lines; `cache-cleanup.yml`) | Measured **mean 8.9-minute wall over 40 runs** (`gh run list`, `updatedAt − createdAt`). PRs #10-#13, #16, #23 spent ~3.6k changed lines to take CI from a 21-40 min serial gate to this. Real, self-financing. Keep as-is. |
| K2 | **`make quick` / `make verify` split** (`Makefile:112,118`) | The inner loop is genuinely small (tools-check, fmt-check, profile lint, default-profile tests) and the heavy gate is deferrable. Right shape; F10 only asks that the heavy gate stop being a local handoff requirement. |
| K3 | **Single base 80% coverage threshold + empty designated-files list** (ADR 0049, `quality/coverage.toml`) | Replaced the A/B/C tier scheme, removing a whole category of policy prose. 80% as a *floor* is defensible; Google's own coverage guidance is that coverage is a proxy whose value is in finding untested code, not in the number (https://testing.googleblog.com/2020/08/code-coverage-best-practices.html). Keep the number, keep it a floor. |
| K4 | **Limits-by-precedent** (ADR 0048, `production-ceiling-removal.md`, 75 lines) | The single best policy in the corpus: no speculative caps, no runtime content scanning, liveness safeguards only. Cheap to apply and it deletes work rather than adding it. |
| K5 | **Single-version / no backward compatibility** (ADR 0038) | Matches the stated conditions exactly (no deployed users, no external data). Removes migration machinery, golden fixtures, and legacy readers. Every removal in PRs #43-#46 is downstream of this policy working. |
| K6 | **Pinned toolchain + `--locked` + non-mutating gates + `bootstrap-tools` as the only installer** (`Makefile:34-109`) | Small surface, strong reproducibility property, no hidden installs. Justified. |
| K7 | **ADRs as the unit of decision, with rationale and status** | The practice is right and cheap per decision; the *size* (7,522 lines/50 records, avg 150 lines) and the corpus-wide ripple are the problems (F04, F05), not the existence of records. Nygard's original framing is deliberately light — a record should be short and incremental (https://www.cognitect.com/blog/2011/11/15/documenting-architecture-decisions). |
| K8 | **Closeout evidence as a concept** | Judging a milestone by observable acceptance rather than by code structure is correct. F09 targets the record's mechanics, not the idea. |
| K9 | **Advisory-only, authority-free instruction channel design** (arch 30, ADR 0043) | Cannot leak authority, frozen at admission, no untrusted material — a sound design. F14 asks for a budget, not a redesign. |
| K10 | **Opt-in, non-blocking live-provider e2e** (ADR 0040, `real-api-e2e.yml`) | ~112 workflow lines and one ignored target; never blocks a gate; gives the only real-provider signal. Effectively free. |
| K11 | **`check_docs.py` link/fence/Mermaid/secret check** (84 lines) | Cheap and catches real breakage in a 39k-line corpus. Keep; F06 asks it not be *mistaken* for claim validation. |
| K12 | **Machine-readable architecture/dependency policy** (`architecture.toml`) | Encoding boundaries as data, not prose, is right — the checker is 730 lines and enforces real isolation. F07 is about de-duplicating the lists, not about deleting the policy. |
| K13 | **Platform-native paths in fixtures (no POSIX-only literals)** | `AGENTS.md:69`; the Windows CI fixture found a real coverage distortion (see `m5-closure-evidence.md:222-231`). Real cross-platform value. |
| K14 | **Observational metrics that never change a verdict** (`quality/metrics.py`, 132 lines; job manifests) | Correctly scoped: diagnostics that cannot fail a build. Keep the pattern. |

Not on the keep-list, deliberately: the **reconciliation register set** (keep
the ownership idea, F13), the **per-milestone pre-authorization rule** (F02),
and the **`m4.md` mandated reading** (F11).

---

## 5. Predictions

| # | Prediction | Signal that confirms it | Confidence |
| --- | --- | --- | --- |
| P1 | **The "hard prerequisite" clause is cut or rewritten** before M6 starts; M6 will begin against M5 contracts with slices 3-5 running in parallel or later | A new ADR reordering the roadmap; or an `impl/m6-*` branch appearing while slices 3-5 are incomplete; or the dependency graph losing the `K --> F` edge | high |
| P2 | **The pre-authorization rule is replaced by a consume-first rule** | The first activating spec that declares fewer contracts than its slice's direction list; or a deletion PR removing types that shipped with no consumer | high |
| P3 | **`pr24-code-review-ledger.md` and the review-register pattern are cut** | A commit titled like `chore(docs): retire the PR #24 review ledger`; or a PR whose body says "findings live in the PR" (this already happened once: `0cd2540` retired the PR #36 register) | high |
| P4 | **`m4plus_concept.md` and `m4.md` leave the active reading path** | Either file moving under `docs/reference/archive/`; or `AGENTS.md:5` losing the m4.md sentence; or `check_docs.py`'s `docs`-only scope widening while the concept is excluded | med-high |
| P5 | **The reconciliation matrix becomes generated or shrinks** | A matrix row whose owner document no longer contains the topic; or an ADR landed without a matrix row; or a script appearing under `quality/` that emits the table | med |
| P6 | **The coverage policy is rewritten a third time** | The first request for a higher bar on a specific file — the designated-files mechanism is empty and has never been exercised, which is the classic prelude to replacing it with something ad hoc | med |
| P7 | **The M5+ five-slice decomposition is re-cut** (merge/split/reorder) | Slice 3 being re-planned after PR #42's 83,493 lines were discarded; or a slice gaining/losing a direction in `0035`/`0043` | med-high |
| P8 | **`make verify` stops being a per-handoff local requirement** and becomes CI-only, with `make quick` as the developer gate | A handoff that cites CI rather than a local `make verify`; or `.NOTPARALLEL` gaining a parallel path; or the roadmap's step 6 being amended | med |
| P9 | **Lint/coverage/feature/Makefile changes stop requiring architecture-documentation edits in the same change** (`AGENTS.md:73` narrowed to public contracts) | The next quality-policy PR touching code + policy but only 2-5 docs instead of 37 | med |
| P10 | **The 8-vs-9 check count and the wrong M5 baseline are never fixed by a gate, only by a human who noticed** | Absence of any `quality/` change that validates cross-document numeric claims; a later closeout repeating a squash-vs-branch SHA confusion | high |
| P11 | **`make quick` / `make verify` / pinned tools / no-backward-compat / limits-by-precedent survive unchanged** | No PR proposing a second build system; no migration machinery reintroduced; no new speculative caps | high |
| P12 | **Instruction-channel budget becomes an explicit design decision at Slice 5** | A bound or projection-budget field appearing in arch 30 or its activating spec | med |

---

## 6. Metrics and commands used

All commands were read-only. No build, test, or package-manager command ran.

**Corpus size**
```
find docs -name '*.md' | wc -l                                  # 149
find docs -name '*.md' -print0 | xargs -0 wc -l | tail -1       # 39,166 lines
for d in docs/intention-relay/*/; do … done                     # per-area files/lines
wc -l docs/intention-relay/architecture/*.md                    # 32 files / 13,549
wc -l docs/intention-relay/decisions/*.md                       # 50 files / 7,522
wc -l docs/intention-relay/reconciliation/*.md                  # 9 files / 3,617
wc -l docs/intention-relay/closeout/*.md                        # 6 files / 779
wc -l docs/intention-relay/m4plus_concept.md                    # 7,731
wc -c AGENTS.md docs/intention-relay/README.md <arch 10,11,12,README>   # 228,123
wc -c docs/intention-relay/architecture/*.md …                  # 787,971 bytes
find crates -name '*.rs' | wc -l                                # 85
find crates -name '*.rs' -print0 | xargs -0 cat | wc -l         # 53,319
find crates -path '*tests*' -name '*.rs' … | wc -l              # 24,962
```

**History and cost**
```
git rev-list --count main                                       # 166
git log --reverse --format='%ad %h %s' --date=iso | head -3      # 2026-07-31
git log -1 --format='%ad' --date=iso                            # 2026-10-02
git log --format='%s' | sed 's/(.*//' | sort | uniq -c           # 49 docs / 38 fix / 33 feat …
git show --numstat --format='' <sha> | awk '…'                   # per-PR docs/crates/quality split
git show --diff-filter=A --name-only --format='' 37bad4e | sort -u
git show --diff-filter=D --name-only --format='' <#43..#46>      # 29/43 files created in #36 deleted
git diff --shortstat 254a029..HEAD -- crates/                    # +13,715 / −10,201
gh pr list --state all --limit 60 --json number,…,additions,deletions,changedFiles
gh pr view 24|42 --json state,additions,deletions,changedFiles,headRefName
```

**Gate and CI**
```
gh run list --workflow=quality.yml --limit 40 --json createdAt,updatedAt…   # mean 8.9 min
gh run view 33265408980 --json headSha,conclusion                 # bf40567…, success
gh run view 33273389437 --json headSha,conclusion                 # b930c14…, success
grep -cE '^ +- os:' .github/workflows/quality.yml                 # 8
grep -n 'eight resulting\|nine required' <arch 12, ADR 0040>
grep -n 'NOTPARALLEL\|^verify:\|^quick:' Makefile
```

**Claims check**
```
git merge-base --is-ancestor bf40567 HEAD ; git branch -a --contains bf40567
rg -o '\| (Verified|Planned|…) \|' reconciliation/evidence-register.md | sort | uniq -c
rg -l 'intention-tools' --glob '!target' . | wc -l                # 27 (excl. own crate)
rg -c '^\| [A-Z]{3}-[0-9]+' reconciliation/source-of-truth-matrix.md   # 317
grep -c 'Documentation-only package' 11-implementation-roadmap.md # 27
```

**External sources used**
- Rust compiler-team major change process (escalation proportional to
  controversy; "to avoid a lot of process overhead"):
  https://rust-lang.github.io/rfcs/2904-compiler-major-change-process.html
- GitHub's analysis of 2,500+ agent instruction files (grow by iteration, not
  upfront planning):
  https://github.blog/ai-and-ml/github-copilot/how-to-write-a-great-agents-md-lessons-from-over-2500-repositories/
- Nygard, "Documenting Architecture Decisions" (short, incremental records):
  https://www.cognitect.com/blog/2011/11/15/documenting-architecture-decisions
- Google Testing Blog, "Code Coverage Best Practices" (coverage as a proxy, not
  a target): https://testing.googleblog.com/2020/08/code-coverage-best-practices.html

---

## 7. Uncertainty and open questions

1. **Rust/test line attribution.** Production Rust was bounded at ≤28,357 lines
   from integration-test paths only; inline `#[cfg(test)] mod tests` volume was
   not measured. The docs:code ratio in §1.3 could move by ±15% if inline tests
   are large. The direction of every conclusion is unaffected.
2. **Local `make verify` duration is inferred, not measured** (no builds
   allowed). The 21m37s/39m40s figures are the pre-parallel single-job CI runs
   recorded in `closeout/m5-closure-evidence.md:196-199`; a modern laptop with a
   warm `target/` will differ in both directions. The serial-execution property
   (`.NOTPARALLEL`) is verified directly.
3. **Human-days were not measured at all.** The repository has no record of
   effort, only wall-clock and diff size. Every "human-days" statement is
   therefore about wall-clock and review surface, not about people.
4. **PR #42's 83,493 lines.** Confirmed closed-unmerged via `gh pr view 42`, but
   I did not verify whether the branch `impl/m5plus-slice3-harness` still exists
   remotely or whether that work was partially recycled. `git branch -a` shows
   no local slice-3 branch.
5. **`pr-44-45-review-report.md` was deliberately not read**, per the mission's
   instruction to derive findings independently. If it contains overlapping
   findings, the overlap is coincidental; if it contains contradicting evidence,
   this audit did not weigh it.
6. **F14 (instruction-channel budget) is the weakest finding** because Slice 5
   is unimplemented and the projection budget is not yet specified. It is
   included because the trigger condition is derivable now (228 KB mandated set
   + frozen projection) and the fix is cheap if taken at activation.
7. **Attribution of cause.** I attribute the 60k-line reversal wave to the
   pre-authorization rule (F02) and the sequencing gate (F01). The repository's
   retained audit attributes it to the mechanisms themselves. These are
   compatible but not identical claims; the commit arithmetic in F02 is mine and
   is independent of the audit's reasoning.
8. **Not examined (out of zone):** contract/DTO correctness, storage
   correctness, security properties of the tools, and the model-tool loop. Those
   belong to other zones.
