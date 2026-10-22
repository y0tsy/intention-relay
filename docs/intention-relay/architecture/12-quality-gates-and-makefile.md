# Quality Gates and Makefile

**Current policy.** The pinned Rust toolchain and external-tool manifest, the root `Makefile`, Cargo configuration and
committed lockfile, the machine-readable policies, the lint configuration, and the CI workflow are implemented and
blocking, and the policy below is the v1 quality policy. Future milestones extend it only when they add a crate, feature
combination, adapter, or an explicitly approved stronger check.

## Scope

The policy covers pinned toolchains and external quality tools; formatting and strict pragmatic linting; per-crate
coverage floors; tests, doctests, and rustdoc; dependency, license, advisory, and manifest hygiene; the Makefile
contract and CI entry point; and explicit, reviewable exception handling. Architecture rules that no mechanism
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
  duplicates it. The Windows job runs `PLATFORM_TESTS`, the subset whose behavior differs by platform, because the
  Linux run already covers the rest and the Windows-target cross-lint compiles every Windows code path. Branch
  protection requires the four resulting status checks.
- CI installs exact tool releases through checksum-verified actions, installs per job only the tools that job runs,
  and caches per phase according to what that phase reuses. `lint-arch`, `test`, and `coverage-default` build a target
  tree and keep it warm under one stable key per phase, so each run replaces its phase's entry instead of adding
  another generation and the repository cache budget is not consumed by copies of the same tree. The coverage job's
  entry is its instrumented tree: `cargo llvm-cov --no-report` leaves that tree in place, so a restored cache turns the
  gate's build into a no-op, and sccache remains the fallback that serves the compiled objects when the tree is cold.
  `deps` compiles nothing and needs only a lockfile-keyed crate-source entry. `mold` comes from its own pinned action,
  and only the jobs whose flags select it install it and pass `-fuse-ld=mold`, so it is never made the system default
  linker; the Windows job selects LLD with `-C linker=` instead of the MSVC `link.exe`. The MinGW cross compiler and
  `nasm` remain host prerequisites of the Windows-target lint alone.
- The `cache-cleanup` workflow prunes every non-`main` Actions cache and every `main` cache older than seven days after
  each merge, on a weekly schedule, and on demand, so the shared cache space cannot fill and start failing new writes.
- The opt-in live-provider run is a local target, `make e2e-real-api`, and never part of the gate.
- The gates never install tools, update the lockfile, or resolve dependencies differently from the committed state.

`make help` is the contract for the supported targets, their dependencies, and their mutation status. The principal
targets are `bootstrap-tools` (mutating and networked), `fmt`/`fmt-check`, `lint`, `lint-windows`, `test`, `doctest`,
`doc`, `coverage`, `deps`, `quick`, `check`, `verify`, `ci`, the `ci-*` job aliases, and the
opt-in `e2e-real-api`.

## Reproducible tooling

- `make bootstrap-tools` is the sole installer for external quality tools; it is explicitly mutating and networked, and
  it installs only exact pinned versions.
- `make tools-check` validates the pinned Rust version, the pinned components, and every pinned tool at its exact
  version; it is non-mutating. Each gate validates the subset it runs by passing its pins to `check_tools.py`
  (`--component`, `--tool`), so a gate fails on a missing tool it needs and never on one it does not.
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
gradual ramp. `make coverage` runs the instrumented workspace suite once, then writes one JSON report and checks every
floor against it. `cargo llvm-cov report` merges the whole profiling set on each invocation, so asking it for one crate
at a time pays that merge once per crate; the report carries every file's line summary, and a crate's line coverage is
the covered-over-total ratio over its own files, so `quality/check_coverage_floors.py` computes the floors from the one
artifact instead. The same artifact is the CI upload.

`PLATFORM_TESTS` in the root `Makefile` selects the suites whose outcome can differ on Windows, and it is the whole
set the Windows CI job runs: the named-pipe transport (`intention-transport`), the Windows process API and process
lifecycle boundary (`intention-tools`, `intention-test-support`), the `APPDATA`/`LOCALAPPDATA`/`XDG_*` resolution and
native absolute-path validation in `intention-client`, `intention-config`, and `intention-proto`, and the daemon's
cross-process `client_e2e` suite, which is the one run that proves a real daemon serves over that transport. The
remaining suites — `intention-storage`'s SQLite contracts, `intention-providers`, `intention-engine`, `intention-tui`,
and the daemon's platform-independent suites — assert logic no platform-conditional branch reaches, so the Linux
coverage run verifies them once and the cross-target lint compiles them for Windows. A new test that reaches a
platform-conditional branch belongs in `PLATFORM_TESTS`; one that does not stays out.

