# Quality Gates and Makefile

## Status

**Decided v1 quality policy.** This document defines the future reproducible quality gate for Intention Relay. It applies before the first production implementation is accepted.

This is a documentation-only plan. It does not create a `Makefile`, toolchain file, CI workflow, Cargo configuration, policy file, or external-tool installation yet.

## Scope

The policy covers:

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

`make quick` is the fast inner-loop signal. `make verify` is the complete reproducible merge/release signal. `make ci` initially aliases `make verify` so local and CI verification behavior cannot drift. A clean CI runner may run explicit pinned-tool setup before it invokes `make ci`.

## Reproducible tooling

### Toolchain policy

The first implementation milestone must commit:

- a pinned Rust toolchain definition;
- required Rust components, including `rustfmt`, `clippy`, and `llvm-tools-preview`;
- a pinned external quality-tool manifest;
- the Cargo lockfile;
- configuration that makes applicable Cargo operations use `--locked`.

Verification must not implicitly download, install, update, or repair tools. A missing or mismatched tool is a typed quality-gate failure.

### External quality tools

The pinned manifest must include exact versions and invocation policy for:

| Tool | Required purpose |
| --- | --- |
| `cargo-nextest` | Reproducible, parallel Rust test execution. |
| `cargo-llvm-cov` | Line/branch coverage collection, report generation, and threshold enforcement. |
| `cargo-deny` | License, bans, sources, advisory, and duplicate-version policy. |
| `cargo-audit` | Independent RustSec advisory audit. |
| `cargo-udeps` | Unused-dependency detection. It may require a pinned auxiliary nightly toolchain. |
| `cargo-machete` | Cargo manifest hygiene. |
| `cargo-outdated` | Stale direct-dependency detection. |

The implementation may add a tool only through a documented quality-policy update. It may replace a listed tool only after preserving its stated outcome.

### Tool management targets

- `make bootstrap-tools` is the sole planned installer for external quality tools. It is explicitly mutating and networked.
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

The workspace lint configuration will require:

1. Clippy defaults as errors.
2. `clippy::nursery` as errors.
3. The following selected `pedantic` lints as errors: `clippy::must_use_candidate`, `clippy::missing_errors_doc`, `clippy::missing_panics_doc`, `clippy::return_self_not_must_use`, and `clippy::use_self`.
4. The following selected `restriction` lints as errors: `clippy::allow_attributes_without_reason`, `clippy::as_underscore`, `clippy::dbg_macro`, `clippy::else_if_without_else`, `clippy::exit`, `clippy::expect_used`, `clippy::mem_forget`, `clippy::panic`, `clippy::print_stdout`, `clippy::print_stderr`, `clippy::todo`, `clippy::unimplemented`, `clippy::unwrap_used`, and `clippy::undocumented_unsafe_blocks`.
5. Rust-level denial of `unsafe_code` and any additional compiler lint needed to prevent unreasoned lint suppression.
6. A centrally documented allowlist, reviewed as policy, rather than scattered broad suppressions.

The implementation must validate the exact names, stability, and interactions of these lints against the pinned toolchain before committing the configuration. If an intended lint is unavailable or conflicts with the pinned compiler, the policy update must select an equivalent supported check, never silently weaken the outcome.

The policy deliberately does **not** deny all `pedantic` or all `restriction` lints. Some are subjective or ergonomically harmful and would encourage formal workarounds instead of better code.

### Architectural lint protections

The future lint and architecture configuration must additionally prohibit or detect:

- direct process-CWD fallback in tools that must use `WorkspaceRoot`;
- public cross-crate APIs exposing provider SDK, SQLite, Tokio, UI, or other implementation resources;
- direct application/runtime/storage implementation access from Tauri/TUI adapters;
- `unsafe` and process-level escape hatches without an approved, local explanation;
- raw debug output or secret-bearing errors reaching observable paths.

Where Clippy cannot enforce a rule, `make architecture` owns the check.

## Coverage policy

Coverage is a blocking guardrail from the moment a crate contains production code. There is no grace baseline and no gradual ramp.

