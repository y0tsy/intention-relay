# Quality Gates and Makefile

## Status

**Implemented M0 quality foundation, decided v1 quality policy.** Milestone 0 delivered the reproducible quality gate in commit `f2bb35d040dcec93639d6715fc06429b9070e1e3`, before the first production implementation was accepted. It includes the pinned Rust toolchains and external-tool manifest, root `Makefile`, Cargo configuration and committed lockfile, machine-readable policies, quality checkers and self-tests, and CI workflow.

This document remains the authoritative v1 quality policy. Its M0 controls are implemented and blocking. Future milestones extend the policy only when they add a new crate, feature combination, adapter, quality tool, or an explicitly approved stronger check. The M0/M1 verification baseline and its known deferred hardening work are recorded in [M0/M1 Closure Evidence](../closeout/m0-m1-closure-evidence.md).

## Scope

The implemented policy covers:

- pinned Rust toolchain and external quality tools;
- strict pragmatic linting and formatting;
- immediate tiered coverage requirements;
- required Cargo feature profiles;
- tests, doctests, documentation, and architecture checks;
- dependency, license, advisory, unused-dependency, stale-dependency, and manifest hygiene checks;
- the root Makefile contract and CI entry point;
- explicit, reviewable exception handling.

It complements, and does not replace, [Test-Driven Delivery and Verification](10-test-driven-delivery-and-verification.md). Numeric coverage never replaces contract, architecture, failure-path, or outcome tests.

## Quality model

```mermaid
flowchart LR
  DV[Developer] --> Q[make quick]
  DV --> V[make verify]
  Q --> F[Format]
  Q --> L[Lint]
  Q --> T[Focused tests]
  V --> F
  V --> L
  V --> T
  V --> X[Feature profiles]
  V --> C[Coverage]
  V --> A[Architecture]
  V --> D[Documentation]
  V --> S[Supply chain]
  V --> CI[Blocking CI]
```

`make quick` is the fast inner-loop signal. `make verify` is the complete reproducible merge/release signal. `make ci` aliases `make verify` so local and CI verification behavior cannot drift. A clean CI runner performs explicit pinned-tool setup before invoking `make ci` on required Linux and Windows runners; CI installs exact tool releases through checksum-verified `taiki-e/install-action` rather than source-compiling every tool on each cache miss. CI records free space and relevant artifact-directory sizes before and after the quality gate as operational diagnostics; those reports never change a gate's verdict. Windows acceptance exercises the named-pipe transport fixture rather than relying on cross-compilation alone.

## Reproducible tooling

### Toolchain policy

M0 commits and enforces:

- a pinned Rust toolchain definition;
- required Rust components, including `rustfmt`, `clippy`, and `llvm-tools-preview`;
- a pinned external quality-tool manifest;
- the Cargo lockfile;
- Cargo invocation with `--locked` where applicable.

Verification does not implicitly download, install, update, or repair tools. A missing or mismatched tool is a typed quality-gate failure.

### External quality tools

The pinned manifest records exact versions and invocation policy for:

| Tool | Required purpose |
| --- | --- |
| `cargo-nextest` | Reproducible, parallel Rust test execution. |
| `cargo-llvm-cov` | Line/branch coverage collection, report generation, and threshold enforcement. |
| `cargo-about` | Deterministic third-party license-notice generation from the locked dependency graph. |
| `cargo-deny` | License, bans, sources, advisory, and duplicate-version policy. |
| `cargo-audit` | Independent RustSec advisory audit. |
| `cargo-udeps` | Unused-dependency detection. It uses the pinned auxiliary nightly toolchain. |
| `cargo-machete` | Cargo manifest hygiene. |
| `cargo-outdated` | Stale direct-dependency detection. |

A subsequent milestone may add a tool only through a documented quality-policy update. It may replace a listed tool only after preserving the stated outcome.

### Tool management targets

- `make bootstrap-tools` is the sole installer for external quality tools. It is explicitly mutating and networked. It retains a matching restored tool binary and reinstalls an absent or version-mismatched one with an exact pinned version, never selecting an unpinned version.
- `make tools-check` validates the pinned Rust version, required components, external tool availability, and exact versions. It is non-mutating.
- `make check`, `make verify`, and `make ci` call `tools-check` but never call `bootstrap-tools`.

## Formatting and lint policy

### Global requirements

