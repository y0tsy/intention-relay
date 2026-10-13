# Over-engineering audit — Intention Relay architecture, roadmap, and process

> **Working artifact, not an approved decision.** Committed at the repository root by owner decision on 2026-10-13.
> Every finding is a recommendation the owner accepts or rejects; nothing here overrides `docs/intention-relay/` or
> `AGENTS.md`. The gates do not read it: `quality/check_docs.py` checks `docs/**` plus `README.md`, `AGENTS.md`, and
> `THIRD_PARTY_NOTICES.md` only.

## How this was produced

Seven read-only audits over the full planned surface, base `a359729` on `main`:

| Lane | Documents audited |
| --- | --- |
| L1 Foundations and process | 00, 01, 02, 12, README, AGENTS.md, `quality/**`, `Cargo.toml` |
| L2 Runtime core | 03, 04, 05 |
| L3 Agent operation | 07, 08, 30, 06 |
| L4 Capability packages A | 15, 18, 19, 20 |
| L5 Capability packages B | 21, 22, 23 |
| L6 Control plane and domains | 25, 28, 29, 09 |
| L7 Roadmap and delivery process | 11, 10, 12, `.factory/skills/**` |

## The framing that justifies every cut below

Intention Relay is a **local-first, single-user** agent application. One local daemon owns runtime and state; a Tauri
desktop UI and a terminal TUI/REPL talk to it over a **local socket** through one typed Rust client. No remote
transport, no multi-user access, no third-party consumers, no deployed users, no externally persisted data. The repo is
a v1 rewrite: M0-M5 and Slices 1, 1.5, 2 are done; M6-M11 and Slices 3-6 are **planned, not built**.

Consequences used throughout:

- **Security is a supplement, not a goal.** The only actor is the local user; the threat model is minimal. Machinery
  that hides information from the user, or models adversaries that do not exist, is a cost with no payoff.
- **Priorities are: (1) speed of scaling and shipping features, (2) end-user UI/UX quality, (3) low maintenance
  burden.** Anything that slows feature work without a user-visible benefit is a candidate for removal.
- **Nothing is distributed**, so process, versioning, and audit machinery that exists for external consumers is
  overhead.

## How to read a finding

Each finding gives: the anchor (`doc:line` or `path:line`), what the document requires, why it burdens the project, a
concrete simpler alternative, and a burden severity (`HIGH` / `MED-HIGH` / `MEDIUM` / `LOW-MED`). Load-bearing items are
listed separately in §9 and must not be cut.

## Status — what the removal branch already resolved

Branch `cleanup/overengineering-removal`, base `a359729`, eight commits:

| Commit | Change |
| --- | --- |
| `eeaa99c` | `refactor(quality)`: relax the lint policy and drop the bypass boilerplate |
| `3aa5b8b` | `build(quality)`: drop `cargo-udeps` and the pinned nightly toolchain |
| `54117be` | `build(deps)`: de-vendor `revue` and adopt upstream 3.10.0 |
| `0231be8` | `build(deps)`: upgrade `openrouter-rs` to 0.19.0 |
| `12ebbf8` + the Part 1 removal commit | `build`/`refactor(quality)`: Clippy carries the two source rules; the architecture and docs checkers and their self-tests are deleted |
| `8e1b3c4` + the matrix-cut commit | `build`/`ci(quality)`: the Windows target is linted from Linux through MinGW, and the blocking matrix drops to four jobs |

Findings this closes:

- **L1-1, L1-2, top burden 1 — resolved** (`eeaa99c`). `expect`, `panic`, `print_stdout`, and `print_stderr` are
  allowed workspace-wide; `nursery` is allowed at priority -1 with five nursery lints re-denied for readability,
  logic, and clarity; `unwrap_used` stays denied. A one-shot codemod deleted 286 bypass attribute lines and rewrote 186
  `PoisonError::into_inner` sites with `.expect(...)`. The 32 remaining `allow`/`expect` sites, across 27 files, name
  other lints (`dead_code`, `too_many_arguments`, `unwrap_used`, `new_without_default`, `large_enum_variant`,
  `must_use_candidate`, `exit`, `await_holding_lock`) and keep their reasons. Policy text now lives at
  `Cargo.toml:28-55` and doc 12, "Formatting and lint policy".
- **L1-11, L7-14, top burden 6, Wave R1 bullet 5 — resolved** (`54117be`). Registry `revue` 3.10.0, with
  `App::prevent_default` carrying the layered Ctrl+C; `vendor/revue` (961 files), the root `[patch.crates-io]` entry,
  the workspace `exclude`, both `.gitignore` entries, the `cargo outdated` patch-free workspace copy in
  `quality/run_deps.py`, and the `vendor/` skip in `check_architecture.py` are deleted. `deny.toml` acknowledges
  exactly the five `unmaintained` rust-unic advisories, each with a reason, and `quality/check_deny_policy.py` now
  requires that set instead of an empty list.
- **L1-10, top burden 3, Wave R1 bullet 3 — partly resolved** (`3aa5b8b`). `cargo-udeps` is gone with the pinned nightly
  toolchain it required; coverage now runs on stable 1.97.1. `cargo deny`, `cargo audit`, `cargo machete`,
  `cargo outdated`, byte-exact notices, and the four-job CI matrix stay.
- **L1-4, L1-5, L1-6, L1-7, L1-8, L1-9, L1-13, L1-14, L1-15, L1-16, L7-8, L7-13, top burdens 4 and 5, Wave R1
  bullet 4, C8 — resolved** (Part 1, commit `12ebbf8` plus the removal commit). `quality/` shrinks by 2,001 lines:
  `check_architecture.py` (1,126), `architecture.toml` (210), `check_docs.py` (288), `test_check_coverage.py` (229),
  and `test_run_coverage.py` (148) are deleted, and no gate runs a meta-test suite. The two source rules that survive
  run natively: `clippy.toml` denies `serde_json::Value`/`serde_json::Map` and
  `std::env::current_dir`/`set_current_dir` through `clippy::disallowed_types` and `clippy::disallowed_methods`, which
  rustc resolves at every use site instead of scanning raw text. Crate classification, the declared dependency-edge and
  external-dependency sets, the named test-target declarations, the composition-name and ordering-suffix bans, the
  documentation link/fence/Mermaid checks, the mirrored `MAX_*`/`MIN_*` constants, the removed-wire-name list, the
  `@todo(core)` placeholder, and the secret-shaped-assignment regex are gone; the boundary rules worth keeping are
  enforced by the manifests, by rustc, and by review. `make doc` (`cargo doc --workspace --no-deps` with
  `RUSTDOCFLAGS=-D warnings`) is all that remains of the documentation gate. The coverage tiers, floors, and
  exclusions stay exactly as they were, with one completeness rule added: a workspace crate with a test-executing
  target and no tier fails `make coverage`, and `intention-test-support` is declared `exempt`.
- **L1-12, Wave R1 bullet 6 — partly resolved** (`8e1b3c4` plus the matrix-cut commit). The blocking matrix is four
  jobs: `lint-arch` on Linux also lints the Windows target and runs the doctests, `coverage-default` and `deps` stay on
  Linux, and `test` runs on Windows only, because Linux test execution already happens inside the coverage run. Branch
  protection requires exactly those four checks. The scheduled cache-pruner stays, so the finding is not closed.

Landed outside the audit's scope: `0231be8` upgrades `openrouter-rs` to 0.19.0, published 2026-10-13, because the
stale-pin gate (`cargo outdated --exit-code 1`) in `make deps` trips on an external release rather than on a defect.

Still open in Wave R1: coverage floors (L1-3, L7-12), the label taxonomy and the agent quotas (L1-17, L1-18), and the
six-job CI matrix (L1-12).

Anchors elsewhere in this file remain as read at `a359729`; doc 12 and doc 03 were rewritten by the branch, so their
line numbers have shifted.

## Top burdens (feature-velocity blockers)

