# ADR 0049: Base 80% line-coverage threshold and designated-files mechanism

## Status

Accepted 2026-09-30. It deletes the A/B/C coverage tiers and replaces them with a single base line-coverage threshold of
80% for every production crate and for the workspace aggregate, plus a designated-files list whose files must reach 85%.
The designated-files list is empty at adoption; the mechanism exists so a future high-risk file can carry a higher bar.
This record authorizes no new test, exclusion, override, or quality gate.

Amended 2026-10-05 by [ADR 0051](0051-per-crate-coverage-tiers.md): the base threshold, the designated-files mechanism,
and invariants 1, 2, and 4 are superseded by per-crate tiers. The exclusion, branch-metric, feature-profile, and
scenario clauses stand.

## Scope and supersession

In scope: the line-coverage threshold policy, its per-crate classification, and its declaration surface.

| Record | Superseded clause | Replaced by |
| --- | --- | --- |
| [ADR 0035](0035-m5plus-complete-foundation-activation.md) | The Slice 1 "ownership, feature-profile, and coverage-tier declarations for every activated family" and the required-evidence "coverage tiers, fixtures, and outcome evidence" item | Coverage declarations under this record: every activated family falls under the base 80% threshold, designated files (if any) carry 85%, and exclusions stay exact and reviewable |
| [ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md) | The Wave 9 "re-verify all tiers" step, "No new crate, dependency, feature, coverage tier, or exclusion is introduced", and "Coverage tiers are unchanged" | Coverage is re-verified against the base 80% threshold and the designated-files list; the single-version removal program itself is unchanged |
| ADRs [0017](0017-build-autopilot-and-plan-focus-continuity.md), [0020](0020-configuration-provider-control-plane-directions.md)-[0034](0034-accepted-m5plus-retained-deferral-directions.md), [0042](0042-project-script-library-for-kernel-cells.md), and [0043](0043-instruction-sources-and-system-context.md) | The activating-specification item "declare exact crate owners, ..., coverage tiers, ..." (each record's evidence or activation clause) | The activating specification declares coverage under this record: coverage-crate membership, any designated file with its rationale, and any enabled exclusion; no tier is declared |
| ADRs [0046](0046-typed-serde-json-contracts.md) and [0048](0048-limits-by-precedent-and-no-content-scanning.md) | The evidence and ownership wording "re-verified coverage tiers" | Coverage is re-verified against the base 80% threshold; the underlying deletions and removals stand unchanged |
| Reconciliation ownership map | The tier cells whose text is "Existing declared tiers" | Base 80% threshold (ADR 0049) |
| [Architecture 12](../architecture/12-quality-gates-and-makefile.md) | The "Tiered line coverage thresholds" table and every tier policy statement | The base 80% threshold, the designated-files mechanism, and the unchanged exclusion semantics recorded in that document |

The tier mentions that remain in closed milestone records and the "no coverage tier is introduced" clause of ADR 0040
are historical or no-op wording: they are not policy declarations and are left as written.

## Decision

### One base threshold

1. Every production crate must reach 80% line coverage.
2. The workspace aggregate must reach 80% line coverage.
3. There are no per-crate categories, tier letters, or 95/90/85 crate
thresholds.

Items 1-3 are superseded by [ADR 0051](0051-per-crate-coverage-tiers.md): per-crate tiers (`core` 75%, `standard` 60%,
`edge` 20%, `exempt` 0%) set the floors, and the workspace aggregate carries no threshold.

### Designated files

4. A versioned designated-files list names individual source files that must
reach 85% line coverage. The list is empty at adoption.
5. Each entry is a workspace-relative path under a production crate `src` root
with a rationale. The file must exist, appear exactly once in the coverage report, and carry reportable source lines.
6. The mechanism exists so a future high-risk file can carry a higher bar
without reintroducing crate categories. A file enters the list only with its recorded rationale; the list is not a
second tier scheme.

Items 4-6 are superseded by [ADR 0051](0051-per-crate-coverage-tiers.md): the designated-files list, its per-file checks,
and the 85% bar are deleted; the tiers are the only numeric classification.

### Unchanged exclusions, metrics, and tests

7. Enabled exclusions stay exactly as they are: exact repository-relative
source-file paths with rationale, owner, and equivalent test evidence. They must resolve under the owner's `src` root,
appear exactly once in the coverage report, and are subtracted from the owner's numerator and denominator.
8. Branch coverage, the pinned dated nightly branch-aware report, and the
required feature profiles are unchanged.
9. Critical safety and recovery branches remain independently mandatory in the
scenario tests defined by [architecture 10](../architecture/10-test-driven-delivery-and-verification.md); a passing line
threshold never replaces them.
10. Tauri and TUI presentation crates keep their existing treatment: complete
command/event mapping contracts, all mandatory fixture-daemon smoke and outcome scenarios, and required platform CI
evidence instead of an aggregate UI line target. — Amended by [ADR
0051](0051-per-crate-coverage-tiers.md): Tauri and TUI carry the `edge` tier floor in addition to this treatment.

## Rationale

- The A/B/C tiers were arbitrary bureaucratic layers: they classified crates by
category rather than by measured risk, and the three numbers required explanation without changing what was tested.
- One legible base threshold is easier to state, review, and apply, and gives
every production crate the same floor; the designated-files mechanism keeps a higher bar possible for a future high-risk
file without reintroducing crate categories.
- Exclusions remain reviewable and exact. Nothing is weakened to make the base
threshold pass, and no scenario, branch, safety, boundary, or outcome test stops being mandatory.

## Invariants

1. 80% floor. Every production crate and the workspace aggregate meet the base
80% line-coverage threshold. — Superseded by [ADR 0051](0051-per-crate-coverage-tiers.md): every collected crate meets
its declared tier floor, and the workspace aggregate carries no threshold.
2. Designated 85%. Only files on the designated-files list carry the 85% bar,
and every entry carries a rationale. — Superseded by ADR 0051: there is no designated-files list.
3. Exact exclusions. Enabled exclusions remain exact source paths with
rationale, owner, and equivalent test evidence; no exclusion is added, removed, or weakened by this record.
4. No tier reintroduction. No per-crate category, tier letter, or 95/90/85
crate threshold returns under another name. — Cancelled by ADR 0051: per-crate tiers are reintroduced under the approved
layout.
5. Semantics first. Branch coverage, feature profiles, and the mandatory
scenario tests are unchanged and are never replaced by a line percentage.
6. No override. There is no coverage override, waiver, ratchet, grace period,
or grandfather clause beyond the designated-files mechanism and the reviewable exclusion policy. — Amended by ADR 0051:
the designated-files mechanism no longer exists and the no-override rule stands.

## Compatibility

The change is a policy simplification, not a behavior change: no protocol, DTO, storage, configuration, or wire format
changes, and no production code is rewritten to satisfy it. The policy file and checker move to the base and designated
thresholds in the same change. Under the repository's no-backward-compatibility rule ([ADR
0038](0038-no-backward-compatibility-and-legacy-removal.md)), the tiers are deleted rather than preserved as a
deprecated alternative; historical milestone records that mention them stay as history.