- `rustfmt` is mandatory. `make fmt-check` fails on formatting drift and never modifies files.
- All warnings are errors in every non-mutating verification target.
- The strict lint policy runs for every required Cargo feature profile.
- `unsafe` is denied by default.
- Production code denies unreviewed `unwrap`, `expect`, `panic`, `todo!`, `unimplemented!`, `dbg!`, direct stdout/stderr printing, direct process termination, and memory-forget patterns.
- A lint suppression uses a narrow local scope and a mandatory `reason = "..."` that explains why it is safe.
- Test-only exceptions are also local and reasoned. Test convenience must not become a production loophole.

### Strict pragmatic Clippy baseline

The workspace lint configuration requires:

1. Clippy defaults as errors.
2. `clippy::nursery` as errors.
3. The selected `pedantic` lints `clippy::must_use_candidate`, `clippy::missing_errors_doc`, `clippy::missing_panics_doc`, `clippy::return_self_not_must_use`, and `clippy::use_self` as errors.
4. The selected `restriction` lints `clippy::allow_attributes_without_reason`, `clippy::as_underscore`, `clippy::dbg_macro`, `clippy::else_if_without_else`, `clippy::exit`, `clippy::expect_used`, `clippy::mem_forget`, `clippy::panic`, `clippy::print_stdout`, `clippy::print_stderr`, `clippy::todo`, `clippy::unimplemented`, `clippy::unwrap_used`, and `clippy::undocumented_unsafe_blocks` as errors.
5. Rust-level denial of `unsafe_code`.
6. A centrally documented allowlist, reviewed as policy, rather than scattered broad suppressions.

M0 validates the selected lint names, stability, and interactions against the pinned toolchain. If a future toolchain makes an intended lint unavailable or conflicting, the policy update must select an equivalent supported check, never silently weaken the outcome.

The policy deliberately does **not** deny all `pedantic` or all `restriction` lints. Some are subjective or ergonomically harmful and would encourage formal workarounds instead of better code.

### Architectural lint protections

`make architecture` currently detects or prohibits:

- direct process-CWD fallback in checked source;
- listed provider SDK namespaces and HTTP/runtime resources from active model/config/domain/runtime/application/storage/protocol/transport/client/daemon/adapter source, while allowing `openrouter-rs` only in private `intention-provider-openrouter` implementation and `async-openai` only in private `intention-provider-generic-chat` implementation;
- provider SDK/resource types from public rustdoc-visible APIs of every active crate;
- direct application/runtime/storage implementation access from Tauri/TUI adapters;
- `unsafe` and process-level escape hatches without an approved, local explanation;
- raw debug output or secret-bearing errors reaching checked paths.

`make architecture` detects workspace dependency cycles, validates that active-crate machine-readable integration targets exactly equal Cargo metadata, and prevents direct forbidden source patterns. It also runs `quality/check_public_api.py`, which uses the pinned nightly rustdoc JSON output to reject forbidden implementation resources and provider SDK namespaces in public reachable aliases, fields, wrappers, nested generic arguments, function signatures, trait surfaces, and re-exports. Private-only implementation details are outside this public-contract check.

## Coverage policy

Coverage is a blocking guardrail from the moment a crate contains production code. There is no grace baseline and no gradual ramp.

M0 provides the versioned coverage-policy file and checker over `cargo llvm-cov` output. Branch-aware reports use the pinned dated nightly toolchain; ordinary application checks remain on pinned stable. The checker maps reportable source files to each declared production crate, normalizing CI report paths back to workspace-relative sources before per-crate and exclusion arithmetic, enforces that crate's individual line tier, requires branch metrics, and emits JSON report artifacts. Enabled exclusions are exact repository-relative source-file paths with rationale, owner, and equivalent test evidence; they must resolve under the active owner's `src` root, appear exactly once in the coverage report, and are subtracted from that crate's numerator and denominator. The M1 reports prove the policy for its four active Tier A crates.

### Tiered line coverage thresholds

| Tier | Crate categories | Minimum line coverage |
| --- | --- | ---: |
| A | `types`, `domain`, `config`, `protocol` | 95% |
| B | `application`, `runtime`, `storage`, `storage-sqlite`, `tools`, `workspace`, `hooks`, `plans`, `vfr`, `headroom` | 90% |
| C | `model`, provider adapters, `transport`, `client`, `daemon` | 85% |
| Adapter exception | Tauri and TUI presentation crates | No aggregate UI line threshold. Require complete command/event mapping contracts, all mandatory fixture-daemon smoke/outcome scenarios, and required platform CI evidence. |

Branch coverage is reported. Critical safety and recovery branches are not excused by a passing line threshold. Those branches remain independently mandatory in the scenario tests defined by [10 Test-Driven Delivery and Verification](10-test-driven-delivery-and-verification.md).

### Coverage constraints

