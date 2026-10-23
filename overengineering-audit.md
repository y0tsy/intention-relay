# Over-engineering audit — future surface and implemented code

> **Working artifact, not an approved decision.** Produced 2026-10-13 by ten read-only lanes: seven over the
> future surface (all 24 architecture documents, the roadmap/milestones/slices, the delivery process, and the
> repository requirements) and three over the implemented code. Every finding is a recommendation to accept or
> reject; nothing here overrides `docs/intention-relay/` or `AGENTS.md`.

## Framing

Intention Relay is a **local-first, single-user** agent application. One local daemon owns runtime and state; a
Tauri desktop UI and a terminal TUI/REPL talk to it over a **local socket** through one typed Rust client. No
remote transport, no multi-user access, no third-party consumers, no deployed users, no externally persisted
data. It is a v1 rewrite: M0-M5 and Slices 1, 1.5, 2 are done; M6-M11 and Slices 3-6 are planned, not built.

Consequences used throughout:

- **Security is a supplement, not a goal.** The only actor is the local user; machinery that models adversaries
  that do not exist, or hides information from the user, is a cost with no payoff.
- **Priorities:** (1) speed of scaling and shipping features, (2) end-user UI/UX quality, (3) low maintenance
  burden.
- **Nothing is distributed**, so process, versioning, and audit machinery that exists for external consumers is
  overhead.
- Never recommended: liveness safeguards, the single-live-version rule (no migrations), typed boundaries that
  prevent real defects, closed failure sets, and tests that catch real defects.

## Status