The first implementation milestone must add a versioned coverage-policy file and checker over `cargo llvm-cov` output. Branch-aware reports use the pinned dated nightly toolchain; ordinary application checks remain on pinned stable. The checker maps reportable source files to each declared production crate, enforces that crate's individual tier, records exclusions, and emits a report artifact.

### Tiered line coverage thresholds

| Tier | Crate categories | Minimum line coverage |
| --- | --- | ---: |
| A | `types`, `domain`, `config`, `protocol` | 95% |
| B | `application`, `runtime`, `storage`, `storage-sqlite`, `tools`, `workspace`, `hooks`, `plans`, `vfr`, `headroom` | 90% |
| C | `model`, provider adapters, `transport`, `client`, `daemon` | 85% |
| Adapter exception | Tauri and TUI presentation crates | No aggregate UI line threshold. Require complete command/event mapping contracts and all mandatory fixture-daemon smoke/outcome scenarios. |

Branch coverage must be reported. Critical safety and recovery branches are not excused by a passing line threshold. Those branches are independently mandatory in the scenario tests defined by [10 Test-Driven Delivery and Verification](10-test-driven-delivery-and-verification.md).

### Coverage constraints

- Every production crate declares its tier before production code is merged.
- Every required feature profile contributes to coverage, where the crate supports that profile.
- A coverage decrease fails `make coverage` and `make verify`.
- Generated code and technically unmeasurable code may be excluded only through a versioned policy entry with rationale, owner, and equivalent test evidence.
- Exclusions cannot hide core runtime, policy, provider translation, persistence, redaction, or security logic.
- Coverage reports are stored as CI artifacts.
- Test code, generated code, and configured exclusions must not inflate the production coverage denominator.

## Cargo feature-profile policy

All applicable check, lint, test, documentation, and coverage paths exercise:

1. default features;
2. `--no-default-features`;
3. `--all-features`;
4. an explicitly versioned list of critical feature combinations.

This is intentionally not an exhaustive combinatorial matrix. Every new optional provider, adapter, or feature must either be covered by one of the first three profiles or add a critical combination in the same change.

The feature-profile policy must be machine-readable and verified by `make features`.

## Makefile contract

The future root `Makefile` is the sole supported orchestration surface for local and CI quality workflows. Recipes use strict shell behavior and label each command as mutating or non-mutating.

| Target | Mutation | Required behavior |
| --- | --- | --- |
| `make help` | No | List supported targets, dependencies, and mutation status. |
| `make bootstrap-tools` | Yes | Install only pinned tool versions with locked installation. |
| `make tools-check` | No | Validate Rust/tool versions and required components. |
| `make fmt` | Yes | Apply formatting deliberately. |
| `make fmt-check` | No | Verify formatting without changing files. |
| `make features` | No | Check default, no-default, all-features, and critical combinations. |
| `make lint` | No | Run strict lint policy with warnings denied for all feature profiles. |
| `make test` | No | Run nextest suites and doctests for all feature profiles. |
| `make docs-check` | No | Build Rust docs with warnings denied and validate Markdown links, planned Mermaid diagrams, and documentation navigation. |
| `make architecture` | No | Run crate-set, dependency, import, DTO, WorkspaceRoot, hook, plan, and public-API boundary checks. |
| `make coverage` | No | Collect coverage and apply the tier policy. |
| `make deps` | No | Run lockfile, deny, audit, unused, manifest, stale direct-dependency, and duplicate-version checks. |
| `make quick` | No | Run tools-check, fmt-check, lint, and focused/default tests for fast iteration. |
| `make check` | No | Run complete source-quality checks: tools, formatting, features, lint, all tests, doctests, docs, and architecture. |
| `make verify` | No | Run `check` plus coverage and dependency/supply-chain checks. |
| `make ci` | No | Alias the blocking CI gate, initially `verify`. |

`make check`, `make verify`, and `make ci` must fail rather than modify source, update a lockfile, install tools, or resolve dependencies differently from committed state.

### Cargo command coverage

The Makefile must orchestrate relevant Cargo quality commands rather than rely on developers remembering individual flags. It must run each applicable command for every workspace member, every target kind, and every required feature profile. It must include applicable equivalents of:

