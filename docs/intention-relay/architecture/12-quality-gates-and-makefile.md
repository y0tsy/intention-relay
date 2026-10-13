# Quality Gates and Makefile

**Current policy.** The pinned Rust toolchain and external-tool manifest, the root `Makefile`, Cargo configuration and
committed lockfile, the machine-readable policies, the lint configuration, and the CI workflow are implemented and
blocking, and the policy below is the v1 quality policy. Future milestones extend it only when they add a crate, feature
combination, adapter, or an explicitly approved stronger check.

## Scope

The policy covers pinned toolchains and external quality tools; formatting and strict pragmatic linting; per-crate
coverage tiers; tests, doctests, and rustdoc; dependency, license, advisory, stale-dependency, and manifest hygiene; the
Makefile contract and CI entry point; and explicit, reviewable exception handling. Architecture rules that no mechanism
can decide are review responsibilities rather than gates. The policy complements, and does not replace, [Test-Driven
Delivery and Verification](10-test-driven-delivery-and-verification.md): numeric coverage never replaces contract,
architecture, failure-path, or outcome tests.

## Quality model

- `make quick` is the fast inner-loop signal: tools check, `fmt-check`, lint, and the test suite.
- `make check` is the complete non-mutating source gate; `make verify` adds coverage and the dependency/supply-chain
  gates; `make ci` aliases the full gate so local and CI behavior cannot drift.
- `.github/workflows/quality.yml` runs the blocking gate in four parallel matrix jobs through the per-job aliases:
  `ci-lint-arch` on Linux (formatting, native lint, the Windows cross-target lint, doctests, and rustdoc),
  `ci-coverage-default` and `ci-deps` on Linux, and `ci-test` on Windows. Linux test execution belongs to
  `ci-coverage-default`, which runs the same instrumented suite `make test` would, so no separate Linux test job
  duplicates it. Branch protection requires the four resulting status checks.
- CI installs exact tool releases through checksum-verified actions, scopes tools per job, uses `rust-cache`, the mold
  linker on Linux jobs whose flags request it, the MinGW cross compiler for the Windows-target lint, and sccache for
  coverage builds.
- The `cache-cleanup` workflow prunes every non-`main` Actions cache and every `main` cache older than seven days after
  each merge and on a weekly schedule, so the shared rust-cache/sccache space cannot fill and start failing new writes.
- The opt-in `real-api-e2e` workflow supports the gate without ever being part of it.
- The gates never install tools, update the lockfile, or resolve dependencies differently from the committed state.

`make help` is the contract for the supported targets, their dependencies, and their mutation status. The principal
targets are `bootstrap-tools` (mutating and networked), `fmt`/`fmt-check`, `lint`, `lint-windows`, `test`, `doctest`,
`doc`, `coverage`, `deps`, `notices`/`notices-check`, `quick`, `check`, `verify`, `ci`, the `ci-*` job aliases, and the
opt-in `e2e-real-api`.

## Reproducible tooling

- `make bootstrap-tools` is the sole installer for external quality tools; it is explicitly mutating and networked, and
  it installs only exact pinned versions.
- `make tools-check` validates the pinned Rust version, required components, and external tool availability with exact
  versions; it is non-mutating and CI scopes it per job through `check_tools.py --scope`.
- Pinned versions and invocation policy live in `quality/tools.toml`; the toolchain is pinned by `rust-toolchain.toml`;
  the lockfile is committed and gates run with `--locked`. A missing or mismatched tool is a typed quality-gate failure,
  never an implicit install.
- The pinned stable toolchain is single-sourced: the Python runners read it from `quality/tools.toml`, and
  `make tools-check` fails when a workflow `toolchain:` literal drifts from that value.
- The pinned toolchain also carries the `x86_64-pc-windows-gnu` target, so the cross-target lint has one declared
  source. MinGW (`gcc-mingw-w64-x86-64`) and `nasm` are host prerequisites of that pass alone; CI installs them for the
  lint job, and locally they are needed only to run `make lint-windows` or `make ci-lint-arch` by hand.

## Formatting and lint policy