- Every production crate declares its tier before production code is merged.
- Every required feature profile contributes to coverage where the crate supports that profile.
- A coverage decrease fails `make coverage` and `make verify`.
- Generated code and technically unmeasurable code may be excluded only through a versioned policy entry with rationale, owner, and equivalent test evidence.
- Exclusions cannot hide core runtime, policy, provider translation, persistence, redaction, or security logic.
- Coverage reports are stored as CI artifacts.
- Test code and generated code must not inflate the production coverage denominator.

Applying an enabled exclusion is explicit and reviewable. The checker rejects duplicate, absolute, traversing, unowned, out-of-source-root, absent, unreported, and all-source-removing exclusions. The sole enabled M2 exclusion is `intention-daemon/src/main.rs`: it is a thin process adapter whose unsafe-argument and concurrent bootstrap behavior are exercised through the real binary in `daemon_bootstrap`; Windows `llvm-cov` does not merge that child process instrumentation into its parent `nextest` report. All daemon library behavior remains subject to Tier C coverage.

## Cargo feature-profile policy

All applicable check, lint, test, documentation, and coverage paths exercise:

1. default features;
2. `--no-default-features`;
3. `--all-features`;
4. an explicitly versioned list of enabled critical feature combinations.

This is intentionally not an exhaustive combinatorial matrix. M1 has no enabled critical combination. Every future optional provider, adapter, or feature must either be covered by one of the first three profiles or add a critical combination in the same change.

The feature-profile policy is machine-readable and verified by `make features`. M4's selected provider SDK dependencies do not add a workspace feature flag or critical combination; default, no-default, and all-features profiles therefore remain the required coverage for their private SDK integration.

Some production packages additionally declare isolated release profiles in the
same policy. `make isolated-release` checks each declared package's explicit
release targets, currently the daemon library and binary, once per declared
profile without `--workspace` or test targets. This prevents workspace feature
unification from hiding a standalone default or `--no-default-features` release
build failure. `make check`, `make verify`, and CI require this gate.

## Makefile contract

The root `Makefile` is the sole supported orchestration surface for local and CI quality workflows. Recipes use strict shell behavior and label each command as mutating or non-mutating.

| Target | Mutation | Required behavior |
| --- | --- | --- |
| `make help` | No | List supported targets, dependencies, and mutation status. |
| `make bootstrap-tools` | Yes | Install only pinned tool versions with locked installation. |
| `make tools-check` | No | Validate Rust/tool versions and required components. |
| `make fmt` | Yes | Apply formatting deliberately. |
| `make fmt-check` | No | Verify formatting without changing files. |
| `make notices` | Yes | Regenerate `THIRD_PARTY_NOTICES.md` from the locked dependency graph and committed notice policy/template. |
| `make notices-check` | No | Regenerate notices in a temporary file and fail if committed notices are missing or stale. |
| `make features` | No | Check default, no-default, all-features, critical combinations, and the machine-readable isolated-release declaration. |
| `make isolated-release` | No | Check each declared production package's declared library/binary release targets per isolated feature profile, without workspace feature unification. |
| `make lint` | No | Run strict lint policy with warnings denied for all feature profiles. |
| `make test` | No | Run nextest suites and doctests for all feature profiles. |
| `make docs-check` | No | Build Rust docs with warnings denied and validate Markdown links, Mermaid diagrams, and documentation navigation. |
| `make architecture` | No | Run crate-set, dependency, import, DTO, WorkspaceRoot, hook, plan, and public-API boundary checks. |
| `make coverage` | No | Collect coverage and apply the tier policy. |
| `make coverage-artifacts-clean` | Yes, generated artifacts only | Remove `target/llvm-cov-target` after coverage's JSON reports are checked; it preserves reports and ordinary Cargo artifacts. |
| `make deps` | No | Run locked metadata, third-party-notice freshness, deny, audit, unused-dependency, manifest, stale-direct-dependency, and duplicate-version checks. |
| `make quick` | No | Run tools-check, fmt-check, lint, and focused/default tests for fast iteration. |
| `make check` | No | Run complete source-quality checks: tools, formatting, features, lint, all tests, doctests, docs, and architecture. |
| `make verify` | No | Run `check` plus coverage and dependency/supply-chain checks. |
| `make ci` | No | Alias the blocking CI gate, `verify`. |

