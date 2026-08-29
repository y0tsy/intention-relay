# M4 Closure Evidence

## Status and purpose

**Closed at immutable implementation baseline
`d2a85370a66d63fc759e4987a74d435ecd5d5115`.**

M4 adds provider-neutral model contracts, private OpenRouter and generic Chat
Completions drivers, durable model facts, one daemon-owned streaming run, and
persistent run-scoped delivery. The baseline passed local `make verify` and
the required Linux and Windows `make ci` matrix before this separate closeout
documentation commit.

M4 decisions and accepted lane integrations are retained in the [M4 execution
charter](../m4.md). The implemented contracts and verification requirements are
defined in [Model Protocol and Providers](../architecture/08-model-protocol-and-providers.md),
[Configuration, Security, and Observability](../architecture/09-configuration-security-and-observability.md),
[Daemon, Transport, and Adapters](../architecture/03-daemon-transport-and-adapters.md),
[Test-Driven Delivery and Verification](../architecture/10-test-driven-delivery-and-verification.md),
and [Quality Gates and Makefile](../architecture/12-quality-gates-and-makefile.md).

| Item | Value |
| --- | --- |
| M4 implementation baseline SHA | [`d2a85370a66d63fc759e4987a74d435ecd5d5115`](https://github.com/y0tsy/intention-relay/commit/d2a85370a66d63fc759e4987a74d435ecd5d5115) (`fix(ci): prevent self-test disk exhaustion`) |
| Baseline contents | Package 1 provider foundation; lanes A–F for durable facts, async transport, runtime execution, run-stream protocol/client, application scheduling, and daemon hosting; followed by config-coverage restoration and the focused CI disk-exhaustion correction. |
| Baseline parent | [`35984f15b9049c966ea8cae5f96c64f16fe2f39f`](https://github.com/y0tsy/intention-relay/commit/35984f15b9049c966ea8cae5f96c64f16fe2f39f) (`test(m4): restore config coverage evidence`) |
| Local verification command | `TMPDIR=/home/data/.intention-relay-quality-tmp make verify` — exit status `0`, executed 2026-08-08 at the implementation SHA. |
| Local verification environment | Linux `7.1.5-201.fc44.x86_64`, `x86_64` GNU/Linux; stable Rust `1.97.1`; coverage nightly `nightly-2026-07-31`. |
| Required CI command | `make ci` on `ubuntu-24.04` and `windows-2025` at the implementation SHA. |
| Linux CI result | Passed, [`ubuntu-24.04 make ci`](https://github.com/y0tsy/intention-relay/actions/runs/31218198652/job/92996535220), completed 2026-08-07T21:14:45Z. |
| Windows CI result | Passed, [`windows-2025 make ci`](https://github.com/y0tsy/intention-relay/actions/runs/31218198652/job/92996535143), completed 2026-08-07T21:20:40Z. |
| Matrix workflow | [`Quality` run 31218198652](https://github.com/y0tsy/intention-relay/actions/runs/31218198652), successful for the exact implementation baseline SHA. |
| CI coverage artifacts | [Linux quality reports](https://github.com/y0tsy/intention-relay/actions/runs/31218198652/artifacts/9009676246) and [Windows quality reports](https://github.com/y0tsy/intention-relay/actions/runs/31218198652/artifacts/9009856851). |
| Closure documentation commit | This separate documentation-only commit, created after baseline verification and CI completion. |

## Verification configuration

### Pinned tools and feature profiles

The local full gate exercised the pinned stable toolchain, the dated nightly
coverage toolchain, `cargo-nextest 0.9.140`, `cargo-llvm-cov 0.8.7`,
`cargo-deny 0.20.2`, `cargo-audit 0.22.2`, `cargo-udeps 0.1.61`,
`cargo-machete 0.9.2`, `cargo-about 0.9.1`, and `cargo-outdated 0.19.0`.

The machine-readable feature policy ran default, `--no-default-features`, and
`--all-features` through check, lint, nextest, doctest, documentation,
architecture, and coverage paths. No critical feature combination is enabled.
Each nextest profile completed **261 tests passed, 0 skipped**; all doctests
passed. `make isolated-release` also checked the `intention-daemon` library and
binary under default and no-default feature profiles.

### Coverage evidence

`make verify` produced branch-aware reports at:

- `quality/reports/coverage-default.json`
- `quality/reports/coverage-no_default.json`
- `quality/reports/coverage-all.json`

The three reports have the same per-crate line results. All active M4 crates
meet their declared tiers in every required profile:

| Crate | Tier | Line coverage |
| --- | ---: | ---: |
| `intention-types` | A, 95% | 100.00% |
| `intention-domain` | A, 95% | 97.58% |
| `intention-protocol` | A, 95% | 97.32% |
| `intention-config` | A, 95% | 97.94% |
| `intention-application` | B, 90% | 95.29% |
| `intention-runtime` | B, 90% | 92.23% |
| `intention-storage` | B, 90% | 94.01% |
| `intention-storage-sqlite` | B, 90% | 93.64% |
| `intention-model` | C, 85% | 95.32% |
| `intention-provider-openrouter` | C, 85% | 90.07% |
| `intention-provider-generic-chat` | C, 85% | 89.91% |
| `intention-transport` | C, 85% | 92.79% |
| `intention-client` | C, 85% | 89.38% |
| `intention` | C, 85% | 88.04% |
| `intention-daemon` library | C, 85% | 87.59% |

The sole enabled coverage exclusion remains `intention-daemon/src/main.rs`.
It is the documented thin child-process adapter whose Windows instrumentation
cannot merge into the parent nextest coverage report; real-binary daemon tests
provide the required equivalent evidence.

## M4 acceptance evidence matrix

| Requirement | Automated proof and recorded result |
| --- | --- |
| Provider-neutral contracts and SDK isolation | `model_contracts`, `m4_execution_contracts`, `openrouter_contracts`, `generic_chat_contracts`, architecture checks, rustdoc public-API checks, and copied-repository fixtures pass. Provider SDK and Tokio resources remain private; only explicit DTOs cross crate, persistence, and protocol boundaries. |
| Provider selection and safe configuration | Config, composition, runtime, and application fixtures prove startup-only opaque credential material, credential-free snapshots, exact persisted/current provider-selection comparison, capability preflight, and safe configuration mismatch failure without a provider call. |
| Stream lifecycle, policy, and durable facts | Model, runtime, domain, storage, and SQLite fixtures prove ordered stream facts, UTF-8-safe 4 KiB assistant batching, typed usage/finish/failure evidence, fixed 250 ms retry sequencing, timeout limits, tool-call denial before M5, atomic fact writes, cursor-bound replay, migration, and terminal fact rejection. |
| One daemon-owned persistent run | `m4_streaming_foundation` uses injected scripted/blocking drivers and a real asynchronous local transport to prove one provider execution, authoritative initial replay, committed live delivery, completion, reconnect/replay, and bounded peer isolation. |
| Cancellation and queued promotion | Runtime and daemon-host fixtures prove `Starting`/`Running -> Cancelling -> Cancelled`, cancellation suppression without late facts, registration/stop linearization, and exactly-once scheduling of an atomically promoted queued `RunId`. |
| Recovery and redaction | Durable restart fixtures prove unfinished work becomes `Interrupted` before readiness and provider work is never resumed. Recognizable fixture credentials are absent from replay, transport frames, events, snapshots, safe errors, diagnostics, and public projections. |
| Cross-platform quality and supply chain | Local `make verify` and green Linux/Windows CI run formatting, lint, feature profiles, tests, doctests, documentation, architecture, coverage, locked metadata, notices, deny, audit, udeps, machete, outdated, and quality self-tests. |

## Scope boundaries retained for later milestones

- M4 supports only `openrouter` and `generic-chat-completion-api` configuration
  kinds. OpenAI Responses and `provider = "openai"` require a separately
  decided driver, contract, and architecture boundary.
- M4 records typed tool-call evidence then fails
  `tool_execution_unavailable`; M5 owns concrete tools, `WorkspaceRoot`, hooks,
  and Plan/Build policy.
- M6 owns Tauri and primary UI delivery. M4's run-stream client/protocol is not
  a claim of a completed desktop adapter.
- Configuration remains startup-only. Live reload, credential persistence or
  rotation, keychain integration, remote transport, multi-user access, idle
  shutdown, upgrades, and automatic external-work resumption remain deferred.

## Exceptions and follow-up

| Item | Disposition |
| --- | --- |
| Provider SDK graph | `openrouter-rs 0.14.0` and `async-openai 0.41.3` are private selected dependencies. Their documented license and duplicate-version exceptions are limited to those dependency paths and must be reassessed if either SDK or its immediate HTTP/random/TLS path changes. |
| Transitive `async-openai` advisories | `RUSTSEC-2025-0012` (`backoff`) and `RUSTSEC-2024-0384` (`instant`) were resolved by the post-M4 `async-openai` 0.41.3 migration, which dropped the `backoff` chain. The acknowledgements were removed from `deny.toml` and the audit command; `cargo audit` is clean. |
| `async-openai` outdated hold | Removed by the post-M4 `async-openai` 0.41.3 migration; `cargo outdated` is clean and `check_deny_policy.py` now rejects new advisory ignores or outdated holds. |
| CI self-test disk exhaustion | The baseline's focused correction removes only generated LLVM coverage artifacts before copied-repository self-tests and shares the controller target directory with those isolated copies. The exact baseline passed the full Linux and Windows matrix after the change. |

## Final baseline recording rule

The immutable baseline above was committed, verified locally with `make
verify`, and accepted by the completed Linux/Windows CI matrix. This closeout
commit is documentation only; later changes must establish their own baseline
and evidence.