## Security and failure behavior

A crate or the workspace aggregate below 80% fails `make coverage` and `make verify`, exactly as a below-tier crate
failed before. A designated file below 85% fails the same gates. The excluded daemon entry point keeps its equivalent
real-binary coverage evidence. No security-relevant test or check is removed; the threshold change only replaces the
numeric classification and its declaration surface.

## Non-goals

No change to what is tested, how tests run, or which gates block; no weakening of any enabled exclusion and no new
exclusion; no override mechanism, waiver, ratchet, grace period, or per-crate escape hatch; no per-crate categories or
additional thresholds beyond the base 80% and the designated 85%; no change to adapter mapping/outcome evidence, branch
metrics, the pinned nightly toolchain, or feature-profile coverage; no edit to closed milestone records, whose tier
numbers remain history.

## Affected documents

[Architecture 12](../architecture/12-quality-gates-and-makefile.md) owns the current coverage policy and the renamed
policy section; [architecture 10](../architecture/10-test-driven-delivery-and-verification.md) keeps the TTD workflow
with a history pointer; [architecture 11](../architecture/11-implementation-roadmap.md) records the policy change; and
architectures 00-09 and 13-25, 27-30 replace their current-policy and future-activation "coverage tier" references with
coverage declarations under this record. The reconciliation ownership map records the replaced tier cells, and the
source-of-truth matrix records the future requirement. [`AGENTS.md`](../../../AGENTS.md) records the declaration rule
for new production crates.

## Evidence

The policy change is accepted only together with: `quality/coverage.toml` declaring the single base threshold and the
designated-file bar with an empty designated-files list; the coverage checker and its self-test
fixtures exercising the per-crate base threshold, the workspace aggregate, the designated-file bar, and the unchanged
exclusion semantics; a documentation search receipt showing no current-policy or future-milestone statement still
requires coverage tiers, and naming the closed records deliberately left as history; and the gate suite passing. Gates:
`make quick`, `make verify`, `docs-check`, Linux/Windows CI.

Amended 2026-10-05: this section records the accepted 2026-09-30 change only. The live policy declares the tier tables
in `quality/coverage.toml` ([ADR 0051](0051-per-crate-coverage-tiers.md)); neither threshold key nor the designated-files
list exists.

## Research provenance

The staged quality/coverage cleanup decision; the repository policy against unbounded bureaucracy and per-crate number
categories ([ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md)); the single-live-version rule that deletes
superseded mechanisms instead of keeping them readable; and the existing exact-exclusion mechanism that the base
threshold reuses unchanged.