| # | Item | Anchor | Severity |
| --- | --- | --- | --- |
| 1 | `nursery=deny` plus panic/print/expect denied everywhere incl. tests (91 suppressions, 72 files with `reason=`) — **resolved** (`eeaa99c`) | `Cargo.toml:36-56`, `12:53-58` | HIGH |
| 2 | Coverage floors bite from the first production line (75/60/20), no ramp | `12:79-99`, `coverage.toml:14-24` | HIGH |
| 3 | Six-tool supply-chain stack with byte-exact notices while `publish=false` — **partly resolved** (`3aa5b8b`): `cargo-udeps` and the nightly toolchain are gone, the rest stays | `12:130-145`, `Cargo.toml:29` | HIGH |
| 4 | The quality system has its own ~1000-line test suite, blocking in `make check` — **resolved** (Part 1): the meta-suite is deleted | `Makefile:60-62`, `check_architecture.py:817-978` | HIGH |
| 5 | Every new crate/dep is a policy edit with declared↔Cargo equality and named test targets — **partly resolved** (Part 1): only the coverage tier is left, and an untiered crate now fails `make coverage` | `architecture.toml:20-130` | MED-HIGH |
| 6 | Vendored, patched `revue` maintained for one advisory — **resolved** (`54117be`) | `Cargo.toml:22,74-76`, `12:146-160` | HIGH |
| 7 | Atomic-slice rule blocks sequential merges inside one slice | `11:85-89`, `11:299-303` | HIGH |
| 8 | Rot-audit apparatus cites an unrecoverable `/tmp` input | `10:403-418` | HIGH |
| 9 | MCP: five versioned record families + two-phase acquire/invoke CAS | `18:42-141` | HIGH (future) |
| 10 | Kernel checkpoint subsystem for "convenience state" | `20:155-192` | HIGH (future) |

---

## L1 — Foundations and process overhead

The boundary *rules* are mostly sound; the *enforcement layer* is where the overhead lives. `quality/` is ~3,717 lines
of Python/Makefile for a local single-user tool, and `make check` blocks on the checkers' own tests.

1. **Panic/print denials apply to all targets** — `Cargo.toml:36-56` (`expect_used`, `panic`, `print_stdout`,
   `print_stderr`, `unwrap_used`, `todo`, `unimplemented` deny) plus `-Dwarnings` (`run_profiles.py:34`), policy
   `12:53-57`. Required: no `unwrap`/`expect` anywhere, tests included. Burden: 91 suppression sites across `crates/`,
   72 files carrying `reason="..."`, plus `unwrap_or_else(PoisonError::into_inner)` boilerplate ~30x in one test file.
   Simpler: keep denials for production library/daemon paths; relax for `#[cfg(test)]`/`tests/`. **HIGH** (partly
   load-bearing: a panic in the daemon loop kills the session). **Resolved** in `eeaa99c`: `expect`, `panic`, and
   `print_*` are allowed everywhere, `unwrap_used` stays denied.

2. **`nursery = "deny"` workspace-wide** — `Cargo.toml:37`, `12:58`. Every unstable nursery lint must pass; nursery
   changes between nightlies, so toolchain bumps become refactor events. Simpler: `clippy::all` plus the explicit
   hand-picked list already present (`Cargo.toml:38-56`). **HIGH.** **Resolved** in `eeaa99c`: `nursery` allows at
   priority -1 with five named nursery lints re-denied.

3. **Coverage blocks from the first production line, no ramp** — `12:79-99`, `coverage.toml:14-24` (75/60/20 floors).
   75% line coverage on transport/storage/engine is a per-feature tax with no user-visible payoff. Simpler: one
   workspace floor (50-60%) until M11 closes, or advisory coverage with tiers re-enabled at the end. **HIGH.**

4. **Exact dependency-set equality against Cargo metadata** — `architecture.toml:98-130`, `01:226-241`, `12:61-68`.
   Adding one dependency means editing the manifest *and* `[external_dependencies]` in the same change; an undeclared
   or stale entry fails. Simpler: assert only edges carrying a real invariant (adapter, proto, composition-root);
   let Cargo own the rest. **MED-HIGH** (load-bearing for the adapter/proto/composition subset). **Resolved** in Part 1:
   `architecture.toml` is deleted; the edges the manifests declare are the edges Cargo enforces.

5. **Integration test targets pinned by name** — `architecture.toml:20-75`, `check_architecture.py:287-306`. Renaming or
   adding a test file is a policy edit. Simpler: require a test target only for crates with a declared contract
   surface. **MEDIUM.** **Resolved** in Part 1: the named test-target declarations are gone; the crate map states the
   targets.

6. **Ordering-authority name-suffix ban** — `architecture.toml:190-210`, `check_architecture.py:750-783`. No production
   type name may end in `SequenceDto`/`CursorDto`/`PositionDto`/`TicketDto`/`WatermarkDto`/`OrderDto`/`EpochDto`/
   `JournalDto`. A regex policing naming. Simpler: the single-live-schema decision already covers it; keep as a
   documented rule. **MEDIUM.** **Resolved** in Part 1: the suffix ban is deleted.

7. **Composition type-name ownership + selection-pattern presence** — `architecture.toml:166-183`,
   `check_architecture.py:786-814`. `SqliteStorageRepository` may be named only in its crate or the daemon, and the
   string `SelectedProvider` must appear in daemon source. Duplicates the dependency-direction rule. **MEDIUM.**
   **Resolved** in Part 1: both checks are deleted; rustc keeps composition-only crates out of the wrong consumers.

8. **Forbidden patterns scanned as raw text, comments included** — `architecture.toml:78-87`,
   `check_architecture.py:73-74,1090-1104`. Literal `serde_json::{`, `::<Value>`, `Value::as_`, `serde_json::Value`,
   `rusqlite::` must not appear in any `.rs` outside `tests/`. A doc comment explaining the ban fails the build; a
   grouped `use serde_json::{json, ...}` fails though `json!` is legitimate. Simpler: match `use`/type positions and
   skip comment lines. **MEDIUM.** **Resolved** in Part 1: the text scan is gone, and `clippy.toml` denies the two
   shapes the policy keeps at every resolved use site, comments included.

9. **The quality system has its own test suite, blocking in `make check`** — `12:75,120`, `Makefile:60-62`,
   `test_check_coverage.py` (229 lines), `test_run_coverage.py` (148), seven `self_test_*` in
   `check_architecture.py:817-978`, three in `check_docs.py:221-252`. ~1,000 lines of meta-code beside 1,128 lines of
   checker. Simpler: 2-3 fixture cases per checker, or run self-tests on demand. **HIGH.** **Resolved** in Part 1: the
   meta-suite and its checkers are deleted, and `make check` runs no self-tests.

10. **Six-tool supply-chain stack with byte-exact notices** — `12:130-145`: `cargo deny`, `audit`, `udeps`, `machete`,
    `outdated`, `cargo-about` byte-compared with a license-grouping fallback, all blocking in `make verify`. Nothing is
    distributed and `publish = false` (`Cargo.toml:29`), so notices/udeps/machete/outdated produce zero user value.
    Simpler: keep `cargo deny` (advisories+licenses); move the rest on-demand; drop `THIRD_PARTY_NOTICES.md`. **HIGH.**
    **Partly resolved** in `3aa5b8b`: `cargo-udeps` and the nightly toolchain are gone; `deny`, `audit`, `machete`,
    `outdated`, and the notices artifact stay.

11. **Vendored, patched `revue`** — `Cargo.toml:22,74-76`, `12:146-160`. Maintain a vendored 3.9.1 copy with dropped
    tests/benches/examples/docs, stripped manifest, a `quit_key` patch, a removed dependency probe, a workspace
    exclusion, and `PATCH.md` — to avoid one `unic-emoji-char` advisory. Simpler: registry 3.10.0 (which supplies
    `prevent_default`) plus one scoped advisory ignore in `deny.toml`. **HIGH.** **Resolved** in `54117be`, exactly as
    described.

12. **CI matrix, six required checks, cache-cleanup workflow** — `12:22-26`, `Makefile:112-126`,
    `.github/workflows/cache-cleanup.yml`. Four matrix jobs, branch protection, and a scheduled cache-pruner for a repo
    with no external contributors. Simpler: one job running `make verify`; delete cache-cleanup. **MEDIUM.** **Partly
    resolved**: the blocking matrix is four jobs, the Windows target is linted from Linux, and the Linux test job folded
    into coverage; branch protection and cache-cleanup remain.

13. **Docs mirror code constants** — `check_docs.py:87-109`, `12:69-74`. Every `MAX_*`/`MIN_*` named in a
    `**Current policy.**` doc must exist verbatim under `crates/`. Renaming a constant forces a doc edit. Simpler: check
    only in the owning document. **MEDIUM.** **Resolved** in Part 1: the constant-mirroring check is deleted.