`make verify` runs `check`, `coverage`, and `deps`, then removes only the generated LLVM coverage target before `quality-self-test`; coverage reports remain available for CI upload. `quality-self-test` still copies and mutates an isolated source tree, but its copied-repository Cargo commands reuse the controller workspace `target` directory. Quality scripts resolve Cargo-generated artifacts through `CARGO_TARGET_DIR` when it is set. Cargo fingerprints the copied source paths and contents, so intentional fixture defects still recompile affected crates and must produce their required failures without allocating a second complete target tree in the temporary directory. `make check`, `make verify`, and `make ci` fail rather than modify source, update a lockfile, install tools, or resolve dependencies differently from committed state.

### Cargo command coverage

The Makefile orchestrates relevant Cargo quality commands rather than relying on developers remembering individual flags. It runs each applicable command for every workspace member, every target kind, and every required feature profile. It includes applicable equivalents of:

- `cargo fmt --check`;
- `cargo check --workspace --all-targets --locked` across the feature policy;
- `cargo clippy --workspace --all-targets --locked` across the feature policy with warnings denied;
- `cargo nextest run --workspace --all-targets --locked` and `cargo test --workspace --doc --locked` across the feature policy;
- `cargo doc --workspace --no-deps --locked` with warnings denied;
- `cargo metadata --locked` and lockfile validation;
- coverage, dependency, license, advisory, unused-dependency, manifest, stale-direct-dependency, and architecture checks described in this policy.

An ordinary `cargo build`, `cargo run`, or destructive `cargo clean` is not an independent required quality gate when the relevant compile/test contracts are already covered. The Makefile avoids redundant commands that increase time without proving a distinct result.

## Supply-chain and dependency hygiene

`make deps` and blocking CI run:

- `cargo metadata --locked` as the authoritative lockfile check;
- `make notices-check`, which regenerates `THIRD_PARTY_NOTICES.md` from `Cargo.lock`, `quality/about.toml`, and `quality/third_party_notices.hbs` through pinned `cargo-about` and fails on drift;
- `cargo deny check` for advisories, licenses, banned crates, allowed sources, and duplicate-version policy, using the supported syntax of the pinned cargo-deny version;
- `cargo audit` as an independent advisory source;
- `cargo udeps` for unused dependencies;
- `cargo machete` for manifest hygiene;
- `cargo outdated` for stale direct dependencies;
- committed-lockfile validation using `--locked`.

`THIRD_PARTY_NOTICES.md` is a checked-in generated disclosure artifact, not a hand-maintained license inventory. It contains license texts and registry dependency attribution for the locked graph; private `publish = false` workspace packages remain project-owned code and are excluded. `cargo-about` selects a valid license branch for multi-licensed crates using `quality/about.toml`, while `cargo deny` independently enforces the complete policy expression and source allowlist. A failed or stale notice generation is a blocking supply-chain failure, not an inferred pass.

M4 added direct `openrouter-rs 0.14.0` and `async-openai 0.29.3` dependencies. Their graph requires existing-policy MIT/Apache terms plus BSD-3-Clause, ISC, and CDLA-Permissive-2.0 for Rustls and `webpki-roots`; each is explicitly allowed in `deny.toml` and `quality/about.toml`, then disclosed by generated notices. M4 also pins the unavoidable duplicate branches from these SDKs: `getrandom@0.2.17`, `getrandom@0.3.4`, target-only `r-efi@5.3.0`, `rand@0.8.7`, `rand_chacha@0.3.1`, `rand_core@0.6.4`, `syn@1.0.109`, `thiserror@1.0.69`, `thiserror-impl@1.0.69`, and target-only `windows-sys@0.52.0`. Each exception is limited to the documented provider SDK path in `deny.toml` and must be reassessed when either selected SDK or its immediate HTTP/random/backoff path changes. The M4 provider graph also requires the two transitive-only advisory acknowledgements `RUSTSEC-2025-0012` (`async-openai -> backoff`) and `RUSTSEC-2024-0384` (`async-openai -> backoff -> instant`): neither has a compatible upstream remediation, neither creates a direct project dependency, and both are explicitly passed to `cargo audit --ignore` as well as recorded in `deny.toml`. Both must be removed or reassessed with an `async-openai`/`backoff` update. `async-openai` is also the only reviewed `cargo outdated` hold because `0.41.3` is outside the selected `0.29.3` compatibility contract; reassess its API and the two advisory acknowledgements together before removing the hold. No source exception is approved by this package.

