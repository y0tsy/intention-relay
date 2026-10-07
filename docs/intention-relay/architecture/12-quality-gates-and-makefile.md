# Quality Gates and Makefile

**Current policy.** The pinned Rust toolchain and external-tool manifest, the root `Makefile`, Cargo configuration and
committed lockfile, machine-readable policies, quality checkers, and the CI workflow are implemented and blocking, and
the policy below is the v1 quality policy. Future milestones extend it only when they add a crate, feature combination,
adapter, quality tool, or an explicitly approved stronger check.

## Scope

The policy covers pinned toolchains and external quality tools; formatting and strict pragmatic linting; per-crate
coverage tiers; required Cargo feature profiles; tests, doctests, documentation, and architecture checks; dependency,
license, advisory, unused-dependency, stale-dependency, and manifest hygiene; the Makefile contract and CI entry point;
and explicit, reviewable exception handling. It complements, and does not replace, [Test-Driven Delivery and
Verification](10-test-driven-delivery-and-verification.md): numeric coverage never replaces contract, architecture,
failure-path, or outcome tests.

## Quality model

- `make quick` is the fast inner-loop signal: tools check, `fmt-check`, the profile-based lint matrix, and the
  default-profile test suite.
- `make check` is the complete non-mutating source gate; `make verify` adds coverage and the dependency/supply-chain
  gates; `make ci` aliases the full gate so local and CI behavior cannot drift.
- `.github/workflows/quality.yml` runs the blocking gate as parallel matrix jobs through the per-job aliases:
  `ci-lint-arch` (formatting, features, lint, docs, architecture) and `ci-test` on Linux and Windows,
  `ci-coverage-default`/`ci-coverage-no-default`/`ci-coverage-all` and `ci-deps` on Linux. Branch protection requires the
  eight resulting status checks.
- CI installs exact tool releases through checksum-verified actions, scopes tools per job, uses `rust-cache`, the mold
  linker on Linux jobs whose flags request it, and sccache for coverage builds; each job writes a job-scoped metrics
  manifest (`quality-run-<job>.json`) that is observational only and never changes a gate's verdict.
- A manual `quality-benchmark` workflow, the opt-in `real-api-e2e` workflow, and a `cache-cleanup` workflow support the
  gate without ever being part of it.
- The gates never install tools, update the lockfile, or resolve dependencies differently from the committed state.

`make help` is the contract for the supported targets, their dependencies, and their mutation status. The principal
targets are `bootstrap-tools` (mutating and networked), `fmt`/`fmt-check`, `lint`, `test`, `docs-check`, `architecture`,
`features`, `coverage` and its per-profile variants, `deps`, `notices`/`notices-check`, `quick`, `check`, `verify`, `ci`,
the `ci-*` job aliases, and the opt-in `e2e-real-api`.

## Reproducible tooling

- `make bootstrap-tools` is the sole installer for external quality tools; it is explicitly mutating and networked, and
  it installs only exact pinned versions.
- `make tools-check` validates the pinned Rust version, required components, and external tool availability with exact
  versions; it is non-mutating and CI scopes it per job through `check_tools.py --scope`.
- Pinned versions and invocation policy live in `quality/tools.toml`; the toolchain is pinned by `rust-toolchain.toml`;
  the lockfile is committed and gates run with `--locked`. A missing or mismatched tool is a typed quality-gate failure,
  never an implicit install.

## Formatting and lint policy

- `rustfmt` is mandatory; `make fmt-check` fails on formatting drift and never modifies files.
- All warnings are errors in every non-mutating verification target, for every required feature profile. `unsafe` is
  denied by default, and production code denies unreviewed `unwrap`, `expect`, `panic`, `todo!`, `unimplemented!`,
  `dbg!`, direct stdout/stderr printing, direct process termination, and memory-forget patterns.
- A lint suppression uses a narrow local scope with a mandatory `reason` that explains why it is safe; test-only
  exceptions are also local and reasoned.
- The strict pragmatic Clippy baseline (Clippy defaults, `nursery`, the selected `pedantic` and `restriction` lints) is
  configured in the root Cargo lint configuration. The policy deliberately does not deny all `pedantic` or all
  `restriction` lints, because some are subjective or ergonomically harmful.
- `make architecture` enforces the architectural protections encoded in `quality/architecture.toml` and implemented by
  `quality/check_architecture.py`: the active crate set, dependency directions, private provider-SDK ownership,
  DTO-only boundaries, no process-CWD fallback, forbidden escape hatches, and the closed ordering-authority set.

## Coverage policy

Coverage is a blocking guardrail from the moment a crate contains production code; there is no grace baseline and no
gradual ramp. Every collected crate's own branch-aware `cargo llvm-cov` report must reach its tier floor; the workspace
aggregate report is informational and never a threshold authority.

`quality/coverage.toml` declares the numeric ladder and assigns every crate:

| Tier | Minimum line coverage | Crates |
| --- | ---: | --- |
| `core` | 75% | `intention-storage-sqlite`, `intention-runtime`, `intention-transport`, `intention-storage`, `intention-application`, `intention-domain` |
| `standard` | 60% | `intention-providers`, `intention-tools`, `intention-workspace`, `intention-config`, `intention-daemon`, `intention-hooks` |
| `edge` | 20% | `intention-tauri`, `intention-tui`, `intention`, `intention-client` |
| `exempt` | 0% (not collected) | `intention-proto` |

- Every required feature profile contributes to coverage where the crate supports that profile; a coverage decrease
  below a crate's tier floor fails `make coverage` and `make verify`.