14. **Removed-wire-name scan over `crates/` too** — `check_docs.py:24,174-185,206-219`, `12:75`. A removed name cannot
    be mentioned in a history note or comment; the curated list is three names. Simpler: scan `docs/` only. **LOW-MED.**
    **Resolved** in Part 1: the scan is deleted.

15. **`@todo(core)` placeholder-constant check** — `check_docs.py:25-30,153-172`. Regex over prose for a convention
    review already enforces. Simpler: delete. **LOW-MED.** **Resolved** in Part 1: the check is deleted.

16. **Secret-shaped-assignment scan contradicts the stated ban** — `check_docs.py:13-15,254-259` scans sources for
    credential-shaped strings while `AGENTS.md:79` bans content scanning. Repo-level vs runtime, but the two policies
    should be reconciled explicitly. **LOW.** **Resolved** in Part 1: the regex scan is deleted, so the repository no
    longer scans any source text for credential-shaped strings (C8).

17. **19-label taxonomy with per-PR `type:` and per-surface `area:` labels** — `AGENTS.md:91-97`. Label bookkeeping with
    no consumer. Simpler: `type:` only. **MEDIUM.**

18. **Agent-directed quotas** — `AGENTS.md:53` ("target at least five… calls"), `AGENTS.md:58`. Encourages manufactured
    parallel calls on narrow work. Simpler: state the principle, not the count. **LOW-MED.**

19. **Seven mandatory contract-test families per boundary, pre-implementation** — `02:195-207`, `12:96`: wire fixtures
    for every family, malformed/foreign-byte, invalid-shape/invalid-ID, schema-compatibility, compile-time
    forbidden-type proofs, consumer-driven contract tests, redaction. The only consumer of the wire is the repo's own
    `intention-client`, and the single-live-version rule (`02:180-194`) makes compatibility fixtures structurally
    pointless. Simpler: round-trip + malformed-decode + redaction; drop consumer-driven and compatibility. **MED-HIGH.**

20. **Every principle must map to an executable test or documented exception** — `00:150-152`, mirrored by
    `AGENTS.md:64` (new crate needs policy declaration + test target before any production code). Simpler: require
    tests for the eight required v1 outcomes (`00:95-112`) instead of all eleven principles. **LOW-MED.**

---

## L2 — Runtime core (daemon, transport, storage, tools/workspace)

Note: the transport bounds, wire version, schema stamp 4, `tui_settings`, `projection_json`, `workspace_root_conflict`,
and `SESSION_LIST_ROWS` are **already implemented**, so the burdens below are maintenance cost being paid now.

1. **Terminal theme as daemon-owned protocol state** — `03:55-62`, `03:478-486`, `04:122`, `04:128-130`. Required:
   `GetTuiSettings`/`SetTuiTheme` wire ops, four DTOs, a singleton `tui_settings` row, a `[tui]` config key, and a
   schema-stamp bump. A light/dark flag owns two protocol ops, a table, a DTO family, preview/commit/revert semantics,
   and its own tests, for zero runtime behavior. Simpler: keep the theme in the adapter's own small settings file (or
   the `[tui]` key alone). **HIGH** (not load-bearing; the doc says it records no revision and never enters run
   selection).

2. **Two durable homes for the same tool projection** — `04:116-119,130-131`. `messages.projection_json` carries the
   typed projection *and* one `tool_results` row per call is "the single durable home". Every write does two inserts
   whose shapes must stay in sync; every read path can pick a different source. Simpler: one table. **HIGH.**

3. **Pending-turn subsystem** — `04:82-105,126-129`. A `turns` table with a four-state machine, per-turn proposed
   `RunId`/`ConfigRevisionId`, idempotency keys with content comparison (`turn_idempotency_conflict`), `turn.remove`,
   boundary-join ordering, bounded projection with `pending_turns_omitted`. A durable work queue with conflict detection
   for one user typing one prompt at a time. Simpler: at most one queued turn per session; no idempotency key; derive
   run/config at start; keep restart durability. **HIGH.**

4. **Redacting the path and OS error out of tool failures** — `05:95-106,70`. `tool_read_failed` + code-owned message;
   guidance must never carry the absolute root, a symlink target, an OS error string, command details, or file content.
   The only reader is the user whose own file it is; hiding *which* path failed and *why* is a UX regression and forces
   a separate rendering path plus a byte bound. Simpler: return path + OS error verbatim (bounded); `05:91-93` already
   says there is no security boundary. **HIGH** (security-shaped machinery with negative product value).

5. **Byte-bounded snapshot that truncates silently** — `03:75,135-137`, `04:142-144`.
   `MAX_TRANSCRIPT_SNAPSHOT_BYTES` (= 1 MiB − ¼ MiB) caps the snapshot; pending turns trimmed with an `omitted` count;
   paging is an explicit `@todo(core)`. A long session reconnects into a truncated transcript with no scroll-back, and
   the fix is deferred. Simpler: cap by row count and page backwards on scroll. **HIGH.**

6. **Configuration-revision identity and a nine-code storage error taxonomy** — `04:120-121,183-193`. Canonical
   credential-free revisions keyed by `ConfigRevisionId` with idempotent-equal/conflict-unequal semantics, plus nine
   distinct codes each with category/retry/safe-message. Nothing in the UI can act differently on more than two.
   Simpler: two codes (not-found vs unavailable/internal); config revision as a content hash on the run row. **MEDIUM.**

7. **`projects` + `workspace_roots` + one-root-one-identity conflict** — `04:30-35,124-125`. Mandatory `WorkspaceId`
   per session; a root binds to exactly one project for the life of the DB, conflicting proposals rejected
   (`workspace_root_conflict`). No multi-project UI is specified. Simpler: store the canonical root path on the
   session; drop the project entity and the conflict rule. **MEDIUM.**

8. **No transport-level deadline; every caller owns its own bound** — `03:79-82`. Hang-proofing duplicated at every
   call site; each new op must remember its own bound. Simpler: one default request/response deadline with an explicit
   opt-out. **MEDIUM.**

9. **`WIRE_VERSION` byte that never changes** — `03:30,193-197`. The byte stays 1 while the wire evolves in place, so
   the real protection is fail-closed decode; the byte implies build isolation it does not provide. Simpler: delete the
   byte; keep one endpoint name and the decode failure. **MEDIUM.**

10. **One-reconnect budget "restored only by a frame"** — `03:243-247`. Frame-delivery-counting state in the adapter to
    police a local socket, with a dedicated integration test. Simpler: bounded retry with a small attempt count and a
    typed failure. **MEDIUM.**

11. **Hidden production test seams** — `03:258-259`. `intention-test-support` calls "the hidden facade injection seam
    and the daemon's hidden one-connection dispatch seam"; fixture lifecycle rules leak into the architecture doc
    (`03:123-126`). Simpler: a `#[cfg(test)]`-gated constructor or a real fixture binary. **MEDIUM.**

12. **Run registry and cancel with a fallback terminalization path** — `04:195-214`. Two stop paths, five boundary
    points, and late-result suppression, plus a second path where "an active run no registered execution owns is
    terminalized durably `Interrupted` by the host". Simpler: assert the registry entry exists; a missing entry is a
    bug, not a third durable path. **MEDIUM.**

13. **JSON Schema text as a second, hand-maintained contract** — `05:13-14,42-47`. Each tool's `input_schema` is
    code-owned text and must match the typed input family; only a test keeps them aligned. Simpler: derive the schema
    from the typed DTOs, or declare the text the only contract. **MEDIUM.**

14. **A call with no declared timeout has no deadline** — `05:141-147`. A hung command leaves the run live
    indefinitely with only a manual stop. Simpler: one generous default deadline, overridable/disableable.
    (`READER_DRAIN_GRACE` `05:148-150` is genuinely load-bearing liveness.) **MEDIUM.**

15. **Stamp-change history in live policy** — `04:128-133` narrates the 2→3→4 moves. Simpler: state the rule, stop
    recording each bump. **LOW.**

16. **Doc-surface overhead** — `03:576-595` (11-row `@todo(core)` stub table duplicating TUI markers),
    `03:597-607` (`mimalloc` as a process-wide allocator recorded as architecture). Simpler: keep markers in code;
    treat the allocator as a benchmarked change. **LOW.**

---

## L3 — Agent operation (Plan/Build, providers, instructions, VFR/Headroom)

