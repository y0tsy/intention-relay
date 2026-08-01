# M1+ Quality Hardening Evidence

## Status and purpose

**Closed enforcement baseline.** M1+ is the quality-policy hardening milestone
between M1 contracts/configuration/workspace skeleton and M2 functional work.
It closes the four quality-system limitations explicitly recorded, but not
claimed, in [M0/M1 Closure Evidence](m0-m1-closure-evidence.md). It adds no M2
daemon, transport, storage, runtime, adapter, or provider-driver behavior.

The M1+ implementation baseline is intentionally separate from this evidence
documentation commit. The SHA below identifies the exact enforcement source tree
that passed full verification before this closeout was added.

| Item | Value |
| --- | --- |
| M0 quality foundation | `f2bb35d040dcec93639d6715fc06429b9070e1e3` |
| M1 contract baseline | `dd7c5e7b5db7131fee7ee015822d905964fe07a6` |
| M1+ graph/target commit | `6a225001997791c8872cfec80f971a1f1f0f390d` |
| M1+ public API commit | `93fc80e1534422ed0cfb58acfc3de34335b340c5` |
| M1+ implementation baseline | `fb84bd50a19094b72870ca55bb478f257175d1a5` |
| Verification command | `make verify` |
| Result | Exit status `0` |
| Evidence capture completed | `2026-08-01T15:09:25Z` |
| Host environment | Linux `7.1.5-200.fc44.x86_64`, `x86_64` GNU/Linux |
| Stable Rust | `rustc 1.97.1 (8bab26f4f 2026-07-14)` |
| Nightly Rust | `rustc 1.99.0-nightly (8ab9fdff5 2026-07-30)`, `nightly-2026-07-31` |

## Verification configuration

### Pinned tools

| Tool | Verified version |
| --- | --- |
| `cargo-nextest` | `0.9.140` |
| `cargo-llvm-cov` | `0.8.7` |
| `cargo-deny` | `0.20.2` |
| `cargo-audit` | `0.22.2` |
| `cargo-udeps` | `0.1.61` |
| `cargo-machete` | `0.9.2` |
| `cargo-outdated` | `0.19.0` |

`make tools-check` also verified the pinned nightly `rustdoc` required for the
semantic public-contract gate. No new third-party quality tool was introduced.

### Feature profiles and reports

`make verify` exercised check, lint, tests, doctests, docs, architecture,
coverage, dependencies, and isolated expected-failure fixtures under all
machine-readable profiles:

| Profile | Cargo flags | Result |
| --- | --- | --- |
| `default` | _(default features)_ | Pass |
| `no_default` | `--no-default-features` | Pass |
| `all` | `--all-features` | Pass |

No critical feature combination is enabled. M1+ retains M1's four active Tier A
crates and no enabled coverage exclusions.

`make coverage` generated these branch-aware artifacts:

- `quality/reports/coverage-default.json`
- `quality/reports/coverage-no_default.json`
- `quality/reports/coverage-all.json`

| Profile | `intention-types` | `intention-domain` | `intention-protocol` | `intention-config` |
| --- | ---: | ---: | ---: | ---: |
| `default` | 427/427, 100.00% | 253/253, 100.00% | 169/169, 100.00% | 454/465, 97.63% |
| `no_default` | 427/427, 100.00% | 253/253, 100.00% | 169/169, 100.00% | 454/465, 97.63% |
| `all` | 427/427, 100.00% | 253/253, 100.00% | 169/169, 100.00% | 454/465, 97.63% |

All Tier A line results satisfy the 95% threshold. Branch coverage is present
in every report: `intention-config`, `intention-domain`, and
`intention-protocol` report 100.000%; `intention-types` reports 84.375%. M1+
does not introduce a branch-percentage threshold.

## Acceptance evidence matrix

| M1+ criterion | Implementation and fixture mapping | Gate and result |
| --- | --- | --- |
| Workspace cycles are rejected as an architectural rule | `quality/check_architecture.py` constructs the workspace graph from Cargo metadata and reports deterministic closed paths. `test_workspace_dependency_cycle` adds a policy-aligned normal dependency edge. | `make architecture` and `make quality-self-test` passed; fixture asserts `intention-types -> intention-protocol -> intention-domain -> intention-types`. |
| Active test declarations are executable and accountable | `quality/architecture.toml` adds exact `test_targets`; the checker compares them to Cargo integration targets. `test_executable_test_target_policy` covers unknown, duplicate, and skeleton declarations. | `make architecture` passed. The standard all-targets `make test` path executed all declared targets in every profile. |
| Public contracts have semantic resource/SDK protection | `quality/check_public_api.py` generates pinned-nightly rustdoc JSON for active crates and walks public reachable type surfaces. `test_signature_aware_public_api_leaks` covers alias, tuple wrapper, nested generic, function signature, and re-export leaks. | `make architecture` and `make quality-self-test` passed. |
| Exclusions are owned exact report files and affect coverage arithmetic | `quality/check_coverage.py` validates exact relative source path, owner, report occurrence, and non-empty remaining denominator. `test_coverage_exclusion_semantics` covers a valid denominator reduction plus missing metadata, absolute/traversing, wrong/inactive owner, unreported, out-of-source, duplicate, and all-source cases. | `make coverage` and `make quality-self-test` passed. |
| Existing M0/M1 quality guarantees remain blocking | Root Makefile keeps `architecture` in `check`, `coverage` and `quality-self-test` in `verify`, and `ci` as the `verify` alias. | Final `make verify` passed. |

## Closed prior limitations

The following items are now implemented in M1+ and are no longer deferred to
M2+:

1. **Dependency cycles:** full workspace graph traversal with cycle-path
   failures and a policy-aligned expected-failure fixture.
2. **Executable `test_target` policy:** exact machine-readable integration
   targets compared to Cargo metadata, with all-targets test execution retained
   by `make test`.
3. **Public API analysis:** pinned-nightly rustdoc JSON analysis of public
   aliases, fields, wrappers, nested generics, signatures, and re-exports.
4. **Coverage exclusion semantics:** exact owned report-file validation and
   denominator subtraction, with misuse fixtures.

## Scope boundaries and remaining work

M1+ hardens the quality system only. It does not alter the M1 public contract
surface or close any M2 functional scope. The repository still has only four
active Tier A production crates; all later planned v1 crates remain M1 skeletons.

Still deferred to owning functional milestones:

- M2 local transport, shared client, daemon bootstrap, protocol negotiation,
  reconnect, and health/session outcomes;
- M3 persistence, sessions, events, snapshots, queue, and recovery;
- M4 model runtime and provider drivers;
- M5+ tools, hooks, adapters, Plan, VFR, Headroom, and end-to-end hardening.

Future quality-policy work may extend forbidden type prefixes, add active crate
targets, enable a reviewed exact-file coverage exclusion, or add critical feature
combinations. Each such change must update policy, fixtures, and verification
evidence in the same slice.

## Exceptions and known risks

There are no enabled coverage exclusions and no approved M1+ quality-gate
exception. Rustdoc JSON is an unstable nightly interface, so the policy keeps a
dated nightly pin and `tools-check` validates availability before architecture
verification. A toolchain upgrade remains a reviewed quality-policy change, not
an implicit compatibility assumption.