Waves R1-R5 landed (`eeaa99c`…`8b91742`, PR #71). The findings below cover the surface that is still live, with
code evidence, and the audit pass over the implemented crates. Findings those waves resolved are not repeated.

**2026-10-14 wave.** Commits `b79ebb1`…`95e0c49` on `cleanup/overengineering-removal`, pushed, close blockers 2, 3,
4, 8, 9, and 10: the remaining Skill machinery, the tool-contract taxonomy, the provider framework, plan frontmatter
and lifecycle, the VFR/Headroom crates, and the instruction model. Three code cleanups land with them
(`ModelToolDefinitionDto` validation removal, `ToolResultOutcomeDto::Cancelled` removal, and the tokio patch refresh
that keeps the stale-pin gate green). Findings below carry a `Resolved 2026-10-14` tag; deviations from the suggested
cuts are recorded in [Wave R6 in detail](#wave-r6-in-detail-2026-10-14).

**2026-10-14 process wave.** Commits `21dd69c`, `259d6d1`, and `7f28842` on the same branch, pushed, close blocker 7
and the F6 lane: coverage becomes one instrumented run with per-crate `--fail-under-lines` floors in the Makefile, the
dependency gate keeps only what no other tool enforces, the `quality/` Python chain shrinks from 1,414 lines to three
short scripts,
and the process artifacts (label system, checklist file, manual e2e workflow) go. The CI job count is unchanged and the
run on the final commit is green on all four jobs. Findings carry a `Resolved 2026-10-14 (implemented)` tag; see
[Wave R7 in detail](#wave-r7-in-detail-2026-10-14-process-and-ci).

**Implemented-code wave.** Commit `bba5e82` on the same branch removes the implemented surface no production caller
consumed and the logic that existed in two or three copies: the unread DTO fields (`SessionSummaryDto::active_run`, the
pending-turn projection budget), `ErrorRetryDto` → `bool retryable`, `CorrelationIdDto`, `DaemonReadinessDto`, the
turn-idempotency replay path, the stale-vs-malformed daemon-line split, five redundant client guards, four unreachable
SQLite result codes, the 528-line storage source scanner, `ConfigSourceKindDto`/`ConfigSourceDto`, the schema-version
equality apparatus, the four run-terminalization writers, the copied interruption boundary, the second cancellation
flag, the production test seam, the six-flag capability declaration, the second stream-order validator, the duplicated
grep body, the hand-maintained guidance argument table, the layout-cache key dependency, the second command parser,
the three stub browser tabs, the second consecutive-press arm, and the three fixture copies. Commit `411b163` lands the
documentation half in documents 00, 02, 03, 04, 06, 07, 08, 09, 11, 18, 19, and 23. Findings carry the
`Resolved 2026-10-14 (implemented)` tags this wave earns; see [Wave R12 in detail](#wave-r12-in-detail-implemented-code-tails).
The branch is ahead of `origin/cleanup/overengineering-removal`; `5e9c471` is the newest commit.

**Closure wave.** Commits `e1c27fe` (code), `a0366d9` (process), `e9cc6ef` (documents), and `1d62d23` (this record)
on `cleanup/overengineering-removal` close the remaining findings: the
per-turn identity machinery (`RunId` proposals, `IdempotencyKey`, `ConfigRevisionId`, the `configuration_revisions`
table, the replay path, and `turn.remove`) is deleted while every accepted turn still queues durably, a run row
carries its frozen credential-free `config_snapshot_json`, storage failures collapse to two codes, tool input schemas
are derived from the typed input DTOs, the `WIRE_VERSION` byte and the notices apparatus are gone, the README states
what the product is instead of restating its documents, and the document set carries no removal narration, ceremony
clauses, or evidence lines. `d43fc9a` refreshes the webpki patches past three advisories. Project/workspace identity is
deferred by decision (F2-5). See
[Work status](#work-status-2026-10-22) and [Closure wave in detail](#closure-wave-in-detail).

## Work status (2026-10-22)

| State | Findings | Evidence |
| --- | --- | --- |
| Closed | F1-1…F1-14; F2-1…F2-4, F2-6…F2-13; F3; F4; F5; F6; F7; C1; C2; C3 | Waves R6-R13 and the closure wave |
| Deferred by decision | F2-5 (project/workspace identity) | recorded under F2-5; `projects` and `workspace_roots` stay as they are |

## Lane map

| Lane | Surface | Findings |
| --- | --- | --- |
| F1 | 00, 01, 02, root README, AGENTS.md | 14 |
| F2 | 03, 04, 05 | 13 |
| F3 | 06, 07, 08, 09, 30 | 14 |
| F4 | 15, 18, 19, 20 | 16 |
| F5 | 21, 22, 23, 28 | 23 |
| F6 | 10, 11, 12, Makefile, quality/**, CI, `.factory/skills` | 15 |
| F7 | 25, 29, cross-document consistency, requirements weight | 13 |
| C1 | intention-proto, -storage, -transport, -client | 14 |
| C2 | intention-daemon, -engine, -test-support | 13 |
| C3 | intention-tools, -config, -providers, -tui | 14 |

## Cross-lane convergence (highest signal)

Items that several independent lanes reached; these are the strongest candidates for the next wave.

1. **The theme is daemon-owned protocol state** — F2-2 (HIGH), C3-1 (HIGH), C1-5. Two wire ops, a singleton
   table, four DTOs, a config key, preview/commit semantics, ~110 production references across 7 crates, and a
   layout-cache key, for a light/dark flag that never enters run selection. **Resolved 2026-10-13 (implemented):** the
   front end now resolves the theme itself (`--theme`, `COLORFGBG`, a local `tui.toml`, a bounded `OSC 11` probe, light
   default) and a selection applies in memory and rewrites the local file; the wire ops, the singleton table, the four
   DTOs, the daemon `[tui]` key, the echo check, and the layout-cache key dependency are deleted. Design:
   [Blocker 1 in detail](#blocker-1-in-detail-the-theme-without-daemon-owned-state).
2. **Tool-contract machinery larger than the shipped behavior** — F4-1, F4-2, F4-3 (HIGH), F3-8, C3-5: a 12-row
   effect profile for six tools and eight unshipped owners; per-run copies of every schema text; an 8-outcome
   taxonomy plus a 7-value lifecycle enum over a 3-value persisted status; validation of code-owned tool
   definitions that cannot fail. Collapse to the stored `Completed | Failed | Partial` status plus metadata
   codes, record the ordered `ToolId` list and one tool-layer revision (read `spec(id)`), and delete the
   profile and the definition-validation branches. **Resolved 2026-10-14 (docs + code):** the stored status plus
   metadata codes, the ordered `ToolId` list with one tool-layer revision, and the removed effect profile and
   definition-validation branches; `ToolResultOutcomeDto::Cancelled` is removed too. See F4-1…F4-4, F3-8, C3-5, C2-6.
3. **Provider configuration is heavier than one local user** — F5-6 (HIGH), F5-8 (HIGH), F5-9, F5-11, F7-12:
   five descriptor/profile/capability/driver DTO families with an intersection law and a compatibility matrix;
   cross-turn reasoning transfer with manifests, compatibility identity, an audit record, and a 4 MiB aggregate
   bound; catalog tombstones and removal history for a TOML file re-read at startup; provider configuration
   split across four documents. One immutable selection (`kind`, `model`, endpoint, effort, optional auth
   header name), v1 declares cross-run reasoning `Disabled`, the TOML is the only catalog state, and one
   document owns provider configuration. **Corrected 2026-10-13:** `Disabled` covers the typed transfer surface only;
   the same-run echo is required by real providers, and the previous-turn replay DeepSeek requires is an open
   `@todo(provider)`, so neither is part of this cut. **Resolved 2026-10-14 (docs):** architecture 22 owns the
   provider configuration and describes named TOML entries plus one global default; architecture 29 shrinks to the
   session-selection layer; the five DTO families, tombstones, user kinds, and the typed cross-run transfer surface
   are removed. The landed shape keeps a per-session selection among named entries rather than the single-selection
   sketch. See F5-6…F5-11, F7-4, F7-9…F7-12.
4. **Context/Skill/compaction record breadth** — F5-1, F5-2 (MED-HIGH), F5-3, F5-4 (MED-HIGH), F5-20, F5-23: a
   per-step context manifest and projection family; an audience/disclosure information-flow dimension with one
   actor; seven Skill origins and six lifecycle states around two real sources; 14 session record types with a
   paginated catalog and 19 codes; Skill and compaction owned twice with duplicate codes. **Resolved 2026-10-13 for the
   context pair:** the per-step manifest and projection and the whole audience/disclosure dimension are removed from
   architecture 21, leaving the admission-frozen source selection. Keep the frozen
   selection, the durable request window, card→revision→body, and one summary record; the Skill lattice, catalog, and
   two-owner duplication findings stand. **Resolved 2026-10-14 for the remaining parts (docs):** the lattice collapses
   to `Project | Personal` and `Enabled | Disabled` with a card→exact revision→body triple, the paginated catalog,
   supplements, 14 record types, and 19 codes are removed, and Skill and compaction each get one owner
   (F5-3…F5-5, F5-20, F5-23).
5. **Doc 25's control-plane cluster** — **Resolved 2026-10-13: the whole cluster is removed.**
   Controlled live reload, in-process credential rotation, provider health checks, pricing and budget policy, and raw-TOML
   editing are gone from doc 25 together with their commands, error codes, DTOs, and test obligations; discovery stays,
   and typed configuration editing survives with restart-only application and no second activation path. See F7-1…F7-6.
6. **Runtime state with no consumer** — F2-3 (HIGH), F2-4, F2-5, F2-7, C1-1, C1-2, C1-3, C2-1: the pending-turn
   subsystem (no front end reads `pending_turns()` or calls `remove_turn`); `ConfigRevisionId` plus a nine-code
   storage taxonomy; project/workspace identity whose conflict arm is unreachable; two cancel paths; unread
   `active_run`, pending-turn projection, and `ErrorRetryDto` fields; run terminalization written in four
   places. Keep the durable queue and the frozen run snapshot; delete the identity, retry, projection, and
   duplicate-writer machinery. **Closed 2026-10-22, except F2-5:** the unread projections, the terminalization
   writers, and the second cancel path are gone; the per-turn identity machinery is deleted while every accepted
   turn still queues; the run row carries its frozen credential-free configuration; storage failures are two codes.
   Project/workspace identity is deferred by decision and is recorded as such under F2-5.
7. **Coverage and per-crate bookkeeping** — F6-3 (MED-HIGH), F6-4, F6-5, F1-4: the suite runs twice (the whole
   workspace instrumented pass is informational), ~660 lines of Python reimplement the tool's own
   `--fail-under-lines`, every new crate needs a central tier edit and a policy edit before its first test.
   One `cargo llvm-cov` per crate with `--fail-under-lines`, a default tier, and a plain exclusion list.
8. **Plan-mode ceremony** — F3-3 (HIGH), F3-4, F3-7: hidden YAML frontmatter duplicating the plan row,
   a seven-state lifecycle with a feedback round trip, and durable retry events (`ProviderAttemptFailed`,
   `RetryScheduled`) that no code produces. Body-only plan markdown with the row as the source of truth,
   `drafting | approved | abandoned`, and no retry event names. **Resolved 2026-10-14 (docs):** body-only plans in
   the daemon-defined global `plans` directory with a model-provided file name (`invalid_plan_file_name`,
   `plan_name_conflict`), the three-state lifecycle, and no retry-event names.
9. **Future crates ahead of consumers** — F3-1, F3-2: `intention-vfr`/`intention-headroom` plus a six-stage
   pipeline, a non-import rule, two coverage tiers, and a CCR content store with retention/expiry duplicating
   the durable tool result. Modules inside `intention-tools` until a second consumer exists; `retrieve`
   resolves against the durable tool result. **Resolved 2026-10-14 (docs):** VFR and Headroom are modules of
   `intention-tools` with retention, rotation, and expiry deferred to `@todo(vfr)`/`@todo(headroom)` markers; the
   CCR store stays as the retrieval backing.
10. **Instruction model** — F3-10 (HIGH), F3-11, F3-12: eight contribution kinds × four scopes with per-source
    order/enable/revision and no editing surface, plus a projection record beyond the frozen text. `AGENTS.md`
    plus one user fragment list plus the two reserved contributions. **Resolved 2026-10-14 (docs):** the code-owned
    base text, one user fragment list, `AGENTS.md`, and the two reserved contributions; no kind/scope table,
    profile, or projection record.
11. **Process checks that re-validate other tools** — F6-1, F6-2, F6-7, F6-8, F6-11, F6-12, F6-13, F6-14,
    F6-15: `check_deny_policy.py` mirroring `deny.toml` (71 runs, 0 failures); `cargo outdated --exit-code 1`
    failing on upstream releases; `cargo audit` reading the advisory DB a second time; the notices byte-compare;
    a canonical-`$CARGO_HOME/bin` assertion; three encodings of the CI job/tool matrix plus a scheduled
    cache-pruner; the `timing.py` log apparatus; the two-stage planning pipeline with a 101-line checklist;
    twenty labels and numeric parallelism quotas. **Resolved 2026-10-14 (implemented), except the notices
    byte-compare (F6-13, not adopted) and the cache-cleanup cron:** `check_deny_policy.py`, `cargo audit`,
    `cargo outdated`, `outdated.toml`, `run_profiles.py`, `run_deps.py`, and `timing.py` are deleted; each gate
    runs its own `cargo` commands and passes its own pins to `check_tools.py`; the label section, the numeric
    quotas, `checklists.md`, and the manual `real-api-e2e` workflow are removed. See
    [Wave R7 in detail](#wave-r7-in-detail-2026-10-14-process-and-ci).
12. **Implemented-code test surface and duplicated state owners** — C1-6 (MED-HIGH), C1-7 (MED-HIGH), C1-13,
    C2-9, C2-10, C2-11: two appliers for one `SessionStreamState` (client copy plus TUI copy); a 528-line
    hand-written source scanner as the storage DTO guard where Clippy already denies the types; storage tests
    reaching into private SQL with duplicated fixtures; a production test seam inside the daemon host; four
    hand-rolled virtual clocks (two of them guessing production internals by `duration < 1s`); fixture builders
    in three copies. **Closed 2026-10-22:** one applier per stream state, no source scanner, one `cfg`-gated
    recorder instead of a test seam, one `VirtualTime` clock, and one fixture-builder home (`bba5e82`, `5e9c471`);
    the attempt holds one message list (`C2-4`).
13. **Documentation restatement tax** — F1-5 (MED-HIGH), F1-12, F2-1 (MED-HIGH), F2-13, F7-13: README
    triplicating the crate map/tiers/TUI and claiming deleted enforcement; `WorkspaceRoot` rules copied ten
    times; doc 03's 254-line landed terminal-UI changelog plus the Slice 1.5 sections duplicating live policy;
    23 "not a second" clauses, 13 ownership sections, 9 evidence lines, 189 cross-references. **Closed
    2026-10-22:** see [Closure wave in detail](#closure-wave-in-detail).

## Per-lane findings

Severity: HIGH / MED-HIGH / MEDIUM / LOW-MED. Anchors are `file:line` (docs under
`docs/intention-relay/architecture/` unless a path is given).

### F1 — Foundations and crate map

1. **Pre-collapse history occupies most of the crate map** — `01:17-110` (~95 of 248 lines name crates that are
   not workspace members) — MED-HIGH. **Resolved 2026-10-22:** document 01 is 158 lines and holds only the Rules,
   Crate map, Ownership decisions, Composition rules, Binaries, Enforcement, and Non-goals sections; no pre-collapse
   crate survives in it.
2. **Architectural test requirements mandate tests no mechanism needs and no test implements** — `01:226-238`;
   the enforcing checker was deleted in Part 1 — MED-HIGH. **Resolved 2026-10-22:** the architectural
   test-requirement table is gone; the Enforcement section names the mechanism that decides each rule
   (`01:140-154`).
3. **DTO-first scope contradicts the activated boundary model** — `00:18-21`, `02:5-11` vs `02:227-228`
   ("a DTO is not introduced between internal crates") — MED-HIGH. **Resolved 2026-10-22:** `00:18-24` and
   `02:8-13` state one model: exactly three boundaries force a serialized shape, and a DTO is not introduced
   between internal crates.
4. **New crate = crate-map edit + tier row + policy edit before any production code** — `AGENTS.md:64`,
   `01:116`, `12:196,208`, `quality/coverage.toml:2-10` — MED-HIGH. **Partly resolved 2026-10-14 (implemented):**
   the central tier table, its completeness rule, and the declared-versus-Cargo policy edit are gone; the crate-map
   entry and one `COVERAGE_FLOORS` row remain as the whole procedure (`12:135-136`). **Resolved 2026-10-22:** the
   surviving procedure is stated identically in `AGENTS.md:64`, `01:38`, and `12:125-136,217-218`.
5. **README restates crate map, coverage tiers, and the TUI, and claims deleted enforcement** —
   `README.md:59,64,74-79,83-122,167-208,356-376` — MED-HIGH. **Resolved 2026-10-22:** the README is 251 lines,
   links to the crate map, the coverage policy, and the Makefile instead of restating them, and every link resolves.
6. **"Changes are additive by default" re-introduces compatibility the repo forbids** — `02:183-187` vs
   `AGENTS.md:34-39` — MEDIUM. **Resolved 2026-10-22:** the clause is absent from document 02.
7. **Required v1 outcome 8 names a gate that no longer exists** — `00:111-112` vs `Makefile:105-108` — LOW-MED.
   **Resolved 2026-10-22:** outcome 8 names the applicable pinned-tooling, formatting, lint, test, documentation,
   coverage, and supply-chain and manifest gates (`00:122-123`).
8. **Scope-boundary diagram shows the pre-collapse layer stack** — `00:81-94` vs `01:148-169` — MEDIUM.
   **Resolved 2026-10-22:** document 00 carries no layer diagram; the crate map and its composition rules are the
   one layering statement.
9. **Semantic invariants mandated twice (decode and admission) for every DTO** — `02:133-137` — MEDIUM.
   **Resolved 2026-10-22:** document 02 states the rule once: the boundary that owns an invariant enforces it, and
   admission validates what its own decisions need rather than every DTO it receives (`02:133-139`).
10. **`CorrelationIdDto` is machinery with no producer and no display** — `02:85,88-95` — MEDIUM. **Resolved
    2026-10-14 (implemented):** the type is deleted.
11. **Per-ID declarations the code does not have** — `02:60` vs the shared parse helper — LOW-MED. **Resolved
    2026-10-22:** the identity list is the complete live set and one shared canonical-UUID path parses and decodes
    every member (`02:45-63`).
12. **`WorkspaceRoot` addressing rule restated in every tool document** — `00:39-44`, `01:123,178-180`,
    `README.md:34-36`, plus 6 more — MEDIUM. **Resolved 2026-10-22:** the rule is stated in the document that owns
    it, and the other documents reference the anchor instead of restating it.
13. **Label taxonomy and agent parallelism quotas** — `AGENTS.md:52-53,58,91-98` — MEDIUM / LOW-MED. **Resolved
    2026-10-14 (implemented):** the label section and the numeric quotas are removed, and dependabot applies no
    labels.
14. **Documentation-consistency obligation is unenforced but still binding** — `AGENTS.md:73` — LOW-MED. **Resolved
    2026-10-22:** `AGENTS.md` states each obligation with the mechanism that decides it and leaves the rest a review
    duty.

### F2 — Runtime core

1. **Doc 03's tail is a landed terminal-UI changelog** — `03:354-575`, plus `03:576-607` — MED-HIGH. **Resolved
   2026-10-22:** document 03 is 346 lines and its tail states the live process contract: exit statuses, restart
   semantics, required tests, and the open decisions.
2. **Theme as daemon-owned protocol state** — `03:55-62`, `04:120,126-127,133-136` — HIGH. **Resolved 2026-10-13
   (implemented):** the daemon, the wire, and storage carry no theme state; the front end resolves and stores it
   locally.
3. **Pending-turn subsystem has no adapter consumer** — `04:81-104,124-126,140-141,231,235`, `03:50` — HIGH.
   **Resolved 2026-10-22:** the durable queue stays and the per-turn identity machinery goes. Every turn accepted
   during an active run still queues, `consume_pending_user_turns` still joins them in insertion order, and the
   terminal front end still acknowledges a queued turn; the `RunId` proposal, `IdempotencyKey`,
   `turn_identity_conflict`, the replay path, and `turn.remove` are deleted, so a repeated command is an ordinary
   second turn. **Kept:** queue-every-prompt, by decision; the alternative "at most one queued turn" was rejected.
4. **Config-revision identity plus a nine-code storage taxonomy** — `04:118-119,181-191,233` — MEDIUM. **Resolved
   2026-10-22:** `ConfigRevisionId` and the `configuration_revisions` table are deleted; a run row carries its
   frozen credential-free `config_snapshot_json`, whose content is the revision, so nothing mints an identity and no
   cross-run dedupe exists. Storage failures are two codes: `storage_record_not_found` (NotFound, not retryable,
   only an absent expected row) and `storage_unavailable` (Unavailable, retryable, every other backend or decode
   failure).
5. **Project/workspace identity proposals with no possible violator** — `04:29-34,113,122`, `03:560-563` —
   MEDIUM. **Deferred by decision 2026-10-22:** the `projects` and `workspace_roots` tables, the per-session
   `ProjectId`/`WorkspaceId` proposals, and the one-root-one-binding resolution stay exactly as they are; the
   simplification is not part of this wave.
6. **`WIRE_VERSION` endpoint byte** — `03:30,193-198,325` — MEDIUM. **Resolved 2026-10-22:** the byte, the
   `WIRE_VERSION` constant, and the versioned endpoint name are deleted; the endpoint name is the fixed literal
   `intention-relay`, and the fail-closed typed decode is the only protection. Documents 03, 09, and 19 state it;
   document 11's roadmap sentence is updated in the same wave.
7. **Run registry with two cancel paths** — `04:193-212` — MEDIUM. **Resolved 2026-10-22:** the second
   cancellation flag and the copied interruption boundary are deleted, leaving one registry keyed by
   `(SessionId, RunId)`. **Kept:** the admission-window fallback. Admission commits the durable `Starting` row and
   only then inserts the registry entry, so a stop can arrive with no execution to signal; the host terminalizes
   that run durably `Interrupted` instead of doing nothing, and
   `a_stop_reaching_a_run_before_its_admission_terminalizes_it` covers it.
8. **Hidden production test seams** — `03:257-260,123-126,335-336` — MEDIUM. **Resolved 2026-10-22:** the
   production host carries one `cfg`-gated recorder instead of the trait pair; document 03 names no injection seam
   and no hidden facade.
9. **JSON Schema text as a hand-maintained second contract** — `05:30,42-46,229-230` — MEDIUM. **Resolved
   2026-10-22:** the typed input DTO is the one contract: `intention-proto` derives `JsonSchema` on it,
   `ToolId::input_schema()` renders the draft 2020-12 schema with model-irrelevant keywords stripped, and
   `ToolSpec::input_schema()` delegates to that renderer. The six hand-written `*_INPUT_SCHEMA` constants are
   deleted, and documents 05 and 15 state the derived shape.
10. **Requirements naming a deleted gate and unbuilt adapters** — `05:197,202-204`, `03:313,318-320` vs
    `11:248-255` — MEDIUM. **Resolved 2026-10-14 (docs):** `05` names the compiler entries that deny CWD fallback,
    and the adapter rows in `03` and `05` scope to the adapter that exists.
11. **Permission-boundary evidence asks for a second OS user** — `03:307` — LOW-MED. **Resolved 2026-10-22:** the
    row asks for the fact the product has: the endpoint file and its parent directory are owner-only on Unix.
12. **Doc 04's aggregate diagram carries entities with no tables** — `04:12-24` vs `04:122-123` — MEDIUM.
    **Resolved 2026-10-14 (docs):** `PERMISSION_REQUEST` and `QUESTION_REQUEST` are removed from the diagram, the
    identity invariant, and the restart sentence.
13. **Slice 1.5 "landed model" changelog sections duplicate the live policy** — `03:328-353`, `05:223-242` —
    MEDIUM. **Resolved 2026-10-22:** the changelog sections are deleted and both documents state the live policy
    once.

### F3 — Agent operation

1. **Two extension crates and a six-stage pipeline mandated before any consumer** — `06:3-6,42-51,71-81,
   93-116,141-145`, `11:301-302` — MED-HIGH. **Resolved 2026-10-14 (docs):** the crates are gone; the pipeline is
   the module call order inside `intention-tools`.
2. **CCR store with retention, expiry, and capacity duplicates the durable tool result** — `06:77-79,85-91,116,
   133` — MED-HIGH. **Partly resolved 2026-10-14 (docs):** retention, rotation, and expiry are deferred to `@todo`
   markers; the CCR store itself stays as the retrieval backing. **Resolved 2026-10-14 (docs):** the durable tool result is the record; the CCR store is a bounded read-through view with no retention, rotation, expiry, or capacity policy of its own, and the `@todo` deferrals for those are gone.
3. **Hidden YAML frontmatter duplicates the plan row** — `07:82-113,212-213` — HIGH. **Resolved 2026-10-14 (docs):**
   the plan file is body-only, and all metadata lives on the SQLite row.
4. **Seven-state lifecycle with a feedback round-trip** — `07:175-204,218` — MEDIUM. **Resolved 2026-10-14 (docs):**
   `drafting | approved | abandoned`; feedback is an ordinary user turn.
5. **Plan write-denial that does not deny what the model can do** — `07:116-136,156-157` — MEDIUM. **Resolved 2026-10-14 (docs):** Plan mode denies the project-mutating `write` and `edit` descriptors and states that it does not prevent project mutation, because `execute` stays available, advisory-guided, and audited. The claim that Plan prevents project mutation is gone from 00 and 07.
6. **FS/DB transactional reconciliation and never-reused plan numbers** — `07:74-80` — MEDIUM. **Resolved
   2026-10-14 (docs):** one ordering rule replaces the reconciliation mandate: the body file is written and flushed
   before the plan record commits, a failed write leaves no record, and a failure between the two leaves an orphan file
   that the next allocation of that number overwrites. The speculative never-reuse-after-deletion clause went with it;
   monotonic numbering per session stays and is what the tests assert.
7. **Retry choreography names durable events no one produces** — `08:239,245-254,267` — MEDIUM. **Resolved
   2026-10-14 (docs):** architecture 08 states the durable attempt count and the fixed 250 ms wait; no retry-event
   names exist.
8. **Unreachable validation codes for code-owned tool definitions** — `08:173-178` — MEDIUM. **Resolved 2026-10-14
   (docs + code):** the whole definition-validation surface and its three codes are deleted; construction is
   infallible.
9. **Bespoke reasoning-echo failure path** — `08:181-187`, `09:127` — LOW-MED. **Resolved 2026-10-14 (docs + code):**
   one code, `invalid_model_assistant_reasoning`, covers the tool-call identity rule and the text rule; the 512 KiB
   bound and the control-character rejection stay, and the provider contract tests assert the one code.
10. **Eight instruction kinds × four scopes with no editing surface** — `30:26-47,49-58,79-90,138-149` — HIGH.
    **Resolved 2026-10-14 (docs):** the base text, one user fragment list, `AGENTS.md`, and the `Mode`/`Vfr`
    contributions; no kinds, scopes, or per-source fields.
11. **Projection record shape beyond a frozen text** — `30:49-58,92-98` — LOW-MED. **Resolved 2026-10-14 (docs):** no
    projection record exists; the frozen text is stored as text.
12. **`Vfr` gets its own contribution revision while `Mode` is a constant** — `06:48-50` vs `07:138-152` —
    LOW-MED. **Resolved 2026-10-14 (docs):** no contribution revisions exist for either reserved contribution.
13. **Mode invariants restate global policy** — `07:154-173` — LOW-MED. **Resolved 2026-10-14 (docs):** the mode-invariant list keeps only Plan/Build-specific rules and cross-references 00, 09, and 12 for the global ones.
14. **Observability carries unbuilt surfaces** — `09:139-155,105` — LOW. **Resolved 2026-10-14 (docs):** the observability section lists only what the daemon can emit — state, durations, identity without credentials, usage, tool lifecycle, bounded sizes, and closed codes — and the diagnostic-bundle promise is gone.
### F4 — Capability packages A

1. **Twelve-row effect profile for six live tools, no consumer** — `15:67-82,58-61` — MEDIUM. **Resolved
   2026-10-14 (docs):** the profile and its 12-row table are deleted.
2. **Per-run selection copies contracts instead of naming them** — `15:139-144,63,149-151`, `18:65-69` — MEDIUM.
   **Resolved 2026-10-14 (docs):** the selection records the ordered `ToolId` list and one tool-layer revision read
   through `spec(id)`.
3. **Eight-outcome taxonomy plus seven-value lifecycle over a three-value stored status** — `15:268-296,
   298-308` — HIGH. **Resolved 2026-10-14 (docs + code):** the durable row stores `Completed | Failed | Partial`
   plus metadata codes; the taxonomy and lifecycle enums are deleted, and `ToolResultOutcomeDto::Cancelled` is
   removed (C2-6).
4. **Async `sub_agent` deferred-delivery path** — `15:120-128,271-277` — MEDIUM. **Resolved 2026-10-14 (docs):**
   the `Deferred` member is gone; the asynchronous acceptance is a `Completed` result carrying the child handle.
5. **Progress-deadline policy layered under the provider attempt timeout** — `15:342-360` — MEDIUM. **Resolved 2026-10-14 (docs):** one owner: 08's provider attempt idle window, its `provider_attempt_timed_out` failure, and at most two attempts. 15 keeps the pause rule, the deadline outcome, and the note that a content-only refinement needs its own specification.
6. **`effect_classification` on every MCP method with no source** — `18:59-60,90,150` — MEDIUM. **Resolved 2026-10-13:**
   removed with the MCP telemetry cut.
7. **MCP result projection fields beyond need** — `18:92-97,101-102` — LOW-MED. **Resolved 2026-10-13:** the progress
   and observation fields are removed.
8. **Two MCP failure codes with no reachable path** — `18:179-183` — LOW-MED. **Resolved 2026-10-13:** both codes are
   removed with the progress and interaction claims.
9. **Connection record splits the endpoint into a reference the user cannot see** — `18:38-44,49-55` — MEDIUM.
   **Resolved 2026-10-13:** one visible, validated, credential-free endpoint, matching the provider pattern.
10. **Per-method user approval as a standing gate** — `18:150,154-155` — MEDIUM. **Resolved 2026-10-13:** the gate is
    removed; a method the connection record exposes is available to the run's frozen selection.
11. **MCP states its rules twice; the bridge restates 15 in negation form** — `18:47-49/154-155`,
    `19:105-110/77-80` — MEDIUM. **Resolved 2026-10-14 (docs):** each rule has one owner and one statement; 18's restatement is replaced by a cross-reference, and 19's two negation-form restatements of 15 are cross-references.
12. **Bridge failure set carries an unreachable member** — `19:113-121` — LOW-MED. **Resolved 2026-10-14 (docs):** `daemon_tool_gateway_required` is deleted; the only described ingress is the spawned child's typed client, so no path produced it. `bridge_authority_unavailable` stays.
13. **Checkpoint metadata carries lineage, revision copies, verification, omission state** — `20:164-188` —
    MEDIUM (cheapest coherent version: one latest verified snapshot; the subsystem is kept). **Resolved 2026-10-14 (docs):** the metadata keeps seven fields; the source-execution, source-run, parent-checkpoint, and verification-status fields go with the rule that one verified checkpoint exists per session kernel selection and is replaced atomically.
14. **Per-cell import evidence and a versioned record for a constant path** — `20:53-57,149-156,256-257` —
    MEDIUM. **Resolved 2026-10-14 (docs):** the library is the constant `.ir/scripts`, so no `KernelScriptLibraryV1` value, no `script_library_reference` on the selection, and no per-cell import record exist; a failing import is the cell's own error, `kernel_script_library_unavailable` is gone, and the kernel DTO family drops to three.
15. **Kernel DTO family: four of nine names have no boundary** — `20:244-258` — MEDIUM. **Resolved 2026-10-14 (docs):** three names stay, the ones that cross a boundary (`KernelOutputChunkDto` and the host request/response pair); the cell's invocation and terminal result are architecture 15's, and the execution binding, the script library, the checkpoint payload, and attempt status are values in their records. With F4-14 removing the import-evidence DTO, the family went from nine names to three.
16. **Archaeology sentences about removed families and M3/M4 rows** — `15:93-94`, `18:9,14-15,29,125,140-142`,
    `19:13-14,127`, `20:9-10,14-15,239` — LOW-MED. **Resolved 2026-10-14 (docs):** the class is gone from 05, 08, 09, 15, 18, 19, 20, 21, 22, 30, and the roadmap. Each sentence was deleted when it only described the past, or rewritten as the forward invariant `No record carries state it did not record, and no current state reconstructs meaning a record never had`; milestone-relative wording ("During M4", "M1 defines", "M3/M4 own") became present-tense ownership.
### F5 — Capability packages B

1. **Per-step context manifest and projection records** — `21:269-295` — MED-HIGH. **Resolved 2026-10-13:** both
   records are removed; admission freezes one ordered, revision-pinned source selection, and each model step is built
   from it plus durable history.
2. **Audience and disclosure information-flow machinery** — `21:60,272-295,304,327` — MED-HIGH. **Resolved
   2026-10-13:** the audience dimension, the Skill disclosure policy, compaction's safe audience, and audience
   revocation are removed; one local user owns the machine.
3. **Skill model: origin/lifecycle lattice plus a five-DTO surface** — `21:97-151,166-197` — MED-HIGH. **Resolved
   2026-10-14 (docs):** two origins (`Project | Personal`, project wins), two states (`Enabled | Disabled`), and a
   card→exact revision→body triple; supplements and the frontmatter metadata fields are gone.
4. **Skill record families, catalog pagination, and nineteen codes** — `21:235-258` — MED-HIGH. **Resolved
   2026-10-14 (docs):** two session records (`SkillSelected`, `SkillBodyAttached`), six codes, and no catalog,
   pagination, or opaque token.
5. **The frozen Skill selection has two carriers** — `21:227-229` vs `28:87-89` — MEDIUM. **Resolved 2026-10-14
   (docs):** one `SkillSelectionV1` definition in 21, referenced by both run-selection shapes.
6. **Provider descriptor / profile / capability / driver framework** — `22:38-96,154-158,392-399,433-436` —
   HIGH. **Resolved 2026-10-14 (docs):** the framework is replaced by named provider entries.
7. **Driver option seam whose declaration vocabulary is empty** — `22:160-168` — MEDIUM. **Resolved 2026-10-14
   (docs):** the seam, and the Slice 4 option vocabulary, are removed.
8. **Cross-turn reasoning transfer** — `22:293,313-345,362-374` — HIGH. **Corrected 2026-10-13:** the same-run reasoning
   echo is load-bearing, not overhead. DeepSeek requires every previous turn's `reasoning_content` back once the request
   carries `tools` (HTTP 400 otherwise), Anthropic requires its signed thinking blocks within the current tool-use turn,
   and Gemini validates a thought signature on each current-turn function call; our generic-chat adapter already serves
   the narrowest of those. Only the typed surface is cuttable — the manifest, `compatibility_id`,
   `ReasoningHistoryBound`, and the aggregate 4 MiB bound. The provider-required previous-turn replay is recorded as
   `@todo(provider)` in docs 02/08/09/10/22 and in the provider, engine, and context-window comments. **Resolved
   2026-10-14 (docs):** the typed surface (manifest, `compatibility_id`, `ReasoningHistoryBound`, the aggregate bound,
   fragment categories, and summaries) is removed; cross-run transfer is `Disabled`.
9. **Catalog tombstones, removal history, startup acceptance audit** — `22:408-436,134-139` — MED-HIGH. **Resolved
   2026-10-14 (docs):** the TOML document is the only catalog state; tombstones, removal history, and the acceptance
   audit are gone.
10. **User-kind typed stateless reasoning dialect catalog** — `22:376-406` — MEDIUM. **Resolved 2026-10-14 (docs):**
    kinds are a closed code-owned set; the user-kind catalog is removed.
11. **Provider configuration owned by four documents** — `22:12-15,449,464-480` — MED-HIGH. **Resolved 2026-10-14
    (docs):** architecture 22 owns the provider configuration; 29 references it for the session selection, 09 keeps
    file mechanics, and 25 keeps discovery and editing.
12. **Fork preview and optimistic binding round trip** — `23:147-151,202-210` — MEDIUM. **Resolved 2026-10-14 (docs):** the preview carries no materialized text or messages and returns counts instead of reference lists; `expected_preview_binding` and `fork_preview_mismatch` go, and the fork transaction validates the one guard that remains, `expected_source_sequence`.
13. **Seventeen-field fork snapshot, copied into the preview** — `23:191-204` — MEDIUM. **Resolved 2026-10-14
    (docs):** `inherited_reasoning_history_references` went with the typed-transfer cut, and the snapshot now keeps ten
    fields — identity, lineage, frozen context, inherited defaults, workspace state, schema versions — while the preview
    stores no copies of the six provenance reference lists, which stay on the source session's records.
14. **Presentation commands carry operation-id idempotency** — `23:158-163` — MEDIUM. **Resolved 2026-10-14 (docs):** `SessionPresentationOperationId`, the equal-repeat rule, and `session_presentation_operation_conflict` are gone; the sequence guard and `session_changed` remain, with the `changed = false` no-op and the archive idle check.
15. **`StartForkRunCommandDto` as a second run-start path** — `23:153-156` — MEDIUM. **Resolved 2026-10-14 (docs):** the command and its paragraph are deleted, so a fork child's runs start from ordinary user turns. Same-session regeneration is a separate mechanism in the same document and stays.
16. **Paged conversation tree** — `23:165-170,227` — MED-LOW. **Resolved 2026-10-14 (docs):** the token, the page size, the 64-summary bound, and `invalid_conversation_tree_page` go; a read returns one parent's immediate children in stable creation order, the registry row keeps only the title bound, and an over-size read is answered by the transport envelope's typed failure.
17. **Fork record families and a derived tree identity** — `23:34-37,91-92,236-239` — MED-LOW. **Resolved 2026-10-14 (docs):** a tree is identified by its root `SessionId`; `ConversationTreeId`, the UUIDv5 derivation, the conversation-tree record family, and the `ConversationTreeCreated`/`ConversationBranchLinked` kinds are gone, lineage lives on the child's `ParentSessionId`, and storage adds only the base snapshot and the fork-operation idempotency record.
18. **Duplicated codec-removal paragraph** — `23:78-84` vs `23:184-189` — LOW. **Resolved 2026-10-14 (docs):** the codec paragraph appears once, in the field tables; the `Disabled` reasoning-transfer rule stays.
19. **Goal bounds table contradicts the flat-goal model** — `28:171-182` vs `28:56` — MED-HIGH. **Reviewed and kept
    2026-10-14 (no change):** the parent link stays display-only and the tree's depth and child counts stay registry
    rows in 09; the flat model keeps its meaning because the link grants no inheritance, readiness, or requirement.
20. **Skill and compaction failure codes defined by two owners** — `28:197-203` vs `21:253-258` — MEDIUM.
    **Resolved 2026-10-14 (docs):** 21 owns the Skill and compaction failures; the duplicated codes are deleted
    from 28.
21. **`RefinementDraftDto` proposal domain** — `28:119-130` — MEDIUM. **Resolved 2026-10-14 (docs):** a proposal is one pending `ask_user` question naming the target record, its exact base revision, and the change as text; one proposal per run. The draft record family, its field enumeration, `refinement_draft_conflict`, and `refinement_draft_too_large` are gone, a stale base is `goal_revision_conflict`, and the 09 registry row reads one pending proposal per run.
22. **Second run kind for verification** — `28:78-81,100-108` — MEDIUM. **Resolved 2026-10-14 (docs):** `VerificationOnly` and the selection's `run kind` field go; evidence is collected by an ordinary leading-goal run or recorded by the accepted user declaration, and the type-level guarantee that a verification run cannot alter goal state is deliberately given up.
23. **Compaction/summary built and owned twice** — `28:132-146` vs `21:301-307` — MED-HIGH. **Resolved 2026-10-14
    (docs):** 21 owns compaction; 28 references it.

### F6 — Delivery process and quality gates

1. **Policy checker re-asserts what cargo-deny already enforces** — `quality/check_deny_policy.py:36-84` —
   LOW-MED (71 recorded runs, 0 failures). **Resolved 2026-10-14 (implemented):** the file is deleted; the
   reviewed five-entry `advisories.ignore` set becomes a stated review responsibility in `deny.toml`.
2. **`cargo outdated --exit-code 1` fails on upstream releases; `cargo audit` duplicates `cargo deny`** —
   `quality/run_deps.py:46-55`, `quality/outdated.toml` — MED. **Resolved 2026-10-14 (implemented):** both tools
   and `outdated.toml` are deleted; freshness is dependabot's work, and `cargo deny` is the single advisory and
   license authority.
3. **Coverage runs the whole instrumented suite twice and reimplements thresholding in 660 lines of Python** —
   `quality/run_coverage.py:234-293`, `quality/check_coverage.py:249-363` — MED-HIGH. **Resolved 2026-10-14
   (implemented):** one `cargo llvm-cov --no-report` workspace run, then `cargo llvm-cov report --package
   --fail-under-lines` per measured crate; both scripts and the informational second pass are gone. The
   instrumented suite went from ~398 s (9 per-crate passes plus the aggregate) to one ~205 s pass.
4. **Exclusion metadata plus a guard for a mechanism that does not exist** — `quality/coverage.toml:31-36`,
   `quality/check_coverage.py:171-214,278-280` — LOW-MED. **Resolved 2026-10-14 (implemented):** the exclusion
   mechanism is deleted rather than reimplemented; the daemon's `main.rs` sits inside its own 60% floor
   (`intention-daemon` at 86.57%).
5. **Every new crate must edit the central tier table before its first test** — `quality/run_coverage.py:159-169`,
   `quality/coverage.toml:7-11`, `12:108-110` — MED. **Resolved 2026-10-14 (implemented):** `coverage.toml` and
   the untiered-crate failure are gone; `COVERAGE_FLOORS` in the Makefile is the whole policy, and a crate absent
   from it is simply not measured. The cost is that a new crate can silently go unmeasured.
6. **Doc 10 prescribes two deleted architecture checks inside a ten-layer/twelve-rule tax** — `10:32-45,47-64` —
   MED-HIGH. **Resolved 2026-10-14 (docs):** the "Small-crate structure" and "Crate accountability" rules, whose
   enforcing checker was deleted, are removed; the layer table and the remaining rules stay (deep rewrite out of
   scope).
7. **Recorded live-provider evidence obligation plus a dedicated workflow** — `10:193-202`, `11:333-345`,
   `.github/workflows/real-api-e2e.yml` — MED. **Resolved 2026-10-14 (implemented):** the workflow, its repository
   secret, and its 150-minute job are deleted; `make e2e-real-api` remains the documented opt-in route, and the
   evidence obligation now names the command and machine instead of a workflow run URL.
8. **Doc 12 keeps an unverifiable gate-failure table and a slice-evidence section** — `12:178-204,218-232` —
   LOW-MED. **Resolved 2026-10-14 (docs):** the gate-failure table and the Slice 1.5 evidence section are deleted.
9. **Roadmap keeps closeout-style narration and two stale CI facts** — `11:16-22,89-136,180-183,229-246,134-135`
   — MED. **Partly resolved 2026-10-14 (docs):** the two stale CI facts are corrected (Slice 2's "all six CI jobs"
   and the `slice2` quality-phase sentence); the closeout narration of the closed M5+ slices stands. **Resolved
   2026-10-22:** the slice list states each slice's scope and status as facts, and the milestone sections carry the
   deliverable, tests-first, exit-criterion, failure, and known-deferral rows.
10. **The same gate sentence is restated in every milestone's exit criteria** — `11:209,397,454`, `11:270-274,
    297-302` — LOW-MED. **Resolved 2026-10-14 (docs):** the restated `make quick`/`make verify`/CI clause is
    removed from the three milestone exit-criteria lists and stays once in "Quality rule for every milestone".
11. **Four-job CI with a mirror-image scope map, a PATH-shape assertion, and a cron repeating the merge cleanup**
    — `.github/workflows/quality.yml:19-79`, `quality/check_tools.py:19-24,49-88,158-167`,
    `.github/workflows/cache-cleanup.yml` — LOW-MED. **Resolved for the scope map and the PATH assertion
    2026-10-14 (implemented):** `CI_TOOLS_SCOPE` and the per-scope tables are gone; each gate passes its own pins
    to `check_tools.py`, which no longer asserts where `cargo` resolves from. The four-job layout is deliberate
    and stays; the cache-cleanup cron stays by decision.
12. **Timing/report apparatus no gate reads, plus a second timing wrapper** — `quality/timing.py`, `Makefile:10-13`
    — LOW-MED. **Resolved 2026-10-14 (implemented):** `timing.py` and the `quality-run.events.jsonl` log
    (9,123 recorded lines with no reader) are deleted; the Makefile's `TIMED` wrapper is the only timing surface.
13. **Notices: a 5,976-line artifact byte-compared with a cache-dependent fallback parser** —
    `quality/generate_third_party_notices.py`, `THIRD_PARTY_NOTICES.md`, `12:151-163` — LOW-MED. **Resolved
    2026-10-22 (cut adopted):** the artifact, its generator, its template, `NOTICES_PINS`, the Makefile targets, and
    the CI step are deleted. Nothing publishes crates (`publish = false`), `deny.toml` is the only license and
    advisory policy, and document 12 states that.
14. **Two-stage planning pipeline plus a 101-line checklist set for one decision-maker** —
    `.factory/skills/plan-intention-relay-milestone/SKILL.md:87-99,108-138,140-156,193-195`, `checklists.md` —
    MED. **Resolved 2026-10-14 (implemented):** `checklists.md` is deleted and its live rules are folded into
    SKILL.md's self-check section; the two-stage structure stays.
15. **Twenty labels and numeric agent parallelism quotas** — `AGENTS.md:89-98`, `.github/dependabot.yml` —
    LOW-MED. **Resolved 2026-10-14 (implemented):** the label section and the numeric quotas are removed, and
    dependabot stops applying labels.

### F7 — Consistency, requirements weight, control plane

1. **Controlled live reload is a second activation path** — `25:22-35` — HIGH. **Resolved 2026-10-13 (docs):**
   controlled live reload is outside the accepted directions, so no command, contract, transaction, DTO, facade, or
   test obligation exists for it.
2. **Credential rotation command with frozen-meaning preflight and driver rebuild** — `25:22-35` — HIGH. **Resolved
   2026-10-13 (docs):** credential material reaches the daemon only through the startup loading boundary, so no
   rotation command and no in-process replacement exists.
3. **Provider health-check service and evidence DTO** — `25:22-35` — MED-HIGH. **Resolved 2026-10-13 (docs):** no
   health-check service exists.
4. **Model discovery as an independently identified external attempt** — `25:37-46` — MEDIUM. **Kept:** discovery
   survives the removal, so this open simplification stands; it was not adopted. **Resolved 2026-10-14 (docs):**
   discovery is an ordinary non-authorizing read; the attempt-evidence obligation is removed.
5. **Pricing and budget policy section** — `25:22-35` — MEDIUM. **Resolved 2026-10-13 (docs):** no pricing or budget
   surface exists.
6. **Raw-TOML and typed configuration editing with server-side credential re-injection** — `25:22-35` — HIGH.
   **Resolved 2026-10-13 (docs):** typed editing survives without the raw-TOML surface and applies at the next daemon
   start only (`25:48-64`).
7. **Presentation-surface ownership handoff to a non-goal** — `25:66-69` vs `29:124-125` — LOW-MED (unchanged, live).
   **Resolved 2026-10-14 (docs):** 25 owns the editing direction only, 29 owns the list and selection protocol, and no
   presentation surface is designed.
8. **Doc 25's scaffolding prose outweighs its direction** — `25:1-9,17-20,77-85` — MEDIUM, **partly closed** by the
   2026-10-13 rewrite: the removed directions carry no scaffolding any more, while the ownership and non-authority prose
   stands as written. **Further reduced 2026-10-14:** the discovery and presentation prose now carries only the
   direction. **Resolved 2026-10-22:** the document states the two accepted directions as facts in 83 lines; no removal
   narration, no duplicated ownership block, and no evidence line remains.
9. **Session default command with optimistic CAS, operation ID, and an unbound event** — `29:30-40` — MED-HIGH.
   **Resolved 2026-10-14 (docs):** the command is plain last-write-wins; no expected revision, operation ID, event,
   or `changed = false` semantics.
10. **Per-turn and fork overrides on three wire commands** — `29:41-44,71-72` — MEDIUM. **Resolved 2026-10-14
    (docs):** `SendUserTurn`, `ForkSession`, and `StartForkRun` carry no provider fields.
11. **Catalog read surface is heavier than the catalog** — `29:76-92` — MEDIUM. **Resolved 2026-10-14 (docs):** a
    bounded list with no pagination, opaque token, or catalog revision.
12. **Duplicated rule blocks across 22/29/09/25/11** — `22:170-173` vs `29:103-106`, `22:464-472` vs
    `29:126-130`, `22:410-414` vs `29:110-112`, `09:40-45,55-59` vs `25:22-35` (the last pair is resolved 2026-10-13:
    the 25 side is reduced to the removal record) — MED-HIGH. **Resolved 2026-10-14 (docs):** 22 owns the provider
    rules; 29, 09, and 25 reference them, and the duplicated blocks are gone.
13. **Repeated per-document ceremony (whole-set convention)** — 23 "not a second" clauses, 13 ownership
    sections, 9 evidence lines, 189 cross-references — MEDIUM. **Resolved 2026-10-22:** no "not a second" clause and
    no evidence line remains in the document set; each ownership section states only who owns what, the three
    "Dependencies, non-goals, and evidence" headings whose evidence lines were deleted become "Dependencies and
    non-goals", and the remaining cross-references each name the owner of the rule they cite.

### C1 — Implemented wire, storage, transport, client

1. **Session summaries carry an `active_run` no caller reads** — `crates/intention-proto/src/protocol.rs:418-419,
   503-505`, producer `crates/intention-storage/src/sqlite.rs:1493-1507` — MED-HIGH. **Resolved 2026-10-14 (implemented):** the field, its producer, and its coherence rule are deleted; the run is carried by `SessionProjectionDto::active_run`.
2. **Pending-turn projection, budget, and omitted count have no production reader** —
   `crates/intention-proto/src/projection.rs:96-99,152-233`, `crates/intention-storage/src/sqlite.rs:1924-1964` —
   MED-HIGH. **Resolved 2026-10-14 (implemented):** the projection fields, the byte budget, and the omitted count are deleted. `PendingTurnProjectionDto` stays because the turn queue builds it.
3. **`ErrorRetryDto` is chosen at ~40 sites and read by none** — `crates/intention-proto/src/lib.rs:283-294`,
   client `crates/intention-client/src/lib.rs:1161-1163` — MED-HIGH. **Resolved 2026-10-14 (implemented):** one real reader distinguished only `Delayed`, so the enum is replaced by `bool retryable` end to end and the engine reads that.
4. **`CorrelationIdDto` has no producer** — `crates/intention-proto/src/lib.rs:296-330,388-389` — MEDIUM. **Resolved 2026-10-14 (implemented):** the type, the field, the accessor, and the engine's forwarding are deleted.
5. **A two-value theme is modeled as four single-field DTOs plus an echo check** —
   `crates/intention-proto/src/protocol.rs:59-159`, client `crates/intention-client/src/lib.rs:411-435` — MEDIUM.
   **Resolved 2026-10-13 (implemented):** the DTOs, the two wire operations, and the echo check are deleted, and the
   protocol request table is down to eight operations.
6. **Frames are applied twice to two copies of one derived state** — `crates/intention-client/src/lib.rs:556-607`
   vs `crates/intention-tui/src/app/run.rs:126,176` — MED-HIGH. **Resolved 2026-10-22 (`5e9c471`):** the
   subscription returns the frame beside the `FrameUpdate` it produced and the one `SessionStreamState` it applied the
   frame to; the TUI takes that state (`Action::FrameReceived { frame, update, state }`) instead of replaying the frame
   into a copy of its own, so one derivation and one transcript version cross the boundary.
7. **A 528-line source-text scanner is the storage DTO guard** —
   `crates/intention-storage/tests/opaque_json_guard.rs` — MED-HIGH. **Resolved 2026-10-14 (implemented):** the scanner is deleted. Its type rule is the `clippy.toml` `disallowed_types` entry the compiler already enforces; its field-name heuristic had no compile-time analog and guarded no field the crate has.
8. **SQLite result-code ladder is finer than any reader** — `crates/intention-storage/src/sqlite.rs:2159-2257` —
   MEDIUM. **Resolved 2026-10-14 (implemented):** four arms no statement can produce (`NotFound`, `FileLockingProtocolFailed`, `NoLargeFileSupport`, `AuthorizationForStatementDenied`) and a duplicate `not_found_or_storage` wrapper are deleted; every code, category, and retry value a caller observes is unchanged.
9. **Turn idempotency replay has no producer of a repeated key** — `crates/intention-proto/src/lib.rs:131-134`,
   `crates/intention-storage/src/sqlite.rs:148-150,574-631` — MEDIUM. **Resolved 2026-10-14 (implemented):** both producers derive the proposed run identity from the key, so a repeated key already fails `turn_identity_conflict`; the unreachable `replayed_turn` path is deleted.
10. **Stale-vs-malformed daemon-line split with no reader** — `crates/intention-proto/src/protocol.rs:959-992` —
    LOW-MED. **Resolved 2026-10-14 (implemented):** `stale_daemon_protocol` had no production branch, so both cases are the one fail-closed `invalid_local_protocol_response`.
11. **Every client call re-validates entity identity the correlation already proves** —
    `crates/intention-client/src/lib.rs:274,300,330,356,432` — LOW-MED. **Resolved 2026-10-14 (implemented):** five redundant guards are deleted. `SessionStreamState::ensure_scope` stays, because stream frames are uncorrelated and the session id is the only proof a frame belongs on the connection.
12. **Dead public accessors and per-enum duplicate wire codecs** — `crates/intention-proto/src/lib.rs:199-209,
    625-634`, `protocol.rs:196-205`, eight enums with hand-written `as_str`/`parse` — LOW-MED. **Resolved 2026-10-14 (implemented):** a workspace-wide caller scan found no dead accessor left; `SchemaVersionDto` and its accessors are deleted, and the two hand-written codecs that remain accept a wire spelling serde cannot express (`ToolIdentityDto`'s undeclared-name round trip and `ProviderErrorDto`'s non-blank code rule).
13. **Storage tests reach around the trait into raw SQL** — `crates/intention-storage/tests/m4_run_safe_config.rs:118-170,
    193-208`, `sqlite_contracts.rs:2064-2245` — LOW-MED. **Resolved 2026-10-14 (implemented):** the recovery test reads `RunProjectionDto` instead of `SELECT status, finished_at`. The sites whose subject is the schema, the stamp, a malformed stored value, or a broken backend stay raw, because no trait call can produce them.
14. **`DaemonReadinessDto` is a one-variant enum** — `crates/intention-proto/src/protocol.rs:21-53` — LOW. **Resolved 2026-10-14 (implemented):** the enum becomes an empty `DaemonHealthDto` with `ready()`.
### C2 — Implemented daemon and engine

1. **Run terminalization written in four places** — `crates/intention-daemon/src/lib.rs:385-400,445-467`,
   `crates/intention-engine/src/runtime.rs:69-94` — MED-HIGH. **Resolved 2026-10-14 (implemented):** `terminalize_run` owns the commit and `terminalize_active_run` owns the read-and-guard; the four sites route through them, keeping each path's status, finish time, and notice.
2. **Status frames force a second durable read of state the committer already held** —
   `crates/intention-engine/src/runtime.rs:1235-1255`, `crates/intention-daemon/src/lib.rs:536-549` — MEDIUM.
   **Resolved 2026-10-22 (`5e9c471`):** `SessionCommitDto::Status` carries the committed `RunProjectionDto`, so
   the terminal publication builds its frame from the value the commit returned; the bounded publication-retry worker
   and its two constants are deleted.
3. **Interruption/notice boundary logic copied three times** — `crates/intention-engine/src/runtime.rs:677-692,
   833-848,1132-1165` — MED-HIGH. **Resolved 2026-10-14 (implemented):** one `settle_interrupt` computes the boundary for all four call sites, and the codes and notices are unchanged.
4. **`messages` and `extra_messages` hold the same values twice** — `runtime.rs:575-576,645-646,940-942` —
   MEDIUM. **Resolved 2026-10-22:** one `messages: Vec<ModelMessageDto>` is built once and passed `&mut` through the
   attempt; the second list and the merge at request build are deleted.
5. **Two cancellation flags for one always-identical state** — `runtime.rs:245-345` — MEDIUM. **Resolved 2026-10-14 (implemented):** the run holds one cancellation flag. `ModelCancellationSignal::shared_flag` exposes it and `CancellationSignal::from_shared` observes it, so the model stream and the blocking tool path cannot disagree.
6. **`ToolResultOutcomeDto::Cancelled` is unreachable and its handler is a duplicate** — `runtime.rs:147-151,
   189-193,793-816` — MEDIUM. **Resolved 2026-10-14 (implemented):** the variant, its constructor, and its handler
   are deleted.
7. **Admission re-validates the same selection three times** — `runtime.rs:541-557,1580-1597`,
   `crates/intention-engine/src/lib.rs:640-651` — MEDIUM. **Resolved 2026-10-14 (implemented):** the self-comparison of the model against the value it was just built from is deleted; the run-identity check, the public input's independent request/config check, and the persisted-selection check stay because each compares independently supplied values.
8. **Provider-slot type is cfg-switched, producing four impls for one slot** —
   `crates/intention-daemon/src/composition.rs:125-212` — MEDIUM. **Resolved 2026-10-22 (`5e9c471`):** the
   `SelectedProvider` alias, the `InjectedProvider` wrapper, and their three impls collapse into one
   `ConfiguredProvider` enum whose `Injected` variant is `cfg`-gated, so a production build has two variants and one
   `driver()`.
9. **Daemon test seam lives inside the production host** — `crates/intention-daemon/src/lib.rs:89-209,1582-1668`
   — MEDIUM. **Resolved 2026-10-14 (implemented):** the recorder trait pair, its no-op implementation, and the host alias are deleted; one `cfg(any(test, feature = "test-support"))` recorder replaces them and a production build carries no fixture state.
10. **Four hand-rolled virtual clocks, with a "classify the delay by its scale" rule** —
    `crates/intention-engine/tests/common/mod.rs:829-931`, `m5_tool_loop.rs:112-170` — MEDIUM. **Resolved
    2026-10-22 (`5e9c471`):** one `VirtualTime` clock replaces `ImmediateTime`, `ManualTime`, `RetryDelayTime`, and
    `RoundSelectTime`; a test declares a held delay with `holding(...)` and observes the arm with `signalling(...)`,
    so no clock classifies a delay by its magnitude.
11. **Fixture configuration/session builders exist in three near-identical copies** —
    `crates/intention-test-support/src/lib.rs:41-87`, `crates/intention-daemon/src/composition.rs:748-777`,
    `crates/intention-daemon/src/lib.rs:1711-1740` — MEDIUM. **Resolved 2026-10-22 (`5e9c471`):** the two daemon
    copies are deleted; `fixture_snapshot()` and `fixture_session_command(session_id, label)` live in
    `intention-test-support` and both daemon suites call them.
12. **Snapshot byte budget is computed by re-encoding every row** — `crates/intention-daemon/src/composition.rs:572-650`
    — LOW-MED. **Resolved 2026-10-22 (`5e9c471`):** one `CountingWriter::encoded_bytes` helper replaces the two
    byte-count functions, and the trim loop walks newest-first so only the served rows and the first rejected one are
    encoded.
13. **Daemon-binary discovery has a five-level ancestor search plus fallback** —
    `crates/intention-test-support/src/daemon_process.rs:20-77,287-360` — LOW-MED. **Resolved 2026-10-22
    (`5e9c471`):** the search and fallback collapse to the profile directory one level above the running test
    executable's own directory; the crate that owns the daemon binary uses `CARGO_BIN_EXE_intention-daemon` directly.

### C3 — Implemented tools, config, providers, TUI

1. **The theme is daemon-owned protocol state** — `crates/intention-config/src/lib.rs:691-717`,
   `crates/intention-proto/src/protocol.rs:61-160`, `crates/intention-storage/src/sqlite.rs:170-172`, ~110
   production references, 31 theme assertions in `crates/intention-tui/src/app/tests.rs` — HIGH. **Resolved
   2026-10-13 (implemented):** the theme lives in `crates/intention-tui/src/tui/theme.rs`; config, proto, storage, and
   the daemon carry no theme surface, the storage schema is seven tables at stamp 6, and the app tests assert the local
   precedence instead of the wire round trip.
2. **Config source-kind and path DTOs for one fixed location** — `crates/intention-config/src/lib.rs:46-83,86-139,
   141-198,337-343,477-481` — MEDIUM. **Resolved 2026-10-14 (implemented):** `ConfigSourceKindDto` and `ConfigSourceDto` are deleted; one validated `ConfigPathDto` plus the resolver carry the override and the one platform location, and the projection no longer names a source kind.
3. **Schema-version equality machinery** — `crates/intention-config/src/lib.rs:16-44,487-549` — LOW-MED. **Resolved 2026-10-14 (implemented):** one `CURRENT_SCHEMA_VERSION` constant is checked once in the parser; the equality apparatus, the snapshot-side version, and the persistence gate are deleted, and any other version fails closed with `unsupported_config_schema_version`.
4. **Capability declaration with one meaningful value** — `crates/intention-providers/src/model.rs:264-356` —
   MEDIUM. **Resolved 2026-10-14 (implemented):** the declaration carries one requirement, streamed text with tool calls; four constant flags and six getters are deleted, and `unsupported_model_capability` stays.
5. **Validation of code-owned tool definitions** — `crates/intention-providers/src/model.rs:358-448` — MEDIUM.
   **Resolved 2026-10-14 (implemented):** `ModelToolDefinitionDto` construction is infallible; the three codes and
   their tests are deleted.
6. **Reasoning-echo attachments and their validator** — `crates/intention-providers/src/model.rs:203-262,
   456-459,577-602`, consumer `generic_chat.rs:456-535` — MEDIUM. **Resolved 2026-10-14 (implemented):** the attachment and its 512 KiB bound stay because the generic driver must replay earlier reasoning once a request carries tools; the empty/duplicate identity rejection is deleted because every producer passes the round's own call ids, so it could never fire.
7. **A second stream-order validator** — `crates/intention-providers/src/model.rs:704-765` vs `stream.rs:87-140`
   — LOW-MED. **Resolved 2026-10-14 (implemented):** `stream.rs` is the only order enforcer, since it builds every adapter stream and cannot emit an out-of-order fact; `ModelStreamLifecycleDto` and `invalid_model_stream_order` are deleted, with one test pinning the surviving guarantee.
8. **One search path written twice** — `crates/intention-tools/src/lib.rs:1863-1956` vs `:1958-2116` — MEDIUM. **Resolved 2026-10-14 (implemented):** the inline explicit-path branch was a hand-inlined `GrepScope::File`; `grep_tool` now resolves the scope and calls the one `grep_scoped` body, −71 lines.
9. **A third hand-maintained copy of the tool argument lists** — `crates/intention-tools/src/guidance.rs:95-113`
   — MEDIUM. **Resolved 2026-10-14 (implemented):** the argument table is deleted; guidance renders the tool's own `input_schema()`, which is the documented argument contract.
10. **Transcript layout cache with five-key invalidation** — `crates/intention-tui/src/tui/layout.rs:711-755`,
    `tui/mod.rs:536-564` — MEDIUM. **Resolved 2026-10-14 (implemented):** the tool-result count is replaced by the real question — whether an appended row is a tool result, which re-pairs an earlier call block — so the key is width, theme, epoch, and reasoning epoch, and the append fast path stays.
11. **Two command vocabularies** — `crates/intention-tui/src/app/commands.rs` vs `repl.rs:264-301` — MEDIUM. **Resolved 2026-10-14 (implemented):** one `COMMANDS` registry feeds the fullscreen submission path, the hint band, every notice, and the line REPL's parse; the REPL's private parser and its own command variants are deleted.
12. **Screens built for facts the core does not have** — `crates/intention-tui/src/app/browser.rs:14-48,116-123`,
    `tui/panes/welcome.rs:12-14`, `tui/panes/input.rs:61` — MEDIUM. **Resolved 2026-10-14 (implemented):** the three stub browser tabs, their actions, notices, and key bindings, the input mode placeholder, and the welcome stub blocks are deleted, along with the palette role that only painted them; a surface with no fact renders nothing.
13. **Two consecutive-press arm mechanisms** — `crates/intention-tui/src/app/ctrl_c.rs:36-73` vs `app/escape.rs:36-68`
    — LOW-MED. **Resolved 2026-10-14 (implemented):** one `Arm` mechanism and one `armed` field replace `CtrlCArm` and `escape_arm`; both keys keep their exact behavior.
14. **Three copies of the same test fixtures** — `crates/intention-tui/src/app/tests.rs:28-507`,
    `tests/tui_render.rs:76-537`, `src/tui/panes/transcript.rs:600-870` — MEDIUM. **Resolved 2026-10-14 (implemented):** `intention-test-support::fixtures` holds the builders and identity constants; the three sites lose 738 lines and every assertion is byte-identical.
## New contradictions and stale text

| # | Contradiction | Anchors |
| --- | --- | --- |
| D1 | ~~DTO scope: "every crate boundary exchanges DTOs" vs "a DTO is not introduced between internal crates"~~ — resolved 2026-10-14: 00 principle 2 and 02's opening state the three physical boundaries, and internal crates exchange domain types | `00:18-21`, `02:5-11` vs `02:227-228` |
| D2 | ~~Additive-by-default compatibility vs the no-compatibility policy~~ — resolved 2026-10-14: 02 states one live version that evolves in place with no compatibility affordance, and an optional field is a property of that version | `02:183-187` vs `AGENTS.md:34-39` |
| D3 | ~~v1 outcome names an architecture gate that no longer exists~~ — resolved 2026-10-14: 00's v1 outcome lists the gates `make verify` runs and names no architecture gate | `00:111-112` vs `Makefile:105-108` |
| D4 | ~~README claims machine-readable enforcement of crate classification and test targets~~ — resolved 2026-10-14: the README row names the policy files that exist (`quality/tools.toml`, `deny.toml`, `clippy.toml`, `quality/about.toml`) and the manifest-and-rustc edge enforcement | `README.md:59` vs the deleted `architecture.toml` |
| D5 | ~~Two architecture pictures (pre-collapse layers vs the live nine-crate graph)~~ — resolved 2026-10-14: 00 states the process boundary in prose and points at 01 for the live nine-crate graph | `00:81-94` vs `01:148-169` |
| D6 | ~~`openrouter-rs` pin~~ — resolved 2026-10-14 (R8, `54c22f8`): 08 and the manifest both pin 0.19.0 | `08:94` (0.18.0) vs `crates/intention-providers/Cargo.toml:18` (=0.19.0) |
| D7 | ~~Adapter classification of an incomplete stream~~ — resolved 2026-10-14: 08 states that the adapter classifies by the SDK's terminal event, so a native stream without one fails `openrouter_stream_incomplete` (`native_ended`, covered by `normalized_stream_emits_started_and_incomplete_error`) | `08:241-243` vs `crates/intention-providers/src/openrouter.rs:178-180` |
| D8 | ~~Required durable retry events exist in no document or crate~~ — resolved 2026-10-14: architecture 08 states the durable attempt count and the fixed 250 ms wait, and no event names exist | `08:239,251` vs `crates/intention-engine/src/runtime.rs` |
| D9 | ~~Instruction classification/redaction owner~~ — resolved 2026-10-14: 30 owns the classification and redaction of instruction content, and 09 owns configuration loading and credential isolation | `30:17,202` vs `09` after Wave R3 (credential isolation only) |
| D10 | ~~Instruction audience is deleted in `30` but alive in `21`/`22`/`23`~~ — resolved 2026-10-13: the audience dimension is removed from 21/22/23 as well | `21:60,276,304`, `22:22`, `23:15` |
| D11 | ~~Unavailable selection: pre-admission rejection vs post-admission run failure~~ — resolved 2026-10-14: 22 and 29 state one rule, the run fails with `provider_configuration_unavailable` and no provider call | `22:196-199` vs `29:66-68` |
| D12 | ~~`SessionProviderProfileChanged` required but no event boundary defined~~ — resolved 2026-10-14: the event and its M6 reservation are removed | `29:33-37`, `11:261` vs `11:170` |
| D13 | ~~Credential handling both forbidden and mandated in one section~~ — resolved 2026-10-13: the rotation section is removed, so the contradiction has no source | `25:22-35` |
| D14 | ~~Profile UI assigned to 29 while 29 declares it a non-goal~~ — resolved 2026-10-14: 29 owns the list and selection protocol, the picker/editor presentation stays a declared non-goal, and 25 owns only the editing direction | `25:66-69` vs `29:124-125` |
| D15 | ~~Flat goals with "no bound in either direction" vs tree depth/child limits~~ — resolved 2026-10-14: 28 keeps the parent link display-only and points at the 09 registry entries that bound the tree | `28:56` vs `28:175-176,189-190` |
| D16 | ~~Compaction owned by two documents~~ — resolved 2026-10-14: 21 owns compaction; 28 references it | `21:301-307` vs `28:132-146` |
| D17 | ~~Skills owned by two documents; one code defined verbatim twice~~ — resolved 2026-10-14: 21 owns Skills; 28 references it, and the duplicated codes are deleted | `21:58-151,235-258` vs `28:110-117,197-200` |
| D18 | ~~Goal scope/link semantics stated in two documents~~ — resolved 2026-10-14: 21 references 28 for Goal identity, scope, and links | `21:41-56` vs `28:22-58` |
| D19 | ~~Run-selection composition split between goal and ordinary runs~~ — resolved 2026-10-14 for the definition: both run-selection shapes carry the one `SkillSelectionV1` defined by 21 | `28:87-89` vs `21:227-229` |
| D20 | ~~Bounds defined outside the single limits registry: 22's bounds, 23's representation bounds, and 28's Goal-domain bounds sat outside it~~ — resolved 2026-10-14: the Goal-domain and conversation-tree bounds are 09 registry rows, 22's bounds are gone with the provider rewrite, and 23 and 28 reference 09 | `22:259-261,344`, `23:227-228`, `28:171-182` vs `09:123-131` |
| D21 | ~~Doc 10 mandates architecture checks the quality policy calls review duties~~ — resolved 2026-10-14: 10 states each rule with the mechanism that decides it and leaves the rest a review responsibility, matching 12 | `10:53-54` vs `12:83-87` |
| D22 | ~~Roadmap states six CI jobs and a `slice2` quality phase~~ — resolved before this wave: no document states a CI job count other than the four matrix jobs in 12 and the README | `11:134-135` vs `12:22-26`, `.github/workflows/quality.yml` |
| D23 | ~~Doc 12's nineteen-row failure table cannot be checked (probes deleted)~~ — resolved 2026-10-14 (R7): the failure table and the probe scripts it named are gone | `12:178-204` |
| D24 | ~~Aggregate diagram entities with no table~~ — resolved 2026-10-14: the two request entities are removed from the diagram and the identity invariant | `04:12-24` vs `04:122-123` |
| D25 | ~~Required rows name unbuilt adapters and a deleted gate~~ — resolved 2026-10-14: 05 names the compiler entries that deny CWD fallback, and the adapter rows in 03 and 05 scope to the adapter that exists | `05:197,202-204`, `03:313,318-320` vs `11:248-255` |
| D26 | ~~Checkpoint save failure stops the run while `Optional` restore failure starts empty~~ — resolved 2026-10-14: 20 applies one rule to both directions: `Required` blocks before effect, `Optional` continues with a typed degraded result | `20:190-194` vs `20:161,45` |
| D27 | ~~Tool schema: code owns it vs a run embeds it and must not re-derive~~ — resolved 2026-10-14: the selection no longer embeds schema text; it is read through `spec(id)` at request build | `15:63` vs `15:143-144,149-151` |
| D28 | ~~Unshipped owners "hold no slot" vs effect flags/mode relations for them~~ — resolved 2026-10-14: the effect profile and the per-tool mode relation are removed, and the unshipped owners hold no slot | `15:58-61` vs `15:73-82,86-89` |
| D29 | ~~Historical M3/M4 rows remain readable evidence vs stamp discard/recreate~~ — resolved 2026-10-14: 18, 19, and 20 speak of rows the current stamp carries, since a database without it is discarded whole | `18:140-142`, `19:127`, `20:239` vs `02:192-193` |
| D30 | ~~MCP connection record claims to mirror provider profiles, with a different endpoint model~~ — resolved 2026-10-13: the record carries one visible validated credential-free endpoint, like a provider profile | `18:35-36,43,49-53` vs `22:58,147-151` |
| D31 | ~~Policy ignore list that another checker forbids from being non-empty~~ — resolved 2026-10-14: the checker and the policy file it read are deleted, and the reviewed `advisories.ignore` set is a stated review responsibility | `quality/outdated.toml:13-14` vs `quality/check_deny_policy.py:66-68` |
| D32 | ~~Milestone range says M6-M11 while headings jump M8 → M10~~ — resolved 2026-10-14: the remaining plan reads M6-M8 and M10-M11, matching the headings | `11:22,70-73` |
| D33 | ~~Stale R2-R5 leftovers: one-value "closed" catalog state~~ — resolved 2026-10-14: the catalog read surface and its readiness states are removed with the 29 rewrite | `29:91-92`; the rotation parenthetical and the restart wording are resolved 2026-10-13 with the removed directions (`25`, `09`) |
| D34 | ~~Interactive gates contradict unattended execution~~ — recorded 2026-10-13: principle 12 states that no execution path waits for user input; `ask_user` is asynchronous, the MCP per-method approval gate and the audience/disclosure layer are removed, and Plan approval and Goal proposal acceptance stay user workflow steps outside a run | `00` principle 12, `15:116-119`, `18:148-155`, `21` |

## Ranked velocity blockers (overall)

1. ~~Theme as daemon-owned protocol state (F2-2, C3-1, C1-5).~~ **Implemented 2026-10-13**; the design notes below
   are the record of what landed.
2. ~~Tool-contract machinery: effect profile, per-run schema copies, 18 enum members over 3 stored values,
   code-owned-definition validation (F4-1…F4-3, F3-8, C3-5).~~ **Implemented 2026-10-14 (docs + code);** the stored
   status plus metadata codes, the ordered `ToolId` list with one tool-layer revision, and the validation removal;
   `ToolResultOutcomeDto::Cancelled` is removed too (C2-6).
3. ~~Provider framework, four-document ownership, cross-turn reasoning, catalog tombstones (F5-6, F5-8, F5-9,
   F5-11, F7-12).~~ **Implemented 2026-10-14 (docs);** named entries plus a global default and a per-session
   selection replace the framework, and the same-run echo stays with architecture 08 (F5-8's 2026-10-13 correction).
4. ~~Context manifest/projection, audience dimension, Skill lattice/catalog, two-owner compaction and Skills
   (F5-1…F5-4, F5-20, F5-23).~~ **Implemented for the remaining parts 2026-10-14 (docs);** F5-1/F5-2 landed
   2026-10-13, and the Skill lattice, catalog, and two-owner duplication are resolved now
   (F5-3…F5-5, F5-20, F5-23).
5. ~~Doc 25's control-plane cluster (F7-1…F7-6).~~ **Resolved 2026-10-13: the cluster was removed from
   architecture 25; discovery is kept and typed configuration editing is restart-only.**
6. ~~Runtime state with no consumer: the unread projections and the four run-terminalization writers are closed~~
   (C1-1…C1-3, C2-1, `bba5e82`), and the status-frame second read closes with the working-tree wave (C2-2). **The
   residual lane closes 2026-10-22:** the per-turn identity machinery is deleted while the durable queue stays, the
   run row carries its frozen configuration, storage failures are two codes, the wire-version byte is gone, and the
   registry keeps only the admission-window cancel fallback (F2-3, F2-4, F2-6, F2-7). Project/workspace identity stays
   by decision.
7. ~~Coverage machinery, per-crate tier table, doc 10's layer/rule tax (F6-3, F6-5, F6-6, F1-4).~~
   **Implemented 2026-10-14 (implemented + docs):** one instrumented run with per-crate `--fail-under-lines`
   floors in the Makefile; `run_coverage.py`, `check_coverage.py`, and `coverage.toml` are deleted; doc 10's two
   nonexistent architecture rules are removed. The layer table itself stands.
8. ~~Plan frontmatter, lifecycle, retry events (F3-3, F3-4, F3-7).~~ **Implemented 2026-10-14 (docs);** body-only
   plans in the daemon-defined directory with a model-provided file name, a three-state lifecycle, and no
   retry-event names.
9. ~~VFR/Headroom crates and the CCR store (F3-1, F3-2).~~ **Implemented 2026-10-14 (docs);** the modules land inside
   `intention-tools`; the durable tool result is the record and the CCR store is a bounded read-through view with no
   retention, rotation, or expiry policy of its own.
10. ~~Instruction kinds/scopes (F3-10).~~ **Implemented 2026-10-14 (docs);** the base text, one user fragment list,
    `AGENTS.md`, and the two reserved contributions.
11. ~~Process checks that re-validate other tools (F6-1, F6-2, F6-7, F6-8, F6-11, F6-12, F6-13, F6-14, F6-15).~~
    **Implemented 2026-10-14 (implemented + docs):** the re-validating scripts, the two duplicate advisory and
    freshness tools, the timing log, the checklist file, the label system, and the manual e2e workflow are gone;
    the cache-cleanup cron stays by decision. **2026-10-22:** the notices artifact, its generator, its template, and
    its CI step go too, so nothing re-validates another tool and `cargo deny` is the only license and advisory
    authority.
12. Code: the 528-line source scanner, the raw-SQL storage tests, the production test seam, and the duplicated TUI
    fixtures are closed (`bba5e82`: C1-7, C1-13, C2-9, C3-14). Two appliers of one stream state, four virtual clocks,
    three fixture builders, the snapshot byte count, and the daemon-binary search close with the working-tree wave
    (`5e9c471`: C1-6, C2-10…C2-13).
13. ~~Documentation restatement tax: README triplication, `WorkspaceRoot` ×10, doc 03's changelog, per-document
    ceremony (F1-5, F1-12, F2-1, F2-13, F7-13).~~ **Closed 2026-10-22:** the README states what the product is and
    links to the owners, the addressing rule is stated once, document 03 carries no changelog, and the ceremony
    clauses and evidence lines are gone.

## Blocker 1 in detail: the theme without daemon-owned state

**Implemented 2026-10-13** in `ffdf4cf`, with the resolution chain `--theme` → `COLORFGBG` → local `tui.toml` →
bounded `OSC 11` background probe → light default, a live switch that repaints in memory and rewrites the local file off
the draw path, and no daemon, socket, or table involvement. One deviation from the sketch below: there is no OS-level
theme read, so zero-configuration detection answers through the terminal surface the front end actually paints.

Replacement direction for F2-2, C3-1, C1-5 (2026-10-13). The setting survives; the protocol
machinery does not. Four mechanisms, none of which needs the daemon, the socket, or a table:

1. **One local TUI configuration file (the right path).** The TUI reads a plain text file at startup, for
   example `~/.config/intention/tui.toml`, containing one line:

   ```toml
   [tui]
   theme = "dark"   # or "light"
   ```

   Start, read the file, pick the palette, draw the interface. No socket, no daemon request, no database.

2. **Zero-config detection by OS or terminal.** Modern environments already report this:
   - Through the OS: a small crate such as `dark-light` asks Windows, macOS, or Linux for the user's system
     theme in about a millisecond.
   - Through the terminal: the TUI sends the standard `OSC 11` escape code, the terminal answers with its
     background colour, and the TUI derives light or dark immediately.

   With no user configuration at all, the TUI adapts to its environment.

3. **Launch flags and environment variables.** `intention-tui --theme light` for a one-off or a script, plus
   `COLORFGBG`, which many terminal emulators export automatically.

4. **Live switching still needs no daemon.** The usual justification for the machinery is "the user wants to
   switch the theme inside a running TUI". That also needs no daemon: a hotkey or a `:theme light` command
   swaps the palette in TUI memory and calls the redraw (`terminal.draw()`), which is effectively instant; the
   TUI then asynchronously rewrites `theme = "light"` in its own local `~/.config/intention/tui.toml` so the
   choice survives the next start.

Consequence for the cut: the setting stays, and the daemon never learns the theme. Delete the two wire ops, the
singleton table, the four single-field DTOs, the daemon-side `[tui]` theme key, the echo check, the
layout-cache key dependency, and the ~110 production references across the seven crates.

## Wave R6 in detail (2026-10-14)

Seven commits on `cleanup/overengineering-removal`, pushed to `origin`; `make quick` (886 tests) and `make verify`
(coverage 92.143%) pass.

| Commit | Blockers | What landed, and where the wave deviated from the sketch |
| --- | --- | --- |
| `b79ebb1` | 4 | Skill model: origins shrink to `Project` (project wins) or `Personal`, states to `Enabled` or `Disabled`, card→exact revision→body, `SkillSelected`/`SkillBodyAttached`, six codes; 28 references 21; permission/question record remnants removed from 04/23/09. Deviations: supplements and the frontmatter metadata fields are dropped entirely, and one frozen `SkillSelectionV1` is the only carrier. |
| `f345621` | 2, 8 | Tools: the durable row stores `Completed`, `Failed`, or `Partial`, plus metadata codes; ordered `ToolId` list with one tool-layer revision; effect profile deleted; `Deferred` removed (async `sub_agent` is `Completed`). Code: definition validation (three codes and tests) and `ToolResultOutcomeDto::Cancelled` removed. Plans: body-only files in `<AppData>/plans/` with a model-provided file name (`invalid_plan_file_name`, `plan_name_conflict`), the `drafting`/`approved`/`abandoned` lifecycle, no frontmatter; the retry-event names replaced by the durable attempt count. Deviation: the naming rule is new, not in the sketch. |
| `2e99e10` | 9 | VFR/Headroom as `intention-tools` modules; no separate crates, no non-import rule, coverage sits under the `standard` tier; retention, rotation, and expiry deferred to `@todo(vfr)`/`@todo(headroom)`. Deviation: the CCR store stays as the retrieval backing; only its retention policy is deferred. |
| `75509c5` | 3 | Provider entries: `[[provider]]` plus `default_provider`, one resolved entry per run into the frozen snapshot; 29 is session selection only (last-write-wins, no per-turn or fork overrides, bounded list); discovery simplified; 23 loses reasoning references and future-profile overrides. Deviations: a named list with a per-session selection replaces the single-selection sketch; `responses` stays a future first-party kind; the same-run echo stays with 08. |
| `537be51` | 10 | Instructions: the code-owned base text, one user fragment list, `AGENTS.md`, and the `Mode`/`Vfr` contributions; fixed assembly; frozen text with no projection records or revisions; one failure code. As sketched. |
| `fe2182f` | — | `build(deps)`: tokio 1.53.2 → 1.53.3 with regenerated notices, because the stale-pin gate tripped on the upstream release (F6-2; the gate itself was removed by Wave R7). |
| `95e0c49` | — | Follow-up from this audit update: 29 states the same provider-selection failure rule as 22 (D11) and references the startup-only owners instead of restating them. |

Not adopted in this wave: F3-5, F3-6, F3-9, F3-13, F3-14, F4-5, F5-12, the untouched parts of F5-13, F5-14…F5-19,
F5-21, F5-22, the F6 findings (closed by Wave R7), F7-13, and the C1/C3 findings outside C2-6 and C3-5. The wave kept
the load-bearing rules it touched: the stored status set, the closed `ToolId` set, the frozen tool selection, the
trusted-local statement, the three admission outcomes, the endpoint and credential rules, startup-only configuration,
the same-run reasoning echo, the `@todo(provider)` replay marker, and the freeze-at-admission instruction text.

## Wave R7 in detail (2026-10-14, process and CI)

Two commits on `cleanup/overengineering-removal`, pushed to `origin`. `quality/*.py` goes from 1,414 lines across
nine files to 297 across three; the CI matrix keeps its four jobs.

| Commit | Blockers | What landed, and where the wave deviated from the sketch |
| --- | --- | --- |
| `21dd69c` | 7, F6-1…F6-6, F6-8…F6-12 | Coverage: `cargo llvm-cov --no-report --workspace … nextest` once, then `cargo llvm-cov report --package <crate> --fail-under-lines <floor>` per crate from `COVERAGE_FLOORS`; `run_coverage.py`, `check_coverage.py`, and `coverage.toml` deleted; no exclusion mechanism. Deps: `cargo metadata --locked`, `cargo deny check`, `cargo machete`, notices check; `cargo audit`, `cargo outdated`, `outdated.toml`, and `check_deny_policy.py` deleted, with the five-entry `advisories.ignore` rule recorded as a review responsibility. Chain: `run_profiles.py`, `run_deps.py`, and `timing.py` deleted and their commands inlined; `check_tools.py` reduced to `--component`/`--tool` validation, so `CI_TOOLS_SCOPE` and the PATH assertion are gone. Docs 00-12, README, and AGENTS.md updated in the same commit. Deviations: the three floors keep their old values (75/60/20) rather than collapsing to one; the `intention-daemon/src/main.rs` exclusion is dropped rather than reimplemented, which holds because the daemon sits at 86.57%; the informational workspace report stays as an artifact with no threshold. |
| `259d6d1` | F6-14, F6-15 | `AGENTS.md` loses the nineteen-label section and the numeric parallelism quotas; dependabot stops applying labels; `checklists.md` is deleted with its live rules folded into SKILL.md's self-check; the manual `real-api-e2e.yml` workflow is deleted while `make e2e-real-api` and the ignored test stay. Deviation: the two-stage structure of the planning skill stays, and `cache-cleanup.yml` keeps its cron, both by decision. |
| `7f28842` | — | CI follow-up. Two defects were visible only on a fresh checkout: `doctest` and `doc` demanded `cargo-nextest`, which the lint-arch job does not install, and `coverage` wrote its aggregate report into the gitignored `quality/reports/`, which the deleted runner used to create. Both are fixed; the first CI run on this wave failed lint-arch and coverage-default, and the run on `7f28842` is green on all four jobs. |

Not adopted in this wave: F6-13 (the 5,976-line notices artifact and its byte-compare stay), the `cache-cleanup` cron,
doc 10's ten-layer/twelve-rule tables, and doc 11's closeout narration. The wave kept every gate that was not
re-validating another tool: rustfmt, native and Windows-target Clippy, nextest, doctests, rustdoc, the lockfile anchor,
the supply-chain policy, the notices check, and the per-crate coverage floors.

**Accepted costs.** A crate absent from `COVERAGE_FLOORS` is silently unmeasured; a newly added `advisories.ignore`
entry passes the gate; a stale direct dependency no longer fails a build. All three are stated in
[architecture 12](docs/intention-relay/architecture/12-quality-gates-and-makefile.md) and `deny.toml`.

**Validation run for this wave:** `make quick` (886 tests), `make verify`, and the four `ci-*` aliases pass; a raised
`intention-engine` floor fails `make coverage` with exit 2, and a banned crate in `deny.toml` fails `make deps` with
exit 2. The GitHub Actions run on `7f28842` is green on all four jobs.

## Wave R8 in detail (2026-10-14, throughput and dependency facts)

The gate is correct but was paying for work it had already done. The findings come from the green run `38548878149`,
the two runs on the wave's own commit `665b569`, and local runs on the same commits; the CI job count, the floors, and
the artifact names are unchanged.

| Finding | Measurement | Resolution |
| --- | --- | --- |
| Nine `report --package` calls re-merge the same profiling set | 10.8 s of the 154 s coverage step, nine merges of one profdata | One `report --json --summary-only` plus `quality/check_coverage_floors.py`, which reads the per-file line summaries the artifact already carries. The report phase is 0.65 s. |
| sccache does not cover the native builds | `aws-lc-sys` (cmake) and bundled SQLite are the 12.6 s and 26.3 s gaps in the instrumented build | The coverage job routes `CC`, `CXX`, `CMAKE_C_COMPILER_LAUNCHER`, and `CMAKE_CXX_COMPILER_LAUNCHER` through sccache. |
| `rust-cache` never caches the coverage target | 101 MB restored, post-step "Cache up-to-date"; the job restores the crate sources it cannot replace and compiles all 221 crates every run | The coverage job keeps only an `actions/cache` entry for `~/.cargo/registry` and `~/.cargo/git`, keyed by `Cargo.lock`. `lint-arch` and `test` keep `rust-cache`, which is their only dependency cache: they rebuild 11 workspace crates instead of about 250. The `deps` job compiles nothing, so its 343 MB target cache is gone and only the registry entry remains. |
| The instrumented target is 4.0 GB and is deleted anyway | `du -sh target/llvm-cov-target` | `coverage-artifacts-clean` reports the size before removing it; the tree stays out of every cache. |
| `apt-get update` for mold | 7.7 s, mold 2.30.0 | `rui314/setup-mold` pinned to a commit, same version, `make-default: false`. The `deps` job installs no linker and passes no linker flag. |
| One test waits out a production constant | `background_descendant_holding_pipes_cannot_extend_the_collection` took 5.116 s, over a third of the 13.7 s suite | `READER_DRAIN_GRACE` stays 5 s in production; the window is a parameter of an internal function and that test passes 100 ms. The test is 0.216 s. Honest scope: the suite is worker-bound at 64.2 s of test CPU, so this is about one second of wall time, not the three to four a single straggler would suggest. |
| `rust-cache` wrote a new key on every run | Seven `test` and seven `lint-arch` generations (6.8 GB) side by side, the cache at 9.53 GB of its 10 GB limit, 58 failed cache writes on the cold run | `add-job-id-key: false`: the matrix phase already separates the jobs, so the job id added nothing but copies and each phase now owns one entry that the next run replaces. |

Commits: `c070395` (floors from one report), `7513bab` (per-phase caching, C/C++ objects, mold from its own action,
dependency facts), `54c22f8` (the pinned SDK version in doc 08), `665b569` (the drain window), `1c582eb` (one stable
rust-cache key per phase).

Not adopted: removing `openrouter-rs`. It is the sole owner of three duplicate families, but collapsing it means
rewriting the 828-line adapter onto the generic Chat Completions path, changing the documented two-driver design, and
it saves nothing measurable because sccache already serves those crates. The graph is left alone and the facts are
recorded in [architecture 12](docs/intention-relay/architecture/12-quality-gates-and-makefile.md) and `deny.toml`
instead.

**Deviation from the approved sketch:** the plan said to remove `rust-cache` everywhere. Measurement showed it is
`lint-arch`'s and `test`'s only dependency cache (11 recompiled crates against roughly 250 from cold), so it stays in
those two jobs and goes only where it is provably unused or unhelpful.

**Validation run for this wave:** `make coverage` holds all nine floors with the workspace aggregate at 92.66%; a
raised floor fails through `make coverage COVERAGE_FLOORS="intention-engine=99"` with exit 2; the checker also fails on
a floor whose crate is absent from the report, on a malformed token, and on an unreadable report; `make deps` passes
with the new `deny.toml` comment; `make verify` and all four `ci-*` aliases pass; the affected `intention-tools` tests
pass in 1.02 s.

**CI measurement.** Five runs, four jobs green in every one: the baseline on `7f28842`, a cold and a warm run on
`665b569`, and a cold and a warm run on the final `1c582eb`. "Warm" is the second run against a populated cache.

| Measurement | Baseline | Cold | Warm |
| --- | ---: | ---: | ---: |
| coverage-default job | 193 s | 288 s | 163 s |
| lint-arch job | 84 s | 452 s | 73 s |
| deps job | 69 s | 76 s | 68 s |
| test job (Windows) | 205 s | 793 s | 244 s |
| coverage gate step | 143.3 s | 260.3 s | 128.3 s |
| instrumented build | 126 s | 242 s | 114 s |
| report phase (nine reports + JSON) | 10.8 s + 1.2 s | 0.8 s | 0.80 s |
| mold install | 7.7 s | 1 s | 1 s |
| Windows suite | 94.5 s | 146.8 s | 110.5 s |
| sccache C/C++ hit rate | not cached | 0.72 % | 99.64 % |
| sccache compilations | 1 | 442 | 59 |
| sccache write errors | 0 | 58 | 0 |

The cold column for `lint-arch` and `test` carries a one-time cost: the final commit changed the rust-cache key, so
those two jobs rebuilt from nothing once (452 s and 793 s) and then restored in full (73 s and 244 s). The warm column
is the steady state.

What the numbers mean. The report phase is where the wave promised its gain and it delivered: 12 s to 0.8 s, which
alone is a fifth of the coverage job. The instrumented build is 12 s faster warm because the C/C++ builds now hit the
object cache, and the `aws-lc-sys` cmake and bundled SQLite steps were the two longest gaps in that build. `lint-arch`
is 11 s faster warm, mostly the mold install falling from 7.7 s to 1 s. `deps` is unchanged, as expected: it compiles
nothing.

What is runner variance: the Windows suite runs 94.5, 146.8, 94.6, and 110.5 s on the same source, so that job's total
is noise around a fixed gate cost and it is now the critical path. Nothing in this wave changes what that job does; its
suite is identical warm to the baseline on the earlier commit (94.58 s against 94.55 s). A wall-clock claim for the
whole pipeline is therefore not available from these runs: the reliably faster parts are the coverage job and the lint
job, and the pipeline total is set by a Windows runner that varies by more than the gain.

**Cache budget.** After the key change the repository cache stands at 9.96 GB of 10 GB, and the composition shows why:
seven `test` entries (3.5 GB) and seven `lint-arch` entries (3.3 GB) from the old per-run keys, five dead `deps`
entries (1.7 GB) for a job that no longer caches a target tree, and 1,080 sccache objects (1.1 GB). The new key
replaces its phase's entry instead of adding one, so this is the backlog rather than a growth rate; it clears through
`cache-cleanup` on the next merge to `main` and through the cache API's own eviction. The cold run's 58 write errors
were the limit being reached while the C/C++ objects were written, which is why the budget became part of this wave
rather than a note.

## Wave R9 in detail (2026-10-14, Windows and coverage run time)

The cache cleanup (`38562432213`) ran first and took the repository cache from 9.96 GB to 2.71 GB in 631 entries, so
the two runs on `93afb59` started from empty caches: the first is cold, the second warm.

| Finding | Measurement | Resolution |
| --- | --- | --- |
| `-fuse-ld=lld-link` does not select a linker | rustc's MSVC path invokes the linker directly, so a GCC driver option reaches `link.exe` unparsed; `-C linker-features=+lld` is unstable and needs nightly | The `test` job sets `-C linker=lld-link`, falling back to the toolchain's own `rust-lld` because `RUSTFLAGS` splits on whitespace and the LLVM install path contains a space. The runner resolves `lld-link` at `/c/Program Files/LLVM/bin/lld-link`. |
| The Windows job runs 870 tests for 414 s of test CPU | Per-binary Windows CPU: `intention-daemon` 193.6 s, `intention-storage` 125.1 s, `intention-tui` 72.9 s, everything else 22.6 s. The two slowest single tests (56.25 s and 49.94 s) are daemon session-window and transport-envelope tests | `PLATFORM_TESTS` selects the crates that cross a platform boundary: `intention-transport`, `intention-tools` (the only caller of the Windows process API), `intention-test-support`, `intention-client`, `intention-config`, `intention-proto`, plus `intention-daemon`'s `client_e2e` binary as the cross-process proof. The suite is 189 tests in 12.45 s. |
| No test is Windows-only, so "only platform tests" cannot mean exclusivity | Two `cfg(windows)`-gated tests exist against twenty `cfg(unix)`-gated ones; every Windows test also runs on Linux | Membership follows the platform-conditional branches in the source, not the test's own gate. The suites left out assert logic no such branch reaches; `lint-windows` still compiles them for Windows. |
| `debug = 1` was already the effective test-profile setting | `cargo build --profile test -v` emits `-C debuginfo=1` | No change. Trimming it further would only remove backtraces from failing Windows tests. |
| The instrumented tree was believed to be unrecoverable | `cargo llvm-cov` refuses `--no-clean` together with `--no-report`, because `--no-report` already leaves the build tree in place; the four gigabytes disappeared because `verify` and `ci-coverage-default` called `coverage-artifacts-clean` after every run | `coverage-default` caches the instrumented tree with `rust-cache` like the other building phases, and the two gates stop deleting it. `coverage-artifacts-clean` remains an on-demand target that reports the size first. |

Commits: `93afb59` (LLD, the platform filter, and the coverage tree cache).

**CI measurement.** Four jobs green in both runs on `93afb59`. "Warm" is the second run against the caches the first
one wrote.

| Measurement | R8 baseline | Cold | Warm |
| --- | ---: | ---: | ---: |
| coverage-default job | 193 s | 288 s | 66 s |
| coverage gate step | 143.3 s | — | 31 s (29.6 s run, 0.68 s report) |
| test job (Windows) | 205 s | 747 s | 144 s |
| Windows suite | 94.5 s | 94.5 s | 12.45 s |
| lint-arch job | 84 s | 432 s | 90 s |
| deps job | 69 s | 69 s | 56 s |

Local: a second `make coverage` reuses the instrumented tree and takes 6.1 s against 155 s cold, with
`target/llvm-cov-target` at 4.0 GB on disk and 393 MB compressed in the cache. `make ci-test` runs 197 tests locally
in 3.0 s and 189 on Windows in 12.45 s; `make quick` and `make verify` still run all 886.

**Honest scope.** The platform filter removes no build time: cargo builds every workspace test target before nextest
applies the expression, so the warm Windows job still spends 40.45 s building and 54 s restoring its cache, and its
144 s is a floor set by the checkout and cache round trip, not by the tests. Narrowing `--workspace` to the selected
packages was not adopted: the four excluded crates are all dependencies of the daemon or of the tools, so their
libraries are compiled regardless, and a second package list to keep in step with the first buys a few seconds.
`sccache` in the coverage job is now idle on warm runs, because a restored target leaves nothing to compile; it stays
as the cold-run fallback and costs about two seconds of steps.

## Contradiction closure (2026-10-14, documentation alignment)

Every row of the contradiction table above reads as resolved. Seventeen were still open; three of those were already
fixed by earlier waves and had only kept their open marking (D6 by the version alignment in `54c22f8`, D22 when the
roadmap's per-milestone sections were removed, D23 when the process wave deleted the check scripts the failure table
named).

Two were not stale text but a wrong claim about behavior:

| Row | What the documents said | What the code does | Resolution |
| --- | --- | --- | --- |
| D7 | "the adapter cannot classify `openrouter_stream_incomplete`" | `native_ended` fails exactly that code, and `normalized_stream_emits_started_and_incomplete_error` proves the path | 08 states the rule the code implements: the completion signal is the SDK's terminal event, and a stream that ends without one fails incomplete |
| D26 | a checkpoint save failure stops the run regardless of policy | nothing implements the kernel yet; the restore half of the same section was already policy-driven | 20 applies one rule to both directions: `Required` blocks before effect, `Optional` continues with a typed degraded result, `Disabled` has no checkpoint to fail |

D20 took the one-owner shape the other duplicate-owner rows took: the Goal-domain and conversation-tree bounds are rows
of the limits registry in document 09, and 23 and 28 reference them instead of restating the values. D5 deleted the
second architecture picture rather than redrawing it, because 01's crate map is the live one.

Validation: every markdown link in the docs tree and the README resolves (26 files, 0 broken); `make doc` builds;
`make verify` is green in 45.4 s with all nine coverage floors, the advisory, ban, license, and source checks, and the
notices check.

## Wave R10 in detail (2026-10-14, future-surface reduction)

The F3/F4/F5 lanes over documents whose features are not implemented. Eleven findings closed; one of them was reviewed
and deliberately kept. No new pinned tool, no new gate, no new dependency, the CI job count and the nine coverage floors
unchanged.

| Finding | Cut | What was given up |
| --- | --- | --- |
| F5-12 fork preview with optimistic binding | The preview carries no materialized instruction text, no materialized model messages, and no stored reference lists; it returns counts read from the source. `expected_preview_binding` and `fork_preview_mismatch` go | Nothing: the fork transaction already validates `expected_source_sequence`, so the second guard only restated it |
| F5-13 sixteen-field snapshot | Six provenance reference lists (source run cursors, config/policy decisions, usage, tool results, child results) leave the snapshot and the preview | Nothing: the child's context is fully materialized, and the references stay on the source session's records |
| F5-15 second run-start path | `StartForkRunCommandDto` deleted; a fork child's runs start from ordinary user turns | Regenerate as a command: answering the anchor turn without a new user turn. Same-session regeneration is a separate mechanism and stays |
| F5-16 paged conversation tree | Continuation token, page size, the 64-summary bound, and `invalid_conversation_tree_page` deleted; a read returns one parent's immediate children in stable creation order | A page ceiling on a very wide branch list; the transport envelope's typed `local_protocol_message_too_large` is the only ceiling now |
| F5-19 goal tree | Nothing changed | Nothing: the parent link is display-only, so the flat model keeps its meaning while the presentation tree survives |
| F5-22 second run kind | `VerificationOnly` and the selection's `run kind` field deleted | The type-level guarantee that a verification run cannot alter goal state |
| F4-5 layered progress deadline | `model_stream_progress_timeout_v1`, its failure code, and its retry rule deleted; 08's attempt window is the one owner | Nothing: the window bounds silence, not content, and the pause rule stays in 15 for the refinement that would need it |
| F4-13 checkpoint metadata | Four of eleven fields deleted (source execution, source run, parent checkpoint, verification status); one verified checkpoint per selection, replaced atomically | Lineage and promotion history, which had no consumer |
| F4-15 kernel DTO family | Nine names to four; the cell's invocation and result are architecture 15's | Nothing: the execution binding, script library, checkpoint payload, and attempt status stay as values inside their records |
| F3-6 plan allocation | The reconciliation mandate and the speculative never-reuse clause become one ordering rule: write and flush the body file before the record commits | Nothing observable: monotonic numbering per session stays, and a file-system failure still leaves no usable record |
| F3-9 reasoning-echo failure path | Two codes and two branches for one value become `invalid_model_assistant_reasoning` | Nothing: the 512 KiB bound, the control-character rejection, and the presence-only case are unchanged |

Commits: `e193e19` (F5: 23, 28, 09, 11), `af54b76` (F4: 15, 20), `09cdccf` (F3: 07, 09, provider code and tests).

Validation: every markdown link in the docs tree and both READMEs resolves (27 files, 0 broken); the eleven removed names
appear nowhere in `docs/`, `crates/`, or the README; `make verify` green in 89.2 s with 886 tests twice, all nine
coverage floors, the advisory/ban/license/source checks, and the notices check; all four CI jobs green on `09cdccf`
(run `38594062090`).

## Wave R11 in detail (2026-10-14, remaining F3/F4/F5 tails)

Seven rows were named for closure. Five were still open; F5-20 and F5-23 had already been closed by earlier waves and
needed no work. Documentation only, except the reasoning-code merge in the previous wave.

| Finding | Cut | What stays |
| --- | --- | --- |
| F4-14 per-cell import evidence and a versioned record for a constant path | `KernelScriptLibraryV1`, the selection's `script_library_reference`, the per-cell import record, `KernelScriptImportEvidenceDto`, and `kernel_script_library_unavailable` | The library is the constant `.ir/scripts`, its import surface is that directory, a fresh run reuses a module by reading the file, and an import that fails is the cell's own error. The kernel DTO family drops to three names |
| F4-16 archaeology sentences | Retained-research headers, "removed ... leaves no live path" clauses, "M3/M4 rows gain no X" paragraphs, "Historical M4 tool calls remain denial evidence", and milestone-relative wording ("During M4", "M1 defines", "M3/M4 own") across 05, 08, 09, 15, 18, 19, 20, 21, 22, 30, and the roadmap | One forward invariant replaces the class: no record carries state it did not record, and no current state reconstructs meaning a record never had. Forward exclusions ("no second activation path", "no legacy selection bridge") survive in present tense |
| F5-17 fork record families and a derived tree identity | `ConversationTreeId`, the UUIDv5 namespace and canonical-name derivation, the conversation-tree record family, `ConversationTreeCreated`, `ConversationBranchLinked`, and `conversation_tree_id` in both field tables | A tree is identified by its root `SessionId`; lineage is the child's `ParentSessionId`; storage adds only the base snapshot and the fork-operation idempotency record |
| F5-18 duplicated codec-removal paragraph | The duplicate in the base-snapshot section | The paragraph appears once, in the field tables; the `Disabled` reasoning-transfer rule and the one-live-shape statement stay |
| F5-20 two owners for Skill and compaction codes | Nothing: already resolved before this wave | 21 owns the codes; 28 references them |
| F5-21 `RefinementDraftDto` proposal domain | The draft record family, its identity, edit set, evidence references, coalescing rule, `refinement_draft_conflict`, and `refinement_draft_too_large` | A proposal is one pending `ask_user` question naming the target record, its exact base revision, and the change as text; one per run; a stale base is `goal_revision_conflict` |
| F5-23 compaction built twice | Nothing: already resolved before this wave | 21 owns compaction; 28 references it |

Two consequences worth stating. The kernel document now carries three DTOs rather than four, because the import-evidence
DTO went with F4-14. And the roadmap lost two acceptance criteria that named the deleted mechanism, so the M11
acceptance list no longer asserts behavior that no longer exists.

Commits: `20ab503` (kernel), `d8d4e58` (branching), `e4bf707` (goals and the limits registry), `160aae3` (archaeology
sweep across nine documents).

Validation at commit time: every markdown link in `docs/` and both READMEs resolves (28 files, 0 broken); none of the
eleven removed names appears anywhere under `docs/` or in the README; `git diff --name-only` shows documentation only.
Compilation and `make verify` are deferred to the next gate run, by request.

## Wave R12 in detail (implemented-code tails)

Two commits on `cleanup/overengineering-removal`: `bba5e82` removes the implemented surface no production caller
consumed and the logic that existed in two or three copies, and `411b163` lands the documentation half.

| Finding | Cut | What stays |
| --- | --- | --- |
| C1-1 `active_run` on session summaries | The field, its producer, and its coherence rule | `SessionProjectionDto::active_run` carries the run |
| C1-2 pending-turn projection, budget, omitted count | The projection fields, the byte budget, and the omitted count | `PendingTurnProjectionDto`, because the turn queue builds it |
| C1-3 `ErrorRetryDto` | The enum becomes `bool retryable` end to end | The one real reader, which distinguished only `Delayed` |
| C1-4 `CorrelationIdDto` | The type, the field, the accessor, and the engine's forwarding | Nothing: no provider produced it |
| C1-7 528-line storage source scanner | The scanner | The `clippy.toml` `disallowed_types` rule the compiler already enforces |
| C1-8 SQLite result-code ladder | Four arms no statement can produce and a duplicate wrapper | Every code, category, and retry value a caller observes |
| C1-9 turn-idempotency replay | The unreachable `replayed_turn` path | `turn_identity_conflict`, which a repeated key already produces |
| C1-10 stale-vs-malformed daemon line | The split | The one fail-closed `invalid_local_protocol_response` |
| C1-11 redundant client identity guards | Five guards the correlated reply identity already proves | `SessionStreamState::ensure_scope`, because stream frames are uncorrelated |
| C1-12 dead accessors and duplicate codecs | `SchemaVersionDto` and its accessors | The two codecs that accept a wire spelling serde cannot express |
| C1-13 raw-SQL storage tests | The recovery test reads `RunProjectionDto` | The schema, stamp, malformed-value, and broken-backend sites, which no trait call can produce |
| C1-14 `DaemonReadinessDto` | The one-variant enum | The empty `DaemonHealthDto` with `ready()` |
| C2-1 run terminalization in four places | `terminalize_run` and `terminalize_active_run` own the two halves | Each path's status, finish time, and notice |
| C2-3 copied interruption boundary | One `settle_interrupt` for all four sites | The codes and notices |
| C2-5 two cancellation flags | One shared flag the model stream and the tool path both observe | No disagreement between the two |
| C2-6 `ToolResultOutcomeDto::Cancelled` | The variant, its constructor, and its handler | The stored `Completed`/`Failed`/`Partial` status set |
| C2-7 admission selection re-validation | The self-comparison against the value it was just built from | The three checks that compare independently supplied values |
| C2-9 daemon test seam | The recorder trait pair, its no-op implementation, and the host alias | One `cfg(any(test, feature = "test-support"))` recorder |
| C3-2 config source-kind and path DTOs | `ConfigSourceKindDto` and `ConfigSourceDto` | One validated `ConfigPathDto` plus the resolver |
| C3-3 schema-version equality machinery | The equality apparatus, the snapshot-side version, and the persistence gate | One `CURRENT_SCHEMA_VERSION` check in the parser |
| C3-4 capability declaration | Four constant flags and six getters | Streamed text with tool calls, and `unsupported_model_capability` |
| C3-5 tool-definition validation | The three codes and their tests | Infallible `ModelToolDefinitionDto` construction |
| C3-6 reasoning-echo identity rule | The empty/duplicate identity rejection that could never fire | The attachment and its 512 KiB bound, which the generic driver needs |
| C3-7 second stream-order validator | `ModelStreamLifecycleDto` and `invalid_model_stream_order` | `stream.rs` as the only order enforcer, with one test |
| C3-8 duplicated grep body | The hand-inlined `GrepScope::File` branch, −71 lines | The one `grep_scoped` body |
| C3-9 third argument-list copy | The guidance argument table | Guidance rendering the tool's own `input_schema()` |
| C3-10 layout-cache invalidation | The tool-result count key | Width, theme, epoch, and reasoning epoch; the append fast path |
| C3-11 two command vocabularies | The REPL's private parser and its own variants | One `COMMANDS` registry for both front ends |
| C3-12 stub screens | The three browser tabs, the input placeholder, and the welcome stub blocks | A surface with no fact renders nothing |
| C3-13 two consecutive-press arms | `CtrlCArm` and `escape_arm` become one `Arm` and one `armed` field | Both keys keep their exact behavior |
| C3-14 three fixture copies | `intention-test-support::fixtures` holds the builders and identity constants | Every assertion is byte-identical; the three sites lose 738 lines |

`411b163` lands the documentation half: the Plan-mode matrix states that Plan denies the project-mutating descriptors
without claiming containment, the mode invariants cross-reference the global owners, the VFR/Headroom store is a
read-through view of the durable tool result, observability lists only what the daemon emits, the provider documents
state one capability requirement and one stream-order enforcer, document 03 states the five-command registry, the MCP
and bridge documents state each rule once, and presentation commands drop the operation-id layer.

## Working-tree wave in detail (`5e9c471`)

Commit `5e9c471` on `cleanup/overengineering-removal` (23 files, +782/−900) closes C1-6, C2-2, C2-8, C2-10,
C2-11, C2-12, and C2-13:

| Finding | Change |
| --- | --- |
| C1-6 two appliers of one stream state | `StreamStep::Frame` carries the frame, its `FrameUpdate`, and the state the subscription applied it to; `Action::FrameReceived { frame, update, state }` hands the state to the front end, which no longer calls `apply_frame` on a copy of its own |
| C2-2 status frame second read | `SessionCommitDto::Status` carries the committed `RunProjectionDto`; the terminal publication builds its frame from it, and the bounded publication-retry worker, `retry_bounded`, `PUBLICATION_RETRY_ATTEMPTS`, and `PUBLICATION_RETRY_DELAY` are deleted |
| C2-8 cfg-switched provider slot | `SelectedProvider` and `InjectedProvider` collapse into one `ConfiguredProvider` enum with a `cfg`-gated `Injected` variant |
| C2-10 four virtual clocks | One `VirtualTime` clock with `holding(...)` and `signalling(...)` replaces `ImmediateTime`, `ManualTime`, `RetryDelayTime`, and `RoundSelectTime`; no delay is classified by its magnitude |
| C2-11 three fixture builders | `fixture_snapshot()` and `fixture_session_command(session_id, label)` live in `intention-test-support`; the two daemon copies are deleted |
| C2-12 snapshot byte budget | One `CountingWriter::encoded_bytes` helper; the trim loop walks newest-first and encodes only the served rows and the first rejected one |
| C2-13 daemon-binary discovery | The five-level ancestor search and its fallback collapse to the profile directory one level above the running test executable's own directory |

Validation of the commit: `cargo check --workspace --all-targets --locked --all-features` is clean, and
`make quick` is green (875 tests passed, 2 skipped, 105 s). The documentation half lands in the closure wave: document
03 carries no changelog, names no injection seam, and states the committed status value it publishes.

## Closure wave in detail

Commits `e1c27fe`, `a0366d9`, and `e9cc6ef` on `cleanup/overengineering-removal` close every finding that the
working-tree wave left open.

| Finding | Change |
| --- | --- |
| F2-3 per-turn identity machinery | The durable queue stays and every accepted turn still queues: `accept_user_turn(session_id, content, config_snapshot, occurred_at)` mints the run identity itself, a turn accepted during an active run commits a `pending` row, and `consume_pending_user_turns` marks each consumed turn `appended` in the transaction that commits its user message. Deleted: the caller-proposed `RunId`, `IdempotencyKey`, the replay path, `turn_identity_conflict`, `RemoveTurnCommandDto`, `RemoveTurnAcceptedDto`, `ClientRequestDto::RemoveTurn`, `ProtocolResultDto::TurnRemoved`, `ApplicationService::remove_turn`, the facade method and daemon dispatch arm, the `proposed_run_id`, `idempotency_key`, and `config_revision_id` turn columns, and the pending-turn projection budget and omitted count. The terminal front end keeps its one acknowledgement, "turn queued behind the active run", so a repeated command is an ordinary second turn |
| F2-4 run-carried configuration | `ConfigRevisionId` and the `configuration_revisions` table are deleted; `runs` carries `config_snapshot_json`, written when the turn starts the run and read back by `load_run_config_snapshot`. The snapshot's content is the revision: nothing mints an identity for it and no cross-run dedupe or conflict remains. `ConfigSnapshotDto` loses its `revision_id`, `RunProjectionDto` and `SessionProjectionDto` lose their `config_revision_id` (always `None` in production), and `SendUserTurnOutcomeDto::Started` carries the run identity alone. `SCHEMA_STAMP` moves 6 → 7, so an existing database is discarded and recreated under the one live schema |
| F2-4 storage codes | Two codes remain: `storage_record_not_found` (NotFound, not retryable, only a genuinely absent expected row) and `storage_unavailable` (Unavailable, retryable, every other backend, driver, or decode failure). `run_configuration_not_found` and `run_model_context_unavailable` are deleted in favour of them; the semantic codes `workspace_root_conflict`, `run_outcome_conflict`, `run_already_terminal`, `active_run_not_found`, `invalid_run_status_transition`, and the validation codes stay |
| F2-9 derived tool schemas | `intention-proto` pins `schemars = "=1.2.2"` (already in the lock through `openrouter-rs`) and derives `JsonSchema` on the typed tool inputs; `ToolId::input_schema()` renders the draft 2020-12 schema with model-irrelevant keywords stripped, and `ToolSpec::input_schema()` delegates to it. The six `*_INPUT_SCHEMA` constants (~163 lines) are deleted, so the typed input DTO is the one contract. A module-level `disallowed_types` allow carries its reason because the derive expands its own `serde_json::Map` into a sibling impl block |
| F2-6 endpoint name | The `WIRE_VERSION` constant, the byte, and the versioned endpoint name are gone; the endpoint name is the fixed literal `intention-relay` and the fail-closed typed decode is the only peer protection |
| F6-13 notices apparatus | `THIRD_PARTY_NOTICES.md`, its generator, its template, `NOTICES_PINS`, the Makefile targets, and the CI step are deleted. Nothing publishes crates, and `deny.toml` is the only license and advisory policy |
| F1-5, F1-12, F2-1, F2-13, F7-13 restatement tax | The README drops from 406 to 251 lines and links to the owners instead of restating them; document 03 drops from 629 to 346 lines and carries no changelog; the ceremony clauses, evidence lines, and the three `Dependencies, non-goals, and evidence` headings whose evidence was deleted are gone; the addressing rule is stated once |
| C2-4 one message list | `execute()` owns one `messages: Vec<ModelMessageDto>` and passes it `&mut` through the attempt; `extra_messages` and the merge at request build are deleted |

F2-5 (project/workspace identity) is the one finding this wave does not take: `projects`, `workspace_roots`, the
per-session `ProjectId`/`WorkspaceId` proposals, and the one-root-one-binding resolution stay as they are. It is
recorded as deferred, not as resolved.

Validation of the final tree (`d43fc9a`): `cargo check --workspace --all-targets --locked --all-features` is clean,
`cargo clippy --workspace --all-targets --locked --all-features -- -Dwarnings` is clean, `make verify` is green, and
the suite reports 876 tests passed with 2 skipped.
