# ADR 0051: Per-crate coverage tiers

## Status

Accepted 2026-10-05. It replaces the single base 80% line-coverage threshold and the designated-files mechanism with
per-crate tiers — `core` 75%, `standard` 60%, `edge` 20%, and an exempt 0% tier whose crates are neither collected nor
counted in any aggregate. It authorizes no new test, exclusion, override, or quality gate.

## Scope and supersession

In scope: the line-coverage threshold policy, its per-crate classification and declaration surface, the collected-crate
set, and the workspace aggregate's role.

Out of scope: enabled exclusions and their evidence; branch metrics and the pinned dated nightly report; required feature
profiles; the TDD and outcome-scenario requirements; the `intention-daemon` entry-point exclusion; and recorded M0-M5
evidence.

| Record | Superseded clause | Replaced by |
| --- | --- | --- |
| [ADR 0049](0049-base-coverage-threshold.md) | Decision "One base threshold" items 1-3: the 80% floor for every production crate and for the workspace aggregate, and the statement that no per-crate thresholds exist | Per-crate tier floors under `quality/coverage.toml` `[tiers]` and `[crate_tiers]`: `core` 75%, `standard` 60%, `edge` 20%; the workspace aggregate carries no threshold |
| [ADR 0049](0049-base-coverage-threshold.md) | Decision "Designated files" items 4-6 and invariant 2: the designated-files list, its per-file checks, and the 85% bar | Deleted: there is no designated-files list, per-file bar, or designated threshold key; the tiers are the only numeric classification |
| [ADR 0049](0049-base-coverage-threshold.md) | Invariant 1, "80% floor" | Every collected crate meets the floor of the tier declared for it |
| [ADR 0049](0049-base-coverage-threshold.md) | Invariant 4, "No tier reintroduction": "No per-crate category, tier letter, or 95/90/85 crate threshold returns under another name" | Cancelled: this record reintroduces per-crate tiers by name and percentage under the approved layout, and no clause of this repository bans them |
| [ADR 0049](0049-base-coverage-threshold.md) | Invariant 6 (no override "beyond the designated-files mechanism") and the non-goals clause "no per-crate categories or additional thresholds beyond the base 80% and the designated 85%" | The no-override rule stands: no coverage override, waiver, ratchet, grace period, or grandfather clause is introduced, and the tier ladder replaces the deleted designated-files mechanism |
| [ADR 0049](0049-base-coverage-threshold.md) | Decision item 10: Tauri and TUI keep their existing treatment "instead of an aggregate UI line target" | Tauri and TUI carry the `edge` tier floor in addition to their complete command/event mapping contracts and mandatory fixture-daemon smoke/outcome scenarios |
| ADRs [0035](0035-m5plus-complete-foundation-activation.md), [0038](0038-no-backward-compatibility-and-legacy-removal.md), [0017](0017-build-autopilot-and-plan-focus-continuity.md), [0020](0020-configuration-provider-control-plane-directions.md)-[0034](0034-accepted-m5plus-retained-deferral-directions.md), [0042](0042-project-script-library-for-kernel-cells.md), [0043](0043-instruction-sources-and-system-context.md), [0046](0046-typed-serde-json-contracts.md), and [0048](0048-limits-by-precedent-and-no-content-scanning.md), and the reconciliation ownership map | The clause text ADR 0049 substituted with the "base 80% threshold", "designated files", and "no tier is declared" wording | Those clauses read through this record: a coverage declaration names the crate's tier in `quality/coverage.toml`, exclusions stay exact and reviewable, and the underlying deletions, activation sequence, and removals stand unchanged |

The named clauses of [ADR 0049](0049-base-coverage-threshold.md) are amended in place; its rationale, compatibility,
evidence, and research-provenance records of the 2026-09-30 change stay as written. Every other clause of every listed
record stays as written.

## Decision

**One ladder.** `quality/coverage.toml` declares the numeric ladder in `[tiers]` and assigns every active production
crate and both presentation adapters to exactly one rung in `[crate_tiers]`:

| Tier | Minimum line coverage | Crates |
| --- | ---: | --- |
| `core` | 75% | `intention-storage-sqlite`, `intention-runtime`, `intention-transport`, `intention-storage`, `intention-application`, `intention-domain` |
| `standard` | 60% | `intention-model`, `intention-tools`, `intention-workspace`, `intention-config`, `intention-daemon`, `intention-hooks`, `intention-provider-openrouter`, `intention-provider-generic-chat` |
| `edge` | 20% | `intention-tauri`, `intention-tui`, `intention`, `intention-client` |
| `exempt` | 0% | `intention-types`, `intention-protocol` |

**Collection.** The coverage runner collects every crate whose tier is above zero, one isolated Cargo package invocation
per profile, and the checker enforces exactly that crate's tier floor. A crate whose package still has no executable test
code has no measurable report: the runner prints the skip and the declared floor applies as soon as the crate gains
tests.

**Exemption.** A 0% tier crate is declared in `[crate_tiers]` like any other and is excluded from collection and from
every aggregate denominator. `intention-types` and `intention-protocol` are exempt because they own DTO and wire shapes
whose behavior is exercised through their consumers; their contract fixtures remain mandatory. Changing or removing an
exemption is a reviewed policy change in `quality/coverage.toml`, never a checker allowance.