The instrumented build tree is a cache, not a scratch artifact: `cargo llvm-cov --no-report` leaves it in place, so a
second `make coverage` reuses it and only recompiles what changed. `make coverage-artifacts-clean` reports its size and
removes it, and is not part of any gate.

`COVERAGE_FLOORS` in the root `Makefile` is the whole policy — one `crate=floor` pair per measured crate:

| Floor | Crates |
| ---: | --- |
| 75% | `intention-transport`, `intention-storage`, `intention-engine` |
| 60% | `intention-tools`, `intention-config`, `intention-daemon`, `intention-providers` |
| 20% | `intention-tui`, `intention-client` |

- A coverage decrease below a crate's floor fails `make coverage` and `make verify`.
- The floors are a guardrail over the crates that carry production logic, not a registry every crate must be entered
  into. A crate absent from `COVERAGE_FLOORS` is not measured; adding one to the list is how a new crate becomes
  measured, and the crate map entry ([01 Workspace and Crate Map](01-workspace-and-crate-map.md)) states its
  responsibility in the same change.
- There is no exclusion mechanism. `intention-daemon`'s `main.rs` is a thin process adapter whose lines sit inside the
  daemon's own 60% floor rather than being subtracted from it, so no crate needs a carve-out and none exists.
- One line-only `cargo llvm-cov` run covers the workspace under its single feature configuration (`--all-features`,
  which enables the daemon's non-production `test-support` feature), and every measured crate's report comes from that
  one run. A floor whose crate contributes no file to the report fails the gate rather than being skipped, so a renamed
  or removed crate cannot drop out silently. Coverage reports are stored as CI artifacts.
- `intention-proto` and `intention-test-support` carry no floor: the first is types and contract fixtures, the second is
  the non-production fixture crate, and neither carries product logic that a line floor would protect.

## Cargo feature policy

The workspace declares exactly one feature: `intention-daemon`'s non-production `test-support`. Every gate runs the
single configuration `--all-features`, the superset the daemon's feature-gated tests require; a new optional provider,
adapter, or feature extends that one configuration instead of adding a profile matrix.

## Makefile contract

The root `Makefile` is the sole supported orchestration surface for local and CI quality workflows. Recipes use strict
shell behavior and label each command as mutating or non-mutating. `make verify` runs `check`, `coverage`, and `deps`;
the instrumented coverage tree stays on disk so a later run reuses it, and `make coverage-artifacts-clean` reports its
size and removes it when the four gigabytes are wanted back. `make ci`,
`make ci-lint-arch`, `make ci-test`, `make ci-coverage-default`, and `make ci-deps` are thin aliases for the gate
sequences CI runs: `ci-lint-arch` composes `fmt-check`, `lint`, the Windows cross-target `lint-windows`, the standalone
`doctest` stage, and `doc`; `ci-test` runs `PLATFORM_TESTS` and the doctests on Windows; `ci-coverage-default` runs the
full instrumented suite; and `check` keeps the doctests inside `test`. `make check`, `make verify`, and `make ci`
fail rather than modify source, update the lockfile, install tools, or resolve dependencies differently from committed
state.

An ordinary `cargo build`, `cargo run`, or destructive `cargo clean` is not an independent required quality gate when
the compile and test contracts are already covered.

## Supply-chain and dependency hygiene

`make deps` and blocking CI run `cargo metadata --locked` as the authoritative lockfile check; `cargo deny check` for
advisories, licenses, banned crates, allowed sources, and duplicate-version policy, using the policy expression in
`deny.toml`; and `cargo machete` for manifest hygiene.

`cargo deny` is the single advisory and license authority: it reads the RustSec database and the license policy in
`deny.toml`, which is what a separate `cargo audit` pass would re-read. Nothing is distributed — every workspace package
is private (`publish = false`) — so no notices artifact exists, and `deny.toml` is the only license and advisory policy
in the repository. Stale direct dependencies are not a gate either: a
dependency releasing a patch would fail the build without any change to this repository, so freshness is dependabot's
work rather than a blocking check.

Duplicate versions are reviewed, not eliminated. Every duplicate family in the graph is owned by the two provider SDKs
and their HTTP/TLS stacks, and neither SDK offers a release that would collapse one: `async-openai` 0.42.2 requires
`reqwest` 0.13 with `base64` 0.22 and `getrandom` 0.3, while `openrouter-rs` 0.19.0 requires `reqwest` 0.12 with
`thiserror` 1 and `dotenvy`, whose macro brings `syn` 1; `futures-macro` brings `syn` 3, `uuid` brings `getrandom` 0.4,
and `hyper-util` brings `base64` 0.23. `deny.toml`'s `[bans] skip` list carries each one with its owning chain, and
`cargo deny` fails on any duplicate that list does not name, so a new family is a blocking finding rather than an
accretion. The diagnostic needs `--workspace`: the root `Cargo.toml` declares `default-members`, so a bare
`cargo tree -d` inspects only `intention-proto` and `intention-config` and reports nothing.

That `deny.toml`'s `advisories.ignore` set is exactly the five reviewed `rust-unic` entries is a review
responsibility, not a gate: `cargo deny` reads the file as written, so a newly added entry passes silently. Adding one
is a decision to review, and the accompanying `reason` is what makes it reviewable.

`revue` is an ordinary crates.io dependency of `intention-tui`, pinned at `3.10.0`. Two of its properties are visible
here. Its event loop quits on Ctrl+C after the handler returns, while Ctrl+C is the shared core's layered action, so
`handle_event` calls `App::prevent_default` for the press the core consumes. It also depends on `unic-emoji-char`,
whose whole `rust-unic` family is archived: five `unmaintained` advisories with no safe upgrade
(RUSTSEC-2025-0075, -0080, -0081, -0090, -0098) reachable only through that probe. Those five are the complete
`advisories.ignore` set in `deny.toml`; every other advisory is resolved by upgrading the graph, and entries there
carry the reason the acknowledgement is allowed to stand. Upstream plans to replace the probe's source
(hawk90/revue#950); the acknowledgement goes away with that release.

The terminal binary takes `mimalloc` as its process-wide global allocator, because a terminal frame allocates many
short-lived small strings; it is an ordinary crates.io dependency under the MIT license.

The terminal's local theme resolution adds three direct dependencies, and none of them adds a package to the locked
graph: `toml` parses and writes the local `tui.toml` (whose `theme` key stores `auto`, `light`, or `dark`), and the
`cfg(unix)` pair `crossterm` (raw mode) and `mio`
(readiness on the terminal's own input) serve the `OSC 11` background probe, bounded at 15 ms, skipped when standard
input or output is not a terminal, and never run at all when the terminal exports `COLORFGBG`. `crossterm` and `mio` were already
locked as the backend and event source of `revue 3.10.0`, and `toml` was already locked through the same graph, so the
declared set changed without a new package, license, or advisory entering it.

## Test-first integration

Adding a production crate is the crate map entry ([01 Workspace and Crate Map](01-workspace-and-crate-map.md)), which
states its responsibility, plus one `COVERAGE_FLOORS` row in the root `Makefile`, in the same change.
There is no tier table and no separate policy edit. A focused test suite is not a substitute for the full gates.

## Opt-in live-provider e2e

The manual `make e2e-real-api` target runs the ignored `real_api_e2e` daemon integration test against
a real provider API, taking the credential only from `INTENTION_REAL_API_KEY` in the environment or a local gitignored
`.env`; the mode is
mutating, networked, and opt-in, and it is never part of the blocking gate. The obligation itself is owned by
[Test-Driven Delivery and Verification](10-test-driven-delivery-and-verification.md), scenario J.

## Non-goals

An aggregate coverage threshold; per-file coverage bars; a coverage exclusion mechanism; a central registry every crate
must be entered into; a policy checker that re-asserts what a pinned tool already enforces; denying all `pedantic` or all
`restriction` lints; a second quality toolset or Makefile surface; hidden installs inside the gates; weakening a gate to
make a change pass.