- `rustfmt` is mandatory; `make fmt-check` fails on formatting drift and never modifies files.
- All warnings are errors in every non-mutating verification target. `unsafe` is
  denied by default, and production code denies unreviewed `unwrap`, `todo!`, `unimplemented!`, `dbg!`, direct process
  termination, and memory-forget patterns. `expect`, `panic`, and direct stdout/stderr printing are allowed: the
  daemon's and terminal's own logs are an observable surface, and a human-readable `expect` message beats a silent
  error path.
- A lint suppression uses a narrow local scope with a mandatory `reason` that explains why it is safe; test-only
  exceptions are also local and reasoned.
- The Clippy baseline denies `clippy::all`, the selected `pedantic` and `restriction` lints, and the five named
  `nursery` lints kept for readability, logic, and clarity (`use_self`, `redundant_clone`, `option_if_let_else`,
  `suspicious_operation_groupings`, `unused_peekable`); it is configured in the root Cargo lint configuration. The
  `nursery` group itself is not denied, because it is unstable and churns between releases, and the policy
  deliberately does not deny all `pedantic` or all `restriction` lints, because some are subjective or ergonomically
  harmful.
- The Windows target is linted beside the native one: `make lint-windows` runs the same Clippy command with
  `--target x86_64-pc-windows-gnu`, so `#[cfg(windows)]` code, Windows-gated dependencies, and Windows-only items are
  checked exactly like their Unix counterparts. Clippy stops before linking, so the pass needs the target's standard
  library and a C compiler for the bundled C dependencies (`libsqlite3-sys`, `libmimalloc-sys`, `aws-lc-sys`, `ring`),
  never a Windows linker; the target is pinned in `rust-toolchain.toml`, and CI installs MinGW for the pass. Local
  `make quick`, `make check`, and `make verify` stay free of that prerequisite.
- Two source rules are enforced by the compiler rather than by a text scan, through the root `clippy.toml` and the
  denied `clippy::disallowed_types` and `clippy::disallowed_methods` lints: production code never names
  `serde_json::Value` or `serde_json::Map` (DTO-first), and no code reaches for the process working directory instead
  of `WorkspaceRoot`. Test targets and in-source test modules carry one reasoned `#[expect]` each where they decode raw
  JSON by shape, and the headless line writer is the single allowed production site; because the attribute is an
  expectation rather than an allowance, it fails as unfulfilled as soon as its last use disappears.
- The structural boundary rules are enforced without a text scan: a crate that names a foreign implementation crate, a
  provider SDK outside `intention-providers`, or a composition-only concrete implementation outside the daemon's
  composition root fails to compile, and `cargo metadata` reports the same edges the manifests declare. Provider-SDK
  ownership, DTO-only public boundaries, the acyclic production dependency graph, and the closed ordering-authority set
  are review responsibilities, checked against the crate map and the dependency manifests rather than by a checker.
- Documentation is verified by rustdoc: `make doc` builds the workspace documentation with `-D warnings`, so a broken
  intra-doc link or an undocumented public item fails the gate. Markdown links, prose counts, tables, code-path claims,
  and cross-document consistency are review responsibilities.

## Coverage policy

Coverage is a blocking guardrail from the moment a crate contains production code; there is no grace baseline and no
gradual ramp. Every collected crate gets one line-only `cargo llvm-cov` pass under the workspace's single feature
configuration, and its own report must reach its tier floor; the workspace aggregate report is informational and never
a threshold authority.

`quality/coverage.toml` declares the numeric ladder and assigns every crate:

| Tier | Minimum line coverage | Crates |
| --- | ---: | --- |
| `core` | 75% | `intention-transport`, `intention-storage`, `intention-engine` |
| `standard` | 60% | `intention-tools`, `intention-config`, `intention-daemon`, `intention-providers` |
| `edge` | 20% | `intention-tui`, `intention-client` |
| `exempt` | 0% (not collected) | `intention-proto`, `intention-test-support` |

- Every workspace crate Cargo declares with a test-executing target carries a tier: the runner reads the locked Cargo
  metadata snapshot and fails on an untiered crate before it collects anything, so a crate cannot enter the workspace
  outside the coverage policy.
- A coverage decrease below a crate's tier floor fails `make coverage` and `make verify`.
- Generated code and technically unmeasurable code may be excluded only through a versioned policy entry with
  rationale, owner, and equivalent test evidence, and exclusions cannot hide core runtime, policy, provider
  translation, persistence, redaction, or security logic. Coverage reports are stored as CI artifacts, and test code
  must not inflate the production denominator.
