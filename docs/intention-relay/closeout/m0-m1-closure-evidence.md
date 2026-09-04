# M0/M1 Closure Evidence

## Status and purpose

**Closed baseline.** This document is the immutable audit trail for the completed M0 quality foundation and M1 contracts, configuration, and workspace skeleton. It fulfills the reportable verification-evidence requirement in [Test-Driven Delivery and Verification](../architecture/10-test-driven-delivery-and-verification.md).

The implementation baseline is deliberately separate from this closeout documentation commit. The SHA below identifies the exact M1 source tree verified before closeout documentation was added.

| Item | Value |
| --- | --- |
| M0 quality-foundation commit | `f2bb35d040dcec93639d6715fc06429b9070e1e3` (`chore: establish M0 quality foundation`) |
| M1 implementation baseline | `dd7c5e7b5db7131fee7ee015822d905964fe07a6` (`feat(m1): establish contracts configuration and workspace skeleton`) |
| Baseline parentage | M1 baseline directly follows the M0 quality-foundation commit. |
| Verification command | `make verify` |
| Result | Exit status `0` |
| Evidence capture completed | `2026-08-01T13:39:02Z` |
| Host environment | Linux `7.1.5-200.fc44.x86_64`, `x86_64` GNU/Linux |
| Stable Rust | `rustc 1.97.1 (8bab26f4f 2026-07-14)` |
| Coverage Rust | `rustc 1.99.0-nightly (8ab9fdff5 2026-07-30)`, toolchain `nightly-2026-07-31` |

## Verification configuration

### Pinned tools exercised

| Tool | Verified version |
| --- | --- |
| `cargo-nextest` | `0.9.140` |
| `cargo-llvm-cov` | `0.8.7` |
| `cargo-deny` | `0.20.2` |
| `cargo-audit` | `0.22.2` |
| `cargo-udeps` | `0.1.61` |
| `cargo-machete` | `0.9.2` |
| `cargo-outdated` | `0.19.0` |

### Feature profiles

The machine-readable policy ran these profiles through check, lint, test, doctest, documentation, and coverage paths:

| Profile | Cargo flags | Result |
| --- | --- | --- |
| `default` | _(default features)_ | Pass |
| `no_default` | `--no-default-features` | Pass |
| `all` | `--all-features` | Pass |

No critical feature combination is enabled in M1. A later optional provider, adapter, or feature must add an enabled critical combination if the three required profiles do not cover it.

### Coverage artifacts

`make verify` generated branch-aware JSON reports under `quality/reports/`; CI uploads that directory and `target/nextest` as the `quality-reports` artifact. The four active M1 production crates are Tier A, with a required **95% line coverage** per crate. There are no enabled coverage exclusions.

| Profile | `intention-types` | `intention-domain` | `intention-protocol` | `intention-config` |
| --- | ---: | ---: | ---: | ---: |
| `default` | 427/427, 100.00% | 253/253, 100.00% | 169/169, 100.00% | 454/465, 97.63% |
| `no_default` | 427/427, 100.00% | 253/253, 100.00% | 169/169, 100.00% | 454/465, 97.63% |
| `all` | 427/427, 100.00% | 253/253, 100.00% | 169/169, 100.00% | 454/465, 97.63% |

Branch coverage was reported in every profile: `intention-config`, `intention-domain`, and `intention-protocol` each reported 100.000%; `intention-types` reported 84.375%. Branch coverage is recorded and required to be present, but M0/M1 establishes no branch-percentage threshold.

Report files:

- `quality/reports/coverage-default.json`
- `quality/reports/coverage-no_default.json`
- `quality/reports/coverage-all.json`

## Acceptance evidence matrix