1. **Dual-source-of-truth plan metadata** — `07:88-107`. Hidden YAML frontmatter (`schema_version`,
   `created_at`/`updated_at`, `created_by_run_id`, `status`, `revision`), corruption rejection, model-visible hiding,
   plus a matching committed revision record — the same facts already live in the durable plan record. Simpler: body-only
   markdown; all metadata in the plan row (the daemon already controls it, so "hiding" is free). **HIGH** (revision
   tracking is load-bearing; frontmatter is not).

2. **Mandated FS/DB transactional reconciliation** — `07:76-80`. Distributed-transaction ceremony between SQLite and
   one file for a single-user local daemon. Simpler: commit the row, then write the file; a missing file is a typed,
   re-creatable error. **MEDIUM.**

3. **Orphan `digest` contract** — `07:161` has approval record "its revision, and the digest", but `02:137-143` removed
   canonical digests and `07:88-95` has no digest field. Nothing can produce or read it. Simpler: approve on
   `(SessionId, plan_number, revision)`. **MEDIUM** (contradiction, not just weight).

4. **Eight-state plan lifecycle** — `07:177-192` plus a state-machine test (`07:217`), while `07:194-196` admits the
   vocabulary "is not defined today" and no validator is kept. Simpler: `drafting | submitted | approved` plus a
   revision counter; drop `Revising`/`Superseded`/`Abandoned` and the feedback round-trip. **MEDIUM** (approval pinning
   is load-bearing).

5. **Plan write-denial / `execute` split is not load-bearing** — `07:122,126,128-130,156-157`. Deny `write`/`edit` in
   Plan while keeping `execute`, then admit the whole thing is not a containment guarantee; any shell one-liner
   bypasses it. Simpler: one Build policy; Plan mode = an instruction plus a read-only advertisement. **MEDIUM.**

6. **Advertise tools that are runtime-denied** — `07:132-136` deliberately advertises `write`/`edit` in Plan mode so the
   model sees tools that always fail. Simpler: filter the six-tool advertisement by mode; removes the denial path.
   **LOW-MED.**

7. **Windowing algorithm over-specified** — `08:210-214`. Repeated longest-compressible-tool-result selection, a
   16-char floor, a fixed marker, a placeholder ladder, re-run after every appended result. Simpler: on overflow,
   truncate tool results in order to a fixed budget (or emit a retrieve reference). **MEDIUM** (windowing is
   load-bearing; the algorithm is not).

8. **Three unreachable validation codes** — `08:173-178`: `invalid_tool_definition_name/description/parameters` for
   definitions that `08:176-177` says are code-owned and always present ("no missing-schema failure mode exists").
   Simpler: construct-time validation in the tool registry; no wire codes. **MEDIUM.**

9. **Bespoke reasoning-echo failure code** — `08:181-187`: a 512 KiB per-round bound, a control-character rule, and
   `reasoning_attachment_unrepresentable` that terminalizes the run, refusing truncation. Simpler: omit/trim above a
   small bound, or reuse a generic validation failure. **LOW-MED.**

10. **Retry modeled as durable event choreography** — `08:246-254`: exactly 250 ms, at most two attempts, two durable
    events (`ProviderAttemptFailed(1)`, `RetryScheduled(1,2)`) with a race against interruption. Simpler: keep the idle
    deadline and one retry; let the SDK back off; record one retry fact (or none). **MEDIUM** (idle-deadline is
    load-bearing).

11. **Prompt-cache breakpoint invariants** — `08:219-226`: at most two ephemeral markers, clearing every earlier marker
    each pass, per-driver translation, "a trim never leaves a stale breakpoint". Simpler: one helper that marks the
    instruction-block end and the window tail. **LOW-MED** (caching is worth keeping).

12. **Custom SDK-ownership source analysis** — `08:57-58,275-276`: a custom checker plus source-level ownership
    analysis to keep `openrouter_rs`/`async_openai` in one crate. Simpler: `cargo deny` + the existing crate boundary.
    **MEDIUM** (tooling).

13. **8 kinds × 4 scopes with no editing surface** — `30:31-47`, `30:155-160`. Four daemon-packaged `Deployment` kinds,
    `Custom` at three scopes, `ProjectInstructions`, `Mode`, `Vfr`; no UI/TUI/CLI/control-plane surface may list or edit
    them, so the user hand-edits TOML and `Session` scope is effectively unreachable. Simpler: `AGENTS.md` + one
    user/project `Custom` fragment list + the two reserved contributions. **HIGH.**

14. **Immutable profile revision history** — `30:69-85`: every change to text/order/enabled/scope is a new immutable
    revision identity with no mutation. Nothing consumes old revisions (`30:133` records only the current projection
    identity). Simpler: profile is current config; keep one typed revision identity for the frozen projection only.
    **HIGH** (freeze-at-admission `30:127-133` is load-bearing; revision history is not).