**Aggregate.** The workspace aggregate report remains an artifact and keeps exercising dependency code in the same
instrumented test process, but it is informational: it prints its line metric over collected crates only, and no
aggregate threshold exists.

**Exclusions, metrics, and tests unchanged.** Enabled exclusions stay exact repository-relative source-file paths with
rationale, owner, and equivalent test evidence; they must resolve under the collected owner's `src` root, appear exactly
once in the coverage report, and are subtracted from that crate's numerator and denominator. Branch coverage, the pinned
dated nightly branch-aware report, and the required feature profiles are unchanged. Critical safety and recovery
branches remain independently mandatory in the scenario tests defined by [architecture
10](../architecture/10-test-driven-delivery-and-verification.md); a passing tier floor never replaces them. The sole
enabled exclusion, `intention-daemon/src/main.rs`, keeps its real-binary equivalent test evidence.

## Rationale

- A single 80% floor treated every crate alike: it inflated test-writing for thin composition, client, adapter, and DTO
  crates, where the number measured ceremony rather than risk, while the durable core carried the same bar as a
  presentation shell.
- Per-crate floors direct effort to the crates whose failure modes matter, and they are legible in one table. The
  exempt declaration keeps pure shape crates outside a number that their consumers' tests would dominate anyway.
- Nothing semantic is weakened: exclusions stay exact and reviewable, branch metrics stay required, and every scenario,
  safety, boundary, and outcome test keeps its own mandate.

## Invariants

1. Tier floors. Every collected crate meets the line-coverage floor of its declared tier.
2. One declaration per crate. Every active production crate and every adapter package declares exactly one tier; the
   architecture checker rejects a crate outside that production set, a missing declaration, and an unknown tier name.
3. Exempt means outside. A 0% tier crate is declared in `[crate_tiers]`, is not collected, and is not counted in any
   aggregate; exemption changes are policy changes.
4. Informational aggregate. The workspace aggregate report has no threshold and never includes an exempt crate's lines.
5. No override. There is no coverage override, waiver, ratchet, grace period, or grandfather clause beyond the
   declared tier ladder and the reviewable exclusion policy.
6. Semantics first. Branch coverage, feature profiles, and the mandatory scenario tests are unchanged and are never
   replaced by a line percentage.

## Compatibility

The change is a policy simplification, not a behavior change: no protocol, DTO, storage, configuration, or wire format
changes, and no production code is rewritten to satisfy it. The policy file, checker, runner, and CI wiring move to the
tiers in the same change. Under the repository's no-backward-compatibility rule ([ADR
0038](0038-no-backward-compatibility-and-legacy-removal.md)), the base and designated keys are deleted in place rather
than preserved as deprecated alternatives; historical milestone records and closeout evidence that mention the A/B/C
tiers or the base threshold stay as history.

## Security and failure behavior

A collected crate below its tier floor fails `make coverage` and `make verify`. An invalid tier table, an unknown tier
name, a crate outside the production set, a missing declaration, an unreported exclusion, or an exclusion owned by an
uncollected crate fails the checker through its typed failure path. Exempt crates carry no numeric gate, so their
contract, redaction, and boundary scenarios remain the only — and still mandatory — evidence. No security-relevant test
or check is removed; the policy change only replaces the numeric classification and its declaration surface.

## Non-goals

No change to what is tested, how tests run, or which gates block; no new exclusion and no weakening of the enabled one;
no override mechanism, waiver, ratchet, grace period, or per-crate escape hatch; no per-file coverage bar and no
aggregate threshold; no change to branch metrics, the pinned nightly toolchain, feature-profile coverage, or adapter
mapping/outcome evidence; no edit to closed milestone records, whose numbers remain history.

## Affected documents

[Architecture 12](../architecture/12-quality-gates-and-makefile.md) owns the current coverage policy and its tier table;
[architecture 10](../architecture/10-test-driven-delivery-and-verification.md) keeps the TTD workflow with the tier
declaration rule; [architecture 11](../architecture/11-implementation-roadmap.md) records the policy change; and
architectures 00-09 replace their current-policy coverage references with the tier policy. [`README.md`](../../../README.md)
and [`AGENTS.md`](../../../AGENTS.md) record the crate-map and new-crate declaration rules, and the [decision
index](README.md) carries this record.

## Evidence

The policy change is accepted only together with: `quality/coverage.toml` declaring the `[tiers]` ladder and the
`[crate_tiers]` assignment, with no base, designated, or adapter keys and the unchanged `intention-daemon/src/main.rs`
exclusion; the coverage checker and runner resolving thresholds and the collected set from those tables; focused
coverage-checker self-tests in `quality/test_check_coverage.py` proving each tier floor, the exempt path, the
informational aggregate, and the typed policy errors; a documentation search receipt showing no current-policy statement
still requires the base threshold or the designated-files mechanism, with closed records named as history; and the gate
suite passing. Gates: `make quick`, `make verify`, `docs-check`, Linux/Windows CI.

## Research provenance

The approved M5+ simplification specification for the coverage policy; the repository policy against bureaucratic
per-crate number categories that classify rather than measure; the single-live-version rule that deletes superseded
mechanisms instead of keeping them readable; and the existing exact-exclusion, branch-metric, and feature-profile
mechanisms reused unchanged.