- `cargo fmt --check`;
- `cargo check --workspace --all-targets --locked` across the feature policy;
- `cargo clippy --workspace --all-targets --locked` across the feature policy with warnings denied;
- `cargo nextest run --workspace --all-targets --locked` and `cargo test --workspace --doc --locked` across the feature policy;
- `cargo doc --workspace --no-deps --locked` with warnings denied;
- `cargo metadata --locked` and lockfile validation;
- coverage, dependency, license, advisory, unused-dependency, manifest, stale-direct-dependency, and architecture checks described in this policy.

An ordinary `cargo build`, `cargo run`, or destructive `cargo clean` is not an independent required quality gate when the relevant compile/test contracts are already covered. The Makefile must avoid redundant commands that increase time without proving a distinct result.

## Supply-chain and dependency hygiene

`make deps` and blocking CI must run:

- `cargo metadata --locked` as the authoritative lockfile check;
- `cargo deny check` for advisories, licenses, banned crates, allowed sources, and duplicate-version policy, using the supported syntax of the pinned cargo-deny version;
- `cargo audit` as an independent advisory source;
- `cargo udeps` for unused dependencies;
- `cargo machete` for manifest hygiene;
- `cargo outdated` for stale direct dependencies;
- committed-lockfile validation using `--locked`.

Exceptions use reviewed, versioned policy/allowlist files. Every exception has a justification and, where applicable, expiration/review date. Ignoring a failing gate, using a broad CI bypass, or silently allowing a tool failure is prohibited.

## Test-first, architectural, and outcome integration

Every implementation slice must:

1. identify the owning architecture document and crate coverage tier;
2. create or update DTO/contract fixtures first;
3. create failing domain, architecture, and outcome tests appropriate to the change;
4. implement the smallest code that makes those tests pass;
5. run `make quick` while iterating;
6. run `make verify` before the slice is accepted;
7. record every policy exception, test exclusion, and known risk explicitly.

A high coverage percentage, passing lint, or successful compilation never replaces a required end-to-end outcome scenario.

## Required quality-gate failure tests

Milestone 0 must prove the quality system itself fails correctly for controlled fixtures:

| Intentional defect | Gate that must fail |
| --- | --- |
| Formatting drift | `make fmt-check`. |
| Compiler or Clippy warning | `make lint`. |
| Unreasoned or broad lint suppression | `make lint` or `make architecture`. |
| Missing/mismatched pinned tool | `make tools-check`. |
| Missing required crate/test target | `make architecture`. |
| Forbidden crate dependency or import | `make architecture`. |
| DTO/SDK implementation leak | `make architecture`. |
| Coverage below declared tier | `make coverage`. |
| Unapproved coverage exclusion | `make coverage`. |
| Uncovered required feature profile | `make features`. |
| Dependency advisory/license/source/ban/duplicate violation | `make deps`. |
| Unused, stale, or manifest-only dependency | `make deps`. |
| Recognizable fake secret in output fixture | `make test` and `make verify`. |

## Acceptance criteria

The quality foundation is accepted only when the plans and, later, implementation establish that:

- formatting, compilation, linting, tests, documentation, architecture checks, coverage, and supply-chain checks are blocking;
- tool versions are pinned and checked before every reproducible quality run;
- Makefile commands orchestrate every non-mutating quality gate and CI invokes `make ci` as its sole verification command after explicit setup;
- coverage thresholds apply immediately by crate tier with no unreviewed escape hatch;
- adapters use mapping/contract/outcome evidence rather than a misleading aggregate UI line target;
- linting is strict but pragmatic, with narrow justified exceptions rather than a blanket unworkable lint set;
- default, no-default, all-features, and critical combinations are always verified;
- TDD and semantic acceptance tests remain mandatory regardless of coverage percentage.

## Non-goals

- Installing or configuring quality tools in this documentation phase.
- Requiring an arbitrary global number of tests.
- Replacing human review with coverage percentage or lint compliance.
- Exhaustive verification of every mathematically possible Cargo feature combination.
- Turning the Makefile into a generic build/run/cleanup wrapper unrelated to quality outcomes.