| Milestone and acceptance criterion | Automated proof and fixture/check mapping | Gate and result |
| --- | --- | --- |
| M0: the root quality workflow is reproducible, non-mutating, and CI uses one entry point | `Makefile` defines `quick`, `check`, `verify`, and `ci`; `.github/workflows/quality.yml` performs explicit setup then invokes only `make ci`. | `make verify` passed; CI entry point is `make ci`. |
| M0: pinned toolchains, components, and quality tools are checked | `rust-toolchain.toml`, `quality/tools.toml`, and `quality/check_tools.py`; self-test `test_tool_version_mismatch`. | `make tools-check` passed; self-test expects the mismatch fixture to fail. |
| M0: formatting, warning, and lint-suppression drift fail | `quality/self_test.py`: `test_formatting_drift`, `test_lint_warning`, and `test_unreasoned_suppression`; `quality/check_architecture.py` validates lint reasons and broad suppressions. | `make fmt-check`, `make lint`, and `make quality-self-test` passed. |
| M0: required feature profiles are enforced | `quality/features.toml`, `quality/check_features.py`, self-test `test_missing_feature_profile`. | `make features` passed for default, no-default, and all-features profiles. |
| M0: coverage tiers and exclusion metadata are enforced | `quality/coverage.toml`, `quality/check_coverage.py`, self-test `test_coverage_failures`. | `make coverage` passed; each active Tier A crate met at least 95% line coverage in every profile. |
| M0: architecture policy rejects invalid workspace/crate metadata and forbidden source | `quality/architecture.toml`, `quality/check_architecture.py`, self-tests `test_missing_crate_metadata` and `test_forbidden_source_boundary`. | `make architecture` and `make quality-self-test` passed. |
| M0: supply-chain and manifest controls reject invalid policy/dependency inputs | `deny.toml`, `quality/check_deny_policy.py`, self-tests `test_supply_chain_policy_failures`, `test_unused_dependency`, and `test_outdated_dependency`. | `make deps` and `make quality-self-test` passed. |
| M0: recognizable fake secrets do not pass checked source/documentation paths | `quality/check_docs.py`, self-test `test_secret_fixture`. | `make docs-check` and `make quality-self-test` passed. |
| M1: all planned v1 crates are workspace members, with four active Tier A crates and later crates restricted to skeletons | `Cargo.toml`, `quality/architecture.toml`, `quality/check_architecture.py`; self-tests `test_m1_dependency_boundary` and `test_m1_skeleton_api_boundary`. | `make architecture` and `make quality-self-test` passed. |
| M1: active DTO/config/protocol crates preserve safe typed contracts and serialized compatibility | Active-crate integration suites: `intention-types/tests/{contracts,error_contracts}.rs`, `intention-domain/tests/{contracts,event_fixtures}.rs`, `intention-protocol/tests/{contracts,accessors,m3_contracts,m4_run_stream_contracts,control_plane_contracts}.rs`, and `intention-config/tests/{contracts,snapshot_fixtures}.rs`, with versioned JSON fixtures. The former `protocol_fixtures` target was removed by ADR 0038; its unknown-additive-field tolerance assertion is re-homed into `intention-protocol/tests/contracts.rs`. | `make test` passes the current protocol and DTO contract suites in each required profile; doctests also pass. |
| M1: invalid TOML is safe, v0 migrates to v1, and public configuration/snapshots are credential-free | `intention-config/tests/contracts.rs`, `snapshot_fixtures.rs`, and `fixtures/config-snapshot-v1.json`; architecture self-test `test_m1_secret_projection`. | `make test`, `make architecture`, and `make quality-self-test` passed. |
| M1: active public contracts exclude listed implementation resources and provider SDK types | `quality/check_architecture.py`, policy resource lists, and self-tests `test_m1_public_resource_leak`, `test_provider_sdk_public_contract_boundary`, and `test_protocol_isolation_boundary`. | `make architecture` and `make quality-self-test` passed. |
| M1: adapters remain isolated and concrete implementation selection stays composition-owned | Architecture policy and self-tests `test_adapter_isolation_boundary` and `test_composition_ownership_boundary`. | `make architecture` and `make quality-self-test` passed. |
| M1: all four active crates meet Tier A coverage without enabled exclusions | Per-profile reports listed above, `quality/check_coverage.py`, and `quality/coverage.toml`. | `make coverage` passed in all required profiles. |

## Scope boundaries and deferred work

### M1 boundaries

M1 is closed only for the following scope:

- `intention-types`, `intention-domain`, `intention-protocol`, and `intention-config` are the four active Tier A production crates.
- All other planned v1 crates are compile-only M1 skeletons with no dependencies and no public API. Their behavior remains owned by their respective milestones.
- M1 supplies DTO, validation, configuration, migration, redaction, protocol, fixture, and architecture-boundary foundations. It does not claim M2+ daemon, transport, storage, runtime, tool, hook, provider-driver, Tauri, TUI, Plan, VFR, Headroom, or end-to-end behavior.
- Configuration snapshot persistence, daemon reload, and attaching immutable snapshots to live runs remain M3/M4 work. The intended transition remains new-run-only until a later typed and tested decision changes it.
- M1 accepts only `openrouter` and `generic-chat-completion-api` provider kinds. An `openai` provider and OpenAI Responses support require a separately declared driver crate, contract, boundary policy, and owning milestone.
- This closeout does not claim user-visible outcome scenarios that belong to later milestones; those stay governed by the scenario portfolio in [Test-Driven Delivery and Verification](../architecture/10-test-driven-delivery-and-verification.md).

### M1+ disposition of originally deferred hardening

The four items in the historical list below were deliberately not claimed as
M0/M1 enforcement. They were subsequently completed in the separate [M1+
Quality Hardening Evidence](m1-plus-quality-hardening-evidence.md) baseline.
This link preserves the original M0/M1 audit record rather than rewriting it.

### Historical M0/M1 deferred hardening, closed by M1+

The following gaps were explicit deferred work when this M0/M1 evidence was
captured. Their subsequent M1+ completion is recorded above; these historical
entries preserve the original scope and do not describe current enforcement:

1. **Dependency-cycle detection.** At M0/M1, `check_architecture.py` validated declared dependencies and M1 boundary subsets but did not construct the complete workspace graph or contain a controlled cycle fixture.
2. **Executable `test_target` policy.** At M0/M1, the checker required non-empty `test_target` policy text and integration-test presence for active M1 crates, but did not prove the named target existed or executed.
3. **Signature-aware public API analysis.** At M0/M1, DTO/resource-leak protection was primarily literal source-pattern scanning and did not cover aliases, wrappers, re-exports, generic resource types, or unlisted implementation resources.
4. **Coverage-exclusion semantics.** At M0/M1, enabled exclusions required rationale, owner, and equivalent-test metadata, but the checker did not subtract them from the denominator or validate ownership/path against coverage-report data.

## Exceptions and known risks

No M0/M1 lint, coverage, feature-profile, dependency, or architecture exception is enabled for this baseline. The four historical M0/M1 quality-system boundaries above were completed by M1+; they must not be represented as coverage already provided by the earlier M0/M1 checks.