- The one enabled exclusion is `intention-daemon/src/main.rs`: a thin process adapter whose real-binary bootstrap
  fixtures are accepted as equivalent evidence, while all daemon library behavior stays under the `standard` floor.
- `quality/run_coverage.py` makes one line-only pass per collected crate under the workspace's single feature
  configuration (`--all-features`, which enables the daemon's non-production `test-support` feature); `intention-daemon`
  and `intention-tools` run through `cargo test` so their library harnesses merge. Test executability comes from the
  Cargo metadata snapshot the runner already writes, not from a source-text marker, so a crate whose only tests use an
  async test attribute is collected like any other. A collected crate is skipped only when Cargo declares no
  test-executing target for it and the crate is listed in `quality/coverage.toml` under `[policy].unexecutable_crates`;
  an undeclared skip, a stale declaration, or a collected crate absent from the snapshot fails the run.
- The tool contract families' move into the exempt `intention-proto` is a coverage-position change, not a policy change:
  `quality/coverage.toml` keeps its tiers, floors, and exclusions, and the moved families' cases are carried by
  `intention-proto`'s declared `contracts`, `m3_contracts`, and `protocol_contracts` targets.

## Cargo feature policy

The workspace declares exactly one feature: `intention-daemon`'s non-production `test-support`. Every gate runs the
single configuration `--all-features`, the superset the daemon's feature-gated tests require; a new optional provider,
adapter, or feature extends that one configuration instead of adding a profile matrix.

## Makefile contract

The root `Makefile` is the sole supported orchestration surface for local and CI quality workflows. Recipes use strict
shell behavior and label each command as mutating or non-mutating. `make verify` runs `check`, `coverage`, and `deps`,
then removes only the generated LLVM coverage target; coverage reports remain available for CI upload. `make ci`,
`make ci-lint-arch`, `make ci-test`, `make ci-coverage-default`, and `make ci-deps` are thin aliases for the gate
sequences CI runs: `ci-lint-arch` composes `fmt-check`, `lint`, the Windows cross-target `lint-windows`, the standalone
`doctest` stage, and `doc`, while `check` keeps the doctests inside `test`. `make check`, `make verify`, and `make ci`
fail rather than modify source, update the lockfile, install tools, or resolve dependencies differently from committed
state.

An ordinary `cargo build`, `cargo run`, or destructive `cargo clean` is not an independent required quality gate when
the compile and test contracts are already covered.

## Supply-chain and dependency hygiene

`make deps` and blocking CI run `cargo metadata --locked` as the authoritative lockfile check; `make notices-check`,
which regenerates `THIRD_PARTY_NOTICES.md` from `Cargo.lock`, `quality/about.toml`, and the notice template through
pinned `cargo-about` and compares it byte for byte, falling back to comparing which license every crate reference is
rendered under when the bytes differ only because the local registry cache grouped or attributed identical license texts
differently, so the check does not depend on which machine ran it while a changed graph, version, or license name still
fails; `cargo deny check` for advisories, licenses, banned crates, allowed sources,
and duplicate-version policy, using the policy expression in `deny.toml`; `cargo audit` as an independent advisory
source; `cargo machete` for manifest hygiene; and `cargo outdated` for stale
direct dependencies under the policy in `quality/outdated.toml`.

`THIRD_PARTY_NOTICES.md` is a checked-in generated disclosure artifact, not a hand-maintained license inventory; private
`publish = false` workspace packages stay project-owned code and are excluded. A failed or stale notice generation is a
blocking supply-chain failure, not an inferred pass.

`revue` is an ordinary crates.io dependency of `intention-tui`, pinned at `3.10.0`. Two of its properties are visible
here. Its event loop quits on Ctrl+C after the handler returns, while Ctrl+C is the shared core's layered action, so
`handle_event` calls `App::prevent_default` for the press the core consumes. It also depends on `unic-emoji-char`,
whose whole `rust-unic` family is archived: five `unmaintained` advisories with no safe upgrade
(RUSTSEC-2025-0075, -0080, -0081, -0090, -0098) reachable only through that probe. Those five are the complete
`advisories.ignore` set in `deny.toml`; every other advisory is resolved by upgrading the graph, and entries there
carry the reason the acknowledgement is allowed to stand. Upstream plans to replace the probe's source
(hawk90/revue#950); the acknowledgement goes away with that release.

The terminal binary takes `mimalloc` as its process-wide global allocator, because a terminal frame allocates many
short-lived small strings; it is an ordinary crates.io dependency under the MIT license, disclosed like every other
dependency through the generated `THIRD_PARTY_NOTICES.md`.

The terminal's local theme resolution adds three direct dependencies, and none of them adds a package to the locked
graph: `toml` parses and writes the local `tui.toml`, and the `cfg(unix)` pair `crossterm` (raw mode) and `mio`
(readiness on the terminal's own input) serve the bounded `OSC 11` background probe. `crossterm` and `mio` were already
locked as the backend and event source of `revue 3.10.0`, and `toml` was already locked through the same graph, so the
declared set changed without a new package, license, or advisory entering it.

## Required quality-gate failure tests

Controlled probes and the native failures of the manifests, rustc, Clippy, and the runners prove that the quality system
fails correctly.

| Intentional defect | Gate that fails |
| --- | --- |
| Formatting drift | `make fmt-check`. |
| Compiler or Clippy warning | `make lint`. |
| Windows-only code that fails Clippy or the Windows-target build | `make lint-windows`. |
| Unreasoned or broad lint suppression | `make lint`. |
| Missing/mismatched pinned tool | `make tools-check`. |
| Missing, stale, or hand-edited third-party notices | `make notices-check` and `make deps`. |
| Forbidden crate dependency, provider-SDK leak, or a composition-only implementation outside the composition root | `cargo metadata` and rustc: the offending crate fails to compile. |
| Untyped JSON in a production public contract | `make lint` (the `clippy.toml` `disallowed-types` ban). |
| Direct process working-directory access | `make lint` (the `clippy.toml` `disallowed-methods` ban). |
| Unfulfilled or unreasoned lint suppression | `make lint`. |
| DTO/SDK implementation leak in a public signature | `make lint` and review. |
| Workspace crate with a test-executing target and no coverage tier | `make coverage`. |
| Coverage below a crate's tier floor | `make coverage`. |
| Undeclared or stale coverage skip declaration | `make coverage`. |
| Unapproved coverage exclusion metadata | `make coverage`. |
| Rustdoc warning or broken intra-doc link | `make doc`. |
| Broken documentation example | `make doctest`, also part of `make test`. |
| Dependency advisory/license/source/ban/duplicate violation | `make deps`. |
| Stale or manifest-only dependency | `make deps`. |
| Recognizable fake secret in output fixture | `make test` and `make verify`. |

## Test-first integration

Every production boundary is activated with its crate-map entry, its declared test targets, and its coverage tier
updated in the same change, together with outcome evidence. A focused test suite is not a substitute for the full gates.

## Opt-in live-provider e2e

The manual `real-api-e2e` workflow and `make e2e-real-api` run the ignored `real_api_e2e` daemon integration test against
a real provider API, taking the credential only from the repository secret `REAL_API_E2E_PROVIDER_KEY`; the mode is
mutating, networked, and opt-in, and it is never part of the blocking gate. The obligation itself is owned by
[Test-Driven Delivery and Verification](10-test-driven-delivery-and-verification.md), scenario J.

## Slice 1.5 quality evidence

Slice 1.5 (the current-state core) changes the evidence base without lowering it:

- the storage contract suite is rebuilt for the seven-table current-state schema and the one-transaction rule, and the
  event/snapshot/replay contract blocks are deleted with their surfaces;
- the engine tests cover commit-and-publish without events or replays, and tool tests cover one transaction per call and
  the pre-effect identity rejection;
- protocol contract fixtures cover the reduced current-state surface (current-state session snapshots and
  `SessionFrameDto` notifications, no cursors or resync);
- deleted-surface tests are deleted without replacement; per-crate coverage tiers in `quality/coverage.toml` stay as
  declared, and a merged crate inherits the strictest source floor;
- `intention-tui`'s declared `edge` floor is enforced again: the runner decides executability from Cargo metadata, so
  the crate's async contract suite is collected instead of skipped, and no collected crate can be skipped without a
  policy declaration.

## Non-goals

An aggregate coverage threshold; per-file coverage bars; denying all `pedantic` or all `restriction` lints; a second
quality toolset or Makefile surface; hidden installs inside the gates; weakening a gate to make a change pass.