- Generated code and technically unmeasurable code may be excluded only through a versioned policy entry with
  rationale, owner, and equivalent test evidence, and exclusions cannot hide core runtime, policy, provider
  translation, persistence, redaction, or security logic. Coverage reports are stored as CI artifacts, and test code
  must not inflate the production denominator.
- The one enabled exclusion is `intention-daemon/src/main.rs`: a thin process adapter whose real-binary bootstrap
  fixtures are accepted as equivalent evidence, while all daemon library behavior stays under the `standard` floor.
- `quality/run_coverage.py` normalizes the daemon's feature-profile flags (`seen_effective`): because the daemon report
  always appends `--all-features`, profiles that reduce to that set are skipped instead of producing duplicate reports.
  This is the only deliberate equivalence in the runner and it does not weaken the feature-profile policy.

## Cargo feature-profile policy

`make features` verifies the required profiles: default features, `--no-default-features`, `--all-features`, and the
explicitly versioned critical combinations in `quality/features.toml` (currently none). This is intentionally not an
exhaustive matrix; a change that adds an optional provider, adapter, or feature covers it through one of those profiles,
or adds a critical combination, in the same change.

## Makefile contract

The root `Makefile` is the sole supported orchestration surface for local and CI quality workflows. Recipes use strict
shell behavior and label each command as mutating or non-mutating. `make verify` runs `check`, `coverage`, and `deps`,
then removes only the generated LLVM coverage target; coverage reports remain available for CI upload. `make ci` wraps
`verify` with `metrics-start` and `metrics-finish`, which write `quality/reports/quality-run.json` and a JSONL event
stream for phase, profile, crate, stage, duration, and outcome records. `make check`, `make verify`, and `make ci` fail
rather than modify source, update the lockfile, install tools, or resolve dependencies differently from committed state.

An ordinary `cargo build`, `cargo run`, or destructive `cargo clean` is not an independent required quality gate when
the compile and test contracts are already covered.

## Supply-chain and dependency hygiene

`make deps` and blocking CI run `cargo metadata --locked` as the authoritative lockfile check; `make notices-check`,
which regenerates `THIRD_PARTY_NOTICES.md` from `Cargo.lock`, `quality/about.toml`, and the notice template through
pinned `cargo-about` and fails on drift; `cargo deny check` for advisories, licenses, banned crates, allowed sources,
and duplicate-version policy, using the policy expression in `deny.toml`; `cargo audit` as an independent advisory
source; `cargo udeps` for unused dependencies; `cargo machete` for manifest hygiene; and `cargo outdated` for stale
direct dependencies under the policy in `quality/outdated.toml`.

`THIRD_PARTY_NOTICES.md` is a checked-in generated disclosure artifact, not a hand-maintained license inventory; private
`publish = false` workspace packages stay project-owned code and are excluded. A failed or stale notice generation is a
blocking supply-chain failure, not an inferred pass.

## Required quality-gate failure tests

Controlled fixtures prove that the quality system fails correctly. The intent of each fixture is recorded here; the
fixtures themselves live with their checkers.

| Intentional defect | Gate that fails |
| --- | --- |
| Formatting drift | `make fmt-check`. |
| Compiler or Clippy warning | `make lint`. |
| Unreasoned or broad lint suppression | `make lint` or `make architecture`. |
| Missing/mismatched pinned tool | `make tools-check`. |
| Missing, stale, or hand-edited third-party notices | `make notices-check` and `make deps`. |
| Missing required crate/test-target policy metadata | `make architecture`. |
| Forbidden crate dependency or import | `make architecture`. |
| DTO/SDK implementation leak | `make architecture`. |
| Coverage below a crate's tier floor | `make coverage`. |
| Unapproved coverage exclusion metadata | `make coverage`. |
| Uncovered required feature profile | `make features`. |
| Dependency advisory/license/source/ban/duplicate violation | `make deps`. |
| Unused, stale, or manifest-only dependency | `make deps`. |
| Recognizable fake secret in output fixture | `make test` and `make verify`. |

## Test-first integration

Every production boundary is activated with its machine-readable architecture, test-target, coverage, and feature
policies updated in the same change, together with focused expected-failure architecture fixtures and outcome evidence.
A focused test suite is not a substitute for the full gates.

## Opt-in live-provider e2e

The manual `real-api-e2e` workflow and `make e2e-real-api` run the ignored `real_api_e2e` daemon integration test against
a real provider API, taking the credential only from the repository secret `REAL_API_E2E_PROVIDER_KEY`; the mode is
mutating, networked, and opt-in, and it is never part of the blocking gate. The obligation itself is owned by
[Test-Driven Delivery and Verification](10-test-driven-delivery-and-verification.md), scenario J.

## Slice 1.5 quality evidence

Slice 1.5 (the current-state core) changes the evidence base without lowering it:

- the storage contract suite is rebuilt for the eight-table current-state schema and the one-transaction rule, and the
  event/snapshot/replay contract blocks are deleted with their surfaces;
- the engine tests cover commit-and-publish without events or replays, and tool tests cover one transaction per call and
  the eight wired hook phases;
- protocol contract fixtures cover the reduced current-state surface (current-state snapshots and `run.frame`
  notifications, no cursors or resync);
- deleted-surface tests are deleted without replacement; per-crate coverage tiers in `quality/coverage.toml` stay as
  declared, and a merged crate inherits the strictest source floor.

## Non-goals

An aggregate coverage threshold; per-file coverage bars; denying all `pedantic` or all `restriction` lints; a second
quality toolset or Makefile surface; hidden installs inside the gates; weakening a gate to make a change pass.