15. **Unconsumed projection fields and a self-contradiction** — `30:105-113` declares `declared_audience` and
    `total_instruction_size` with no reader, while `30:14-18` assigns audience semantics to architecture 21;
    `30:186-188` specifies a "future projection identity" covering audience, contradicting `30:64-67` ("no canonical
    digest or identity layer exists… introduced only with its first real consumer"). Simpler: projection = assembled
    text + ordered source revision references. **MEDIUM.**

16. **Fake-secret regression over non-secret text** — `30:208-217` requires secret-style regression across every
    public/durable/log/activity surface for instruction text, while `30:213-216` says no surface displays fragment text
    at all — there is no leak path. Simpler: one assertion that instruction text is never written to logs/activity.
    **MEDIUM.**

17. **Two new crates and a six-stage pipeline ahead of need** — `06:95-102,113-116,141-145`, while `06:153-159` still
    leaves the language set, thresholds, CCR backend, capacity, retention, compaction, and fallback undecided. Simpler:
    implement expansion and retrieval inside `intention-tools` behind config flags; split crates only if a second
    consumer appears. **MED-HIGH** (Headroom/CCR retrieval is load-bearing).

18. **VFR instruction needs a revision identity; the Plan focus text does not** — `06:44-52,59-60` gives the `Vfr`
    contribution its own revision identity, while the equally reserved `Mode` contribution is a fixed constant
    (`07:140-152`). Simpler: treat both reserved contributions as static daemon-owned text. **LOW-MED.**

---

## L4 — Capability packages A (tool registry, MCP, bridge, kernel)

All four docs are "Approved future design. Not implemented" (`15:3`, `18:3`, `19:3`, `20:3`).

1. **Effect profile for tools that don't exist** — `15:67-91`: a `ToolEffectProfile` flag set and a 12-row mapping
   covering `expand`, `fetch_url`, `ask_user`, `todo`, `plan_submit`, `retrieve`, `sub_agent`, `mcp` while only six
   `ToolId`s are live (`15:33-40`); `15:86` states "No flag requires confirmation." Simpler: delete the profile and
   table; keep the trusted-local prose (`15:126-136`). **MEDIUM** (not load-bearing).

2. **Selection duplicates code-owned schemas** — `15:139-152`: each run's frozen selection embeds every tool's input
   JSON Schema *text* plus an admission-engine revision and forbids rebuilding from the spec match. Simpler: store the
   ordered `ToolId` list plus one tool-layer revision; re-derive specs from code. **MEDIUM** (recording which tools the
   model saw is load-bearing; duplicating schema text is not).

3. **Eight-member outcome taxonomy + seven statuses** — `15:268-296`: `Succeeded/Deferred/DeniedBeforeExecution/
   FailedBeforeExternalEffect/CancelledBeforeStart/InterruptedBeforeStart/ExecutionUnavailable/Partial`, plus
   `ToolLifecycleStatusDto` (7), `ToolResultStatusDto` (3), three interruption codes, six verbatim notices, each needing
   proto/storage/transport/UI/test coverage. Simpler: `Completed | Failed | Cancelled` + `partial: bool` + a reason
   code; keep the six notices. **HIGH** (notices are load-bearing; granularity is not).

4. **Async `sub_agent` deferred-delivery path** — `15:274-278`: `Async` returns `Deferred` with the child handle as a
   plain-text UUIDv4, then the child answer is one durable fact "delivered once into the parent's next fresh model
   request", with rules for an already-terminal parent. Simpler: ship `Sync` only. **MEDIUM** (not load-bearing).

5. **Progress-deadline policy layered under another timeout** — `15:342-360`: `model_stream_progress_timeout_v1` adds
   content-only progress accounting, pause conditions, exactly-one-retry, while `15:348-353` concedes the enforcing
   bound is the provider attempt timeout owned by 08/09. Simpler: rely on the existing attempt timeout. **MEDIUM.**

6. **Five versioned record families for one user's MCP servers** — `18:42-90`: `McpCapabilitySourceV1`,
   `McpDiscoveryV1`, `McpCapabilityRevisionV1`, `McpCapabilitySelectionV1`, `McpInvocationSelectionV1` (~40 fields,
   incl. `private_endpoint_generation_reference`, `private_credential_generation_reference`,
   `acquisition_operation_id`, `attempt_evidence_reference`, `gateway_contract_revision`). Simpler: one user-editable
   connection record plus capability rows keyed by connection+method; keep only the invocation row durable. **HIGH.**

7. **Two-phase acquire-then-invoke with revision CAS** — `18:96-97,107-108,136-141`: the model first `AcquireCapability`
   (with an `accumulated_selection_revision` per step), then `InvokeCapability`; concurrent commits compare predecessor
   revisions and "a loser rereads." A single-writer local daemon has no concurrent writers, and the extra turn is pure
   latency. Simpler: discover at connect/run start, expose methods directly. **HIGH.**

8. **Closed schema families reject real MCP servers** — `18:134-135`: discovery normalizes only into "closed typed
   input/result schema families"; raw-map-only or unsupported schemas "fail before capability registration." Most MCP
   servers publish plain JSON Schema. Simpler: pass validated JSON through under a size bound. **HIGH** (actively
   counterproductive).

9. **Run-owned MCP processes, never shared** — `18:192-193,210-211`: local stdio services start on first call and
   terminate at run end, never shared or reattached, while `18:210-211` accepts they run with ordinary OS authority.
   Cost: respawn + re-handshake every run. Simpler: a daemon-owned per-user process per connection. **MEDIUM.**

10. **Exact-revision gating on every invocation** — `18:145-150`: each call re-validates endpoint, credential
    generation, descriptor, gateway, protocol, and schema revisions. Simpler: validate against the current connection
    record. **MEDIUM.**

11. **Ephemeral grant handshake between components of one local process tree** — `19:45-66`: issuance requires a durable
    reread and yields `BridgeAttachmentGrantV1` (opaque id, daemon epoch, protocol revision, run, model-step position)
    with six expiry conditions and a never-logged rule, while `19:15-16` states the document is "not a privilege
    boundary". Simpler: the kernel is a daemon-spawned child holding a typed client bound to its run; no grant. **HIGH.**

12. **A third identity plus a durable binding record** — `19:75-108`: `BridgeOperationId` distinct from `CorrelationIdDto`
    and `ToolCallId`, with `BridgeOperationV1` (10 fields) and pre-effect `bridge_operation_conflict` on unequal reuse.
    Simpler: one `ToolCallId` at ingress; reuse the turn idempotency key for dedupe. **HIGH.**

13. **New wire surface and DTO family** — `19:34-41,69-73,174-186`: `BridgeSelectionV1` joins the run record, four
    `Bridge*Dto` families added, attachment rides the shared socket with its own handshake. Simpler: call the tool
    service in-process or over a minimal framed pipe. **HIGH** if the kernel is a daemon child; **MEDIUM** otherwise.

14. **Fourth copy of shared cancellation/recovery/partial rules** — `19:152-159,196-206` restate 15's rules; `18:26-31`,
    `19:13-22`, `20:15-23` each re-declare "not a second X". Every future change must be mirrored four ways. Simpler:
    state bridge/kernel specifics once and reference 15. **MEDIUM.**

15. **Nine-field `KernelSelectionV1` plus separate epoch identity** — `20:27-52`: kernel/interpreter/namespace/checkpoint/
    host-request contract revisions, checkpoint policy, projection revision, a script-library reference, and a
    run-scoped `KernelEpochId` distinct from the session-scoped kernel. Simpler: one kernel config (interpreter,
    workspace, script dir). **HIGH.**

16. **Checkpoint subsystem for convenience state** — `20:155-192`: `KernelCheckpointMetadataV1` (12 fields, incl.
    `parent_checkpoint_reference`, `verification_status`, `omission_summary`), a serializer `kernel-state-snapshot-v1`,
    atomic generation promotion, Required/Optional restore, three failure codes — for state `20:155-157` calls "private
    convenience state, not durable application progress". Simpler: ship `checkpoint_policy = Disabled`; the
    `.ir/scripts` library already gives cross-run reuse. **HIGH.**

17. **Session-scoped vs run-scoped kernel contradiction** — `20:27-29` ("One live `KernelEpochId` belongs to exactly one
    admitted `RunId`… a new run receives a fresh epoch") vs `20:253-258` ("A kernel is session-scoped"). Implementers
    cannot tell which lifetime governs disposal/grants/checkpoints. Simpler: pick one (session-scoped kernel, run-scoped
    namespace) and state it once. **MEDIUM** (ambiguity; load-bearing to resolve).

18. **Three-hop kernel→bridge→tool-loop indirection** — `20:118-122,196-202`: kernel host requests require a current
    grant, a new `BridgeOperationId`, then 15's `ToolCallId`; background code may call tools only while the foreground
    grant lives. Simpler: kernel Python calls the daemon client directly with its run context. **HIGH.**

19. **Per-cell script-import evidence** — `20:144-151`: each foreground cell records per-module path/count/size evidence,
    and unrepresentable evidence fails publication with `kernel_script_library_unavailable`. Audit value near zero; the
    failure mode blocks results over metadata. Simpler: debug log or drop. **MEDIUM.**

---

## L5 — Capability packages B (goals/skills/context/memory, provider evolution, branching)

1. **Memory records have two owners** — `21:7-8,301-305` define immutable typed records with provenance/scope/audience/
   supersession/rollback; `28:3-5,160-178` defines the same family (`MemoryKindDto`, scopes, replacement/rollback,
   cards). Simpler: 28 owns the records; 21 only selects references. **HIGH.**

2. **Three compaction/summary mechanisms** — `21:307-313` (immutable compaction), `28:200-214`
   (`ConversationSummaryDto` cumulative working form), `08:193-217` (live window pass compressing tool results). Simpler:
   keep 08's window pass plus one summary record owned by one doc. **HIGH.**

3. **Skill origin/lifecycle lattice is speculative** — `21:158-188` (7 origins with precedence, overridden/revoked/
   archived), `21:239-262` (14 record types, 19 failure codes, catalog revision + opaque token + `has_more`); `21:176-181`
   admits no install/plugin/marketplace path exists, so `ProjectPlugin`/`UserPlugin`/`Builtin`/`Personal` are dead
   taxonomy. Simpler: project + user skill directories, project wins, enabled/disabled only. **HIGH.**

4. **Catalog acceptance machinery contradicts "startup-only"** — `22:170-204` (prepared candidate, pending removal
   requiring explicit accept/reject, degraded read mode, `activation_recovery_required`, exact registry swap) and
   `22:439-460` (nine ordered audit events, tombstones). For a TOML file read at startup on one machine. Simpler: parse
   at startup, fail loudly on invalid/removed entries, restart to apply. **HIGH.**

5. **Provider descriptor/user-kind framework is speculative breadth** — `22:118-132,388-418` (closed protocol-part
   registry, compatibility matrix, per-descriptor private decoders; `responses` + user-kind parser + `SafeHeader`
   deferred to M11 per `11:508`). Nothing consumes it. Simpler: one adapter per provider actually used; a user kind =
   base URL + model + dialect enum. **HIGH.**

6. **Cross-turn reasoning transfer is heavy for its value** — `22:325-357`: `ReasoningHistoryManifestDto`, per-entry
   references/sizes, `ReasoningHistoryBound` audit, `compatibility_id`, whole-or-reject 4 MiB transfer, plus fork
   inheritance (`22:374-386`). The useful same-run echo is already owned by 08. Simpler: store reasoning on the message
   row; send prior assistant text only. **HIGH.**

7. **Provider config fragmented across four documents** — `22:13-15,470-474` hands control plane to 25, session
   selection/protocol to 29, on top of 09, all delivered together in Slice 4 (`11:239-246`). Simpler: one provider
   document. **HIGH.**

8. **Fork lineage requires a migration, which policy forbids** — `23:266-268` mandates an "additive migration" creating
   root lineage for M3/M4 sessions; `00:132-133` and `11:126,275` state there are no migrations and a stamp bump
   recreates the DB. Simpler: delete the migration clause; lineage starts empty. **HIGH** (contradiction).

9. **Regeneration is implemented as a fork** — `23:31-32,48`: "Regenerate response" = user-turn fork +
   `StartForkRunCommandDto`, pulling in preview, snapshot, lineage, idempotency. Simpler: append-only same-session rerun
   bound to a parent `RunId`; keep fork for explicit branching. **HIGH.**

10. **Presentation commands carry operation-id idempotency** — `23:89-102,157-162`: rename/archive/restore get
    `SessionPresentationOperationId`, stale-sequence conflicts, `changed = false`. Distributed-systems machinery for a
    title edit. Simpler: plain last-write-wins commands. **HIGH-MEDIUM.**

11. **Per-model-step manifest + projection records** — `21:14-24,273-299`: a `ContextSourceManifestV1` and a
    `ModelContextProjectionV1` per step with disclosure decisions and omission evidence, beside already-durable
    messages/tool facts. Simpler: one frozen request snapshot per step. **MED-HIGH.**

12. **Audience/disclosure information-flow control** — `21:26-40,281-291`: `declared_audience`,
    `representation_policy_revision`, disclosure policy, safe projections for a threat model with exactly one actor.
    Simpler: no audience dimension. **MED-HIGH.**

13. **Goal scope/link semantics duplicated** — `21:42-57` restates `28:23-30` (`GoalId`/`GoalRevisionId`/`GoalLinkId`,
    explicit durable link). Simpler: 28 owns links; 21 references them. **MEDIUM.**

14. **`SkillSelectionV1` defined twice, differently** — `21:65-71` vs `21:219-229` (same name, different field sets).
    Simpler: one definition. **MEDIUM.**

15. **Four nested provider revision DTOs** — `22:38-96` (`ProviderKindDescriptorRevisionV1`,
    `ProviderProfileRevisionV1`, `ProviderSelectionV1`, `ModelCapabilitySelectionV1`) plus envelope ∩ subset plus a
    driver-contract "compatibility proof". Simpler: one frozen profile snapshot (`kind`, `model`, endpoint, effort).
    **MEDIUM.**

16. **Credential-transport/SafeHeader policy** — `22:141-151,404-411`: bearer or one descriptor-selected validated
    header, `AuthenticationHeaderPolicyV1`. Partly load-bearing (a custom endpoint needs one header) but oversized.
    Simpler: one optional auth-header value on the profile, non-serde/non-`Debug`. **MEDIUM.**

17. **Fork preview + optimistic binding round trip** — `23:139-150,202-209`: the client accepts a preview and binds
    `expected_source_sequence`/`expected_preview_binding`. Simpler: one fork command naming the boundary; the daemon
    validates at commit. **MEDIUM.**

18. **17-field fork snapshot** — `23:190-195`: `policy_decision_references`, `terminal_child_result_references`,
    `historical_config_policy_references`, `safe_usage_provenance`, plus reasoning references (`22:374-386`). Simpler:
    boundary + materialized messages + instruction projection + `workspace_state`. **MEDIUM.**

19. **Paged conversation tree** — `23:164-169`: opaque continuation, 1..=64 page, "live between pages". A single local
    SQLite parent pointer suffices. **MEDIUM.**

20. **Frontmatter/DTO surface wider than the feature** — `21:108-118` (license, compatibility, declared_version,
    validated_metadata, which `21:145-148` declare semantics-free) and `21:78-141` duplicate `28:180-185`'s Skill
    definition. Simpler: name/description/enabled/invocable; one Skill definition. **MED-LOW.**

21. **Duplicated paragraph** — `23:78-84` and `23:184-188` restate the same typed-serde-JSON removal text; `23:229-233`
    grants forks "no base-snapshot size limit" while `11:265` defers deletion/GC to Slice 6. Cleanup only. **LOW.**

---

## L6 — Control plane and domains (config/provider control plane, goals, provider session, config/security/observability)

1. **Controlled live reload as a command/transaction** — `25:30-39`: a validated TOML candidate is re-parsed, committed
   as a new revision in one transaction, applied to fresh runs only; catalog-affecting changes rejected
   `catalog_change_requires_restart`. A second configuration-activation path for a daemon the user starts themselves,
   and a whole class still needs a restart. Simpler: keep restart-only (`09:50-63`); surface "restart to apply".
   **HIGH.**

2. **Credential rotation command with frozen-meaning preflight and driver rebuild** — `25:45-69`: rotation re-reads the
   TOML credential through the private boundary, runs frozen-meaning checks, rebuilds the provider SDK client in place,
   with two new typed errors. Simpler: edit the file and restart. **HIGH.**

3. **Provider health-check service producing typed readiness evidence** — `25:71-82`: a health-check service with its own
   evidence DTO and `provider_profile_revision_id` absence rules, forbidden from doing selection/routing/credential
   testing. Simpler: one user-triggered probe on the live client. **MED-HIGH.**

4. **Model discovery as an independently identified external attempt** — `25:84-93`: discovery gets full attempt
   lifecycle evidence and non-authorizing records. Simpler: refresh the catalog at startup (already happens, `29:116`),
   or accept a typed model ID. **MEDIUM.**

5. **Pricing and budget policy section (vacuous)** — `25:95-100`: an owned pricing/budget policy that cannot gate
   admission, while `29:68-69` states usage records no price, currency, or cost. Simpler: delete until a price feature
   exists. **MEDIUM.**

6. **Raw-TOML and typed configuration editing with server-side credential reconstruction** — `25:114-130`: typed edits
   reconstruct a credential-free candidate server-side and re-inject the retained private credential inside the private
   boundary; raw TOML editing goes through the same reload transaction. Simpler: user edits TOML directly; daemon
   validates at load. **HIGH.**

7. **Profile UI / configuration control plane as a second owned presentation surface** — `25:103-112` vs `29:76-84`:
   two owners for one surface → drift and duplicated redaction rules. Simpler: one owner (29) for the protocol; 25
   references it. **MEDIUM.**

8. **Goal tree DAG with obligatory children gating parent readiness** — `28:25-73`: a DAG with obligatory components; a
   parent is not ready until every obligatory child reaches a terminal user-decision state; child links carry
   `child_revision_at_link` and link identity. A workflow/graph engine for a single user. Simpler: flat goals per
   project with an optional parent pointer for display; no readiness inheritance. **HIGH.**

9. **Three orthogonal state machines plus exception-set propagation** — `28:76-108`: lifecycle + readiness +
   user-decision states, where `AcceptedWithException` carries evidence upward and a parent may be accepted only with an
   exception set explicitly including each inherited child exception. Simpler: one status (active/done/stopped) plus an
   `accepted` boolean. **HIGH.**

10. **`GoalRunSelectionV1` frozen mega-record** — `28:122-127`: leading goal, ordered parent revision chain, obligatory
    component refs, selected gate/template revisions, memory/Skill/role cards and revisions, revealed-record refs,
    target snapshot, bounds. Simpler: store goal_id + revision on the run. **HIGH.**

11. **Verification gates as a user-authored typed template domain** — `28:140-156`: `ReferenceGate`/`ExecutableGate`,
    user-created typed templates with Project/Goal/Session scope, immutable revisions, safe cards, explicit
    create/edit/archive/restore, provenance, plus gate-invalidation rules (`28:103-104`). Simpler: the user marks a goal
    verified, optionally attaching one command they run. **HIGH.**

12. **Working memory as first-class durable records with replacement links and rollback** — `28:158-185`: typed records
    with scopes, revisions, safe cards, explicit replacement/rollback links; the daemon never decides two texts conflict;
    full content only via `retrieve`. Simpler: memory = a markdown file per project/goal the model reads directly.
    **HIGH.**

13. **Skills and Roles as typed narrowing-only domains** — `28:180-186`: Skill = typed procedure with typed refs; Role =
    reusable `sub_agent` template that may only narrow. Simpler: skills as markdown files; roles as a prompt fragment.
    **MED-HIGH** (partly load-bearing: roles feed `sub_agent`).

14. **Model proposals / `RefinementDraftDto` with milestone gating and coalescing** — `28:187-198`: a durable draft per
    owner scope/kind, one pending with evidence coalescing, base-revision conflict semantics, hidden from cards/context.
    Simpler: model suggests an edit in chat; user requests it. **MEDIUM.**

15. **Compaction summary domain with predecessor chain and immutable corrections** — `28:200-215`:
    `ConversationSummaryDto` with contract version, start/end refs, previous-summary ref, cumulative working form,
    correction revisions, fork-stored summary refs. Simpler: one rolling summary text per session/run with its covered
    range. **MED-HIGH** (partly load-bearing).

16. **Goal bounds table and 30 closed error codes** — `28:235-256,258-287`: 12 code-owned limits (256 goals/project,
    64/session, depth 16, 32 children, 32 gates, 128 memory cards…) and 30 goal error variants, against AGENTS.md's
    limits-by-precedent rule. Simpler: keep snapshot size and cycle/depth guards; drop the rest. **MEDIUM.**

17. **Session default + per-turn + fork overrides with 14-field `ResolvedRunProviderSelectionDto`** — `29:30-74`: a
    durable session default, optimistic revision-checked commands, per-turn and fork overrides, one persisted selection
    record with 14 fields. Simpler: one global provider/model setting; session default only. **MED-HIGH.**

18. **Catalog pagination, pending-removal candidates with 30-minute expiry, degraded recovery states** — `29:85-88,100-124`:
    opaque pagination tokens with catalog-revision invalidation; candidate handles, accept/reject, 30-minute expiry,
    four degraded reasons, activation-recovery state. Simpler: startup removes missing profiles; referencing sessions
    show "provider unavailable". **HIGH.**

19. **Data-classification matrix and redaction-crossing law** — `09:134-158`: 5 classes × persist/emit/log duties and a
    7-crossing redaction boundary whose failure is a security defect needing regression coverage, while credentials
    already never enter DTOs (`intention-config/src/lib.rs:541`). Simpler: keep one rule (credentials never cross) and
    one absence test. **MEDIUM** (load-bearing only as the single structural rule).

20. **File-permission warning/enforcement policy** — `09:100-102,197,215-216`: creation/update code warns or refuses
    unsafe permissions per a documented platform policy, with a Unix permission test. Simpler: document the risk; do not
    warn or refuse. **LOW-MED.**

---

## L7 — Roadmap and delivery process

Note: `docs/intention-relay/closeout/` **does not exist**; roadmap `11:19-23` records its deletion in `eeff780` as
"dead historical artifacts". That deletion is the baseline for several findings.

1. **Unfalsifiable "no retroactive change" acceptance criterion** — `11:270-271`: M5+ exit must prove M6/M7/M8/M10/M11
   can begin "without any retroactive protocol, DTO, schema, crate-boundary, migration, or quality-policy change." No
   pre-merge evidence can prove this, and the history already violates it (Slice 2 retrospective corrections,
   `10:299-399`). Simpler: require each remaining milestone to have an accepted activating spec; delete the promise.
   **HIGH.**

2. **Mandatory atomic slices** — `11:85-89,299-303`: slices are approved as one package, "no slice ships half-ready", a
   slice that cannot be completed atomically is not accepted. Slice 3 alone spans three architectures plus a new crate
   (`11:212-238`). Simpler: permit sequential merges inside one slice under one spec; drop atomic acceptance. **HIGH.**

3. **Per-slice activating specification** — `11:13-14,88-89`: every slice needs its own accepted activating spec at
   milestone start. Simpler: one activating spec per milestone (M5+), with slices as sections. **MED-HIGH.**

4. **Open-decision ledger with anchor + base SHA + re-audit verdict** — `11:305-326`: 11 rows each pinning a live code
   anchor, the base commit it was read at, and a five-lane re-audit stamp (`re-audited: L5 79082b4`), plus a non-entry
   paragraph (`11:328-333`). Every doc move re-invalidates anchors. Simpler: a plain "Known deferrals" list (fact +
   owning milestone); git history holds anchors. **HIGH.**

5. **Process locks dead UI in place** — `11:316-326` keeps owner-deferred stub facts as permanent ledger rows, and
   `12:72-73` has `docs-check` verify that a placeholder constant named by a `@todo(core)` sentence exists under
   `crates/`; the stub tabs still ship (`10:261`). Simpler: delete the stubs and their rows; drop the placeholder check.
   **MED-HIGH.**

6. **Rot-audit apparatus cites ephemeral input** — `10:403-418`: the whole corrective pass is defined by
   `/tmp/rot-audit_2.md` (MR-1…MR-31), and ~100 lines then cite MR ids, wave names, and lane verdicts
   (`10:423-542`). The authoritative input is unrecoverable. Simpler: record each correction where it lands (owner doc)
   with no MR ids, wave labels, or `/tmp` references. **HIGH.**

7. **360 of 586 lines of doc 10 are closed-slice evidence** — `10:203-562` (including the 81-case render enumeration
   `10:239-262` and per-commit test-name lists `10:423-512`), contradicting the rationale that deleted `closeout/` as
   "dead historical artifacts" (`11:19-23`). Simpler: keep current policy (`10:1-201`) only. **HIGH.**

8. **New-crate / new-dependency tax** — `10:76-77` requires every production crate's test targets before
   implementation; `12:61-68` compares declared edges/external deps/named test targets for equality. Simpler: keep
   role/responsibility classification; flag undeclared deps instead of failing stale declarations; drop the named
   test-target assertion. **MEDIUM.**

9. **Ten required test layers plus thirteen mandatory architecture rules** — `10:33-44,48-63`: the taxonomy is an
   obligation per crate. Simpler: name the five layers mapping to real failure modes (contract, storage, runtime,
   adapter, outcome). **MEDIUM.**

10. **Recorded live-provider evidence obligation** — `10:165-190`, `11:417-420`: a manual, non-hermetic run must record
    date, commit, provider, model, and workflow URL, plus a dedicated workflow. Simpler: keep the `#[ignore]`d test as a
    developer tool; drop the recorded-evidence and workflow obligations. **MEDIUM.**

11. **M8 mandates speculative decisions for v1** — `11:391-410`: M8 must close daemon-upgrade coordination,
    stale-listener recovery, idle shutdown (`11:404-405`), TOML include/import policy, non-Unix config permissions,
    diagnostic-export policy (`11:406-407`), and decode-time DTO validation enforcement (`11:402-403`). Simpler: declare
    them explicit v1 non-goals. **HIGH.**

12. **Blocking coverage floors with no grace baseline** — `12:79-97`: floors bite from the first production line and any
    decrease fails `make coverage`. Simpler: enforce at milestone acceptance, or on changed lines only. **MEDIUM.**

13. **The quality system is itself a product** — `12:165-190` requires 19 controlled failure fixtures, backed by ~11
    Python checkers (`check_architecture.py` 49KB) and `make quality-tests` in `make check`. Simpler: keep
    expected-failure fixtures for gates that have caught something (lint, architecture, notices). **MED-HIGH.**
    **Resolved** in Part 1: the Python checkers, the meta-suite, and `make quality-tests` are gone, and the failure table
    now records the native failures of the manifests, rustc, Clippy, the coverage runner, and rustdoc.

14. **Vendored UI toolkit gated on an upstream issue** — `12:146-159`: `vendor/revue` with `PATCH.md` and a deletion
    trigger (`hawk90/revue#950`) exists mainly because the advisory policy forbids acknowledging `unic-emoji-char`.
    Partly load-bearing (the Ctrl+C fix is upstream in 3.10.0 per `12:148-150`). Simpler: a scoped, time-boxed advisory
    exception in `deny.toml`. **MEDIUM.** **Resolved** in `54117be`: registry 3.10.0 plus the scoped advisory set in
    `deny.toml`.

15. **Planning pipeline per milestone** — `SKILL.md:87-99` (mandatory option/trade-off briefs), `:108-138` pre-draft,
    `:140-156` consolidated draft, `:158-172` spec, plus `checklists.md:41-54` decision-record template and five
    checklist groups (`checklists.md:56-101`). The owner is the only decision-maker. Simpler: one decision table plus one
    spec. **MEDIUM.**

16. **Dual-OS CI on every slice** — `11:283-285`, `12:22-24` require Linux/Windows per activated slice and six blocking
    checks. Partly load-bearing (AGENTS.md names "supported platforms"). Simpler: run Windows at milestone acceptance
    only. **LOW-MED.**

17. **Dependency graph overstates edges** — `11:36-43`: `P --> F`, `P --> H`, `P --> I` duplicate the `K -->` edges (K
    is downstream of P). Simpler: draw only direct edges, with K as the single post-M5 gate. **LOW.**

18. **Roadmap doubles as changelog** — `11:69-333` is 265 lines for one milestone; the single Slice 2 bullet runs
    `11:127-211` and restates UI detail already owned by docs 03/10, while `11:7-8` claims the roadmap owns "order,
    dependencies, activation, and status only". Simpler: enforce that claim; link owner docs. **MEDIUM.**

---

## 8. Cross-document contradictions (fix before implementation — cheapest now)

| # | Contradiction | Anchors |
| --- | --- | --- |
| C1 | Approval records a plan `digest`, but canonical digests were removed and no digest field exists | `07:161` vs `02:137-143`, `07:88-95` |
| C2 | Instruction projection identity covers audience, but "no canonical digest or identity layer exists" | `30:186-188` vs `30:64-67` |
| C3 | Fork lineage mandates an additive migration, but there are no migrations and a stamp bump recreates the DB | `23:266-268` vs `00:132-133`, `11:126,275` |
| C4 | Kernel is run-scoped (fresh epoch per run) in one place and session-scoped in another | `20:27-29` vs `20:253-258` |
| C5 | `SkillSelectionV1` is defined twice with different field sets | `21:65-71` vs `21:219-229` |
| C6 | Memory records have two owners with overlapping semantics | `21:7-8,301-305` vs `28:3-5,160-178` |
| C7 | Profile UI / configuration control plane has two owners | `25:103-112` vs `29:76-84` |
| C8 | Secret-shaped source scan contradicts the repo's ban on content scanning — **resolved** in Part 1: the scan is deleted | `check_docs.py:13-15,254-259` vs `AGENTS.md:79` |
| C9 | Roadmap cites `docs/intention-relay/closeout/`, which was deleted | `11:19-23` vs absent directory |

---

## 9. Load-bearing — do NOT cut

**Foundations / runtime**
- Single live version; one integer schema stamp with discard-and-recreate, no migration chain (`02:178-194`,
  `00:129-131`, `04:109-112`).
- "No canonicalization/digest until a real consumer exists" (`02:137-141`).
- DTOs only at the three physical boundaries; domain types internally (`02:229-241`).
- Closed six-variant `ToolId` with no registry (`01:44-46`).
- One composition root and the acyclic dependency direction (`01:137-141`, `architecture.toml:98-130`).
- `WorkspaceRoot` as an addressing anchor, not a security boundary; no symlink parser or containment (`05:57-71,179-184`).
- No handshake, method table, version negotiation, or capability DTOs; the request enum is the protocol
  (`03:29-31,43-44,190-191,347-349`).
- No cursors, no event tail, no resync; subscription returns current state and reconnect re-reads
  (`03:107-113,205-209,346`).
- One connection carries requests, replies, rejections, and frames (`03:88-92`).
- Recovery marks unfinished runs `Interrupted` and resumes nothing (`03:286-299`, `04:219-227`).
- One direct typed tool sequence with no phase dispatch, registry, or extension points (`05:154-163`).
- `ToolDispatchOutcome { completed | interrupted }` (`05:38-40`) and typed `ToolProcessStatus` (`05:136-139`).
- `READER_DRAIN_GRACE` liveness (`05:148-150`).

**Agent operation**
- Instruction freeze-at-admission (`30:127-133`).
- Plan approval pinning one exact revision (`07` invariant 6).
- Provider idle deadline (`08`); windowing/context-window pass (`08:193-217`).

**Capability packages**
- Trusted-local statement with no sandbox/privilege separation and the explicit "not OS isolation" warning
  (`15:126-136`, `20:255-258`).
- One capability path and the refusal of a second registry/tool collection (`15:21-31`, `18:20-24`, `19:11-22`).
- Three admission outcomes with no discretionary authorization layer (`15:171-177`).
- The 64 KiB output bound (`15:265`).
- Closed failure-code sets and credential-free DTOs (`18:161-176`, `19:209-217`, `20:271-280`).
- Progressive disclosure card→revision→body (`21:190-211`); immutable Skill/selection freezing (`21:231-237`).
- `ReasoningDelta` category and `responses` `store: false` / no remote continuation (`22:240-250,279-283`).
- Fork boundary eligibility + text-only `fork-model-context-v1` (`23:55-69`); `WorkspaceStateDto::Unverified`
  (`23:86-87`).

**Process**
- Contract-first delivery for the shared local wire (`10:11-29,31-44`).
- The M5+ foundation gate ordering (`11:71-81,25-54`).
- "No second runtime, registry, persistence authority, or sandbox" (`11:286-287,479`).
- Limits-by-precedent with per-value rationale (`09:112-133`); logs are diagnostics, committed rows are the audit
  (`09:178-186`).
- `make quick` / `make verify` split (`Makefile:105-110`).
- Result-oriented outcome scenarios A–I (`10:102-163`).

---

## 10. Prioritized remediation (max velocity gain, min risk)

**Wave R1 — process and gates (biggest velocity win, no product behavior change)**
- Drop `nursery` from deny; relax `expect_used`/`panic`/`print_*` for tests only (`Cargo.toml:36-56`). — **done**
  (`eeaa99c`), relaxed workspace-wide instead of tests-only.
- Move coverage floors to milestone acceptance, or changed-lines only (`12:79-99`).
- Reduce supply chain to `cargo deny` + on-demand rest; drop `THIRD_PARTY_NOTICES.md` (`12:130-145`). — **partly done**
  (`3aa5b8b`): `cargo-udeps` and the nightly toolchain are gone; the notices artifact and the other tools stay.
- Remove `make quality-tests` from the blocking gate (`Makefile:60-62`). — **done** (Part 1): the target, the checkers,
  and their self-tests are deleted.
- `revue` to registry 3.10.0 + scoped advisory ignore (`Cargo.toml:74-76`, `12:146-160`). — **done** (`54117be`).
- One CI job running `make verify`; delete cache-cleanup (`12:22-26`). — **partly done**: the blocking matrix is four
  jobs with the Windows target linted from Linux; one job and the cache-pruner remain.

**Wave R2 — contracts and versioning (remove ceremony with no consumer)**
- Seven contract-test families → round-trip + malformed-decode + redaction (`02:195-207`).
- Remove the orphan plan `digest`, `declared_audience`, `total_instruction_size` (C1, C2).
- Drop immutable profile-revision history; keep freeze-at-admission (`30:69-85`).
- One durable home for the tool projection (`04:116-119`).

**Wave R3 — security-shaped machinery (restore UX)**
- Return path + OS error in tool failures (`05:95-106`).
- Drop fake-secret regression for instructions; keep one "never logged" assertion (`30:208-217`).
- Collapse the 5×3 classification matrix to one rule + one absence test (`09:134-158`).
- Drop file-permission policy (`09:100-102,197,215-216`).

**Wave R4 — future packages (rewrite while unbuilt; cheapest now)**
- MCP: one connection record + direct invoke; pass through JSON Schema (`18:42-141,134-135`).
- Bridge: delete grant + `BridgeOperationId`; kernel is a daemon child (`19:45-108`).
- Kernel: `checkpoint_policy = Disabled`; resolve the scope contradiction (C4) (`20:155-192,27-29`).
- Goal domain: flat goals + one status; drop DAG readiness, exception sets, gate templates, memory domain
  (`28:25-256`).
- Provider catalog: startup-only; drop pending-removal/expiry/degraded states (`22:170-204`, `29:100-124`).
- Regeneration as an append-only same-session rerun (`23:31-32,48`).

**Wave R5 — roadmap and docs**
- Ledger → "Known deferrals" (fact + owner); delete evidence sections of doc 10 (`11:305-326`, `10:203-562`).
- Delete atomic-slice and unfalsifiable-exit rules; one activating spec per milestone (`11:85-89,270-271`).
- Declare M8's speculative decisions explicit v1 non-goals (`11:391-410`).
- Fix C3, C5–C9; move the roadmap's restated UI detail into owner docs (`11:69-333`).

---

## 11. Method note

Findings are anchored to document line numbers at `a359729`; they are the auditors' reading, not owner decisions.
Severity is a burden estimate, not a correctness verdict. Anything marked load-bearing in §9 should be preserved
unless the owner explicitly changes the underlying decision. Contradictions in §8 should be resolved before the
affected milestone activates, because they are cheapest to fix while the code does not yet exist. The status tags and
the status section above record what branch `cleanup/overengineering-removal` already removed.