Exceptions use reviewed, versioned policy/allowlist files. Every exception has a justification and, where applicable, expiration/review date. The approved `0BSD` entry is required transitively by `interprocess`'s local-IPC support (`doctest-file` and `recvmsg`) and is an OSI-approved permissive license. The SQLite graph also introduces `foldhash 0.2.0`, whose Zlib license is explicitly allowed in both `quality/about.toml` and `deny.toml` and disclosed through the generated notices. M3 has two narrowly version-pinned duplicate exceptions, both limited to the target-specific WASM dependencies selected by `rusqlite 0.40.1 -> sqlite-wasm-rs 0.5.5`: `hashbrown@0.16.1` is required only by `rsqlite-vfs 0.1.1`, while `syn@2.0.119` is required only by `wasm-bindgen-macro-support 0.2.126` through `wasm-bindgen 0.2.126`. Native bundled SQLite instead resolves `rusqlite 0.40.1 -> hashlink 0.12.1 -> hashbrown 0.17.1`; the independently required native proc-macro path retains `syn@3.0.3`. The native daemon continues to use bundled SQLite through `rusqlite` with its `bundled` feature and retains `rusqlite_migration 2.6.0`; these exceptions do not replace either dependency or relax duplicate bans for other crates or versions. Reassess both exceptions whenever `rusqlite`, `sqlite-wasm-rs`, `rsqlite-vfs`, `wasm-bindgen`, or their supported target selection changes, and again before M3 closeout; record the locked native and WASM validation trees with the review. Ignoring a failing gate, using a broad CI bypass, or silently allowing a tool failure is prohibited.

## Test-first, architectural, and outcome integration

Every implementation slice must:

1. identify the owning architecture document and crate coverage tier;
2. create or update DTO/contract fixtures first;
3. create failing domain, architecture, and outcome tests appropriate to the change;
4. implement the smallest code that makes those tests pass;
5. run `make quick` while iterating;
6. run `make verify` before the slice is accepted;
7. record every policy exception, test exclusion, and known risk explicitly.

During the completed M4 delivery, the controller-owned
[`M4 execution charter`](../m4.md) additionally required package-level
code-first batching: agents wrote the complete bounded fixture portfolio, then
finished the bounded production change before entering a grouped Makefile
validation/repair phase. This historical process did not replace steps 5–7 or
authorize an unimplemented focused gate.

A high coverage percentage, passing lint, or successful compilation never replaces a required end-to-end outcome scenario.

### M4 controller charter and closure record

[`docs/intention-relay/m4.md`](../m4.md) records the controller-owned M4
execution charter. It is read-only historical context and does not override
this quality policy or authorize worktrees, lane execution, or any Makefile
change. M4 is closed at the immutable baseline documented in [M4 Closure
Evidence](../closeout/m4-closure-evidence.md); changes to that scope require a
new decision and their own acceptance evidence.

The active Makefile contract remains unchanged: every implementation handoff
runs `make quick` while iterating and `make verify` before acceptance. The
charter's proposed package-scoped `make focus PACKAGES=...` target remains
unimplemented and may not weaken the full-gate requirement. No simultaneous
full `make verify` executions may claim independent acceptance on the same
host; coverage, dependency, documentation, and Cargo resource contention would
make their results impractical to interpret.

## Required quality-gate failure tests

M0 proves that the quality system fails correctly for controlled fixtures:

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
| Coverage below declared tier | `make coverage`. |
| Unapproved coverage exclusion metadata | `make coverage`. |
| Uncovered required feature profile | `make features`. |
| Dependency advisory/license/source/ban/duplicate violation | `make deps`. |
| Unused, stale, or manifest-only dependency | `make deps`. |
| Recognizable fake secret in output fixture | `make test` and `make verify`. |

The specific fixtures and gates, including M1 additions, are listed in the [closure evidence matrix](../closeout/m0-m1-closure-evidence.md#acceptance-evidence-matrix).

## Acceptance criteria

M0 is accepted because its implementation establishes that:

- formatting, compilation, linting, tests, documentation, architecture checks, coverage, and supply-chain checks are blocking;
- tool versions are pinned and checked before every reproducible quality run;
- Makefile commands orchestrate every non-mutating quality gate and CI invokes `make ci` as its sole verification command after explicit setup;
- coverage thresholds apply immediately by crate tier with no unreviewed escape hatch;
- adapters use mapping/contract/outcome evidence rather than a misleading aggregate UI line target;
- linting is strict but pragmatic, with narrow justified exceptions rather than a blanket unworkable lint set;
- default, no-default, all-features, and enabled critical combinations are verified;
- TDD and semantic acceptance tests remain mandatory regardless of coverage percentage.

## Non-goals

- Requiring an arbitrary global number of tests.
- Replacing human review with coverage percentage or lint compliance.
- Exhaustive verification of every mathematically possible Cargo feature combination.
- Turning the Makefile into a generic build/run/cleanup wrapper unrelated to quality outcomes.
