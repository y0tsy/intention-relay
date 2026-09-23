# ADR 0040: Opt-in live-provider end-to-end channel

## Status

Accepted as an additive, non-blocking verification channel for the ordinary
production tool loop completed by
[ADR 0039](0039-request-side-tool-advertisement.md), within the retrospective
scope of [ADR 0035](0035-m5plus-complete-foundation-activation.md). It
introduces the repository's first deliberately `#[ignore]`d integration test
target (two ignored test functions): source that is compiled and reviewed with
the workspace but executes only under an explicit operator opt-in. It is not a
slice activation; the M5+ Slice 3 reservations and all reserved tags remain
untouched and reserved.

## Scope and supersession

In scope is exactly one opt-in live-provider e2e channel:

- one ignored daemon integration test target,
  `crates/intention-daemon/tests/real_api_e2e.rs`, whose two ignored test
  functions spawn the real daemon binary, drive it through the real local
  transport, and execute a real model tool loop (the positive tool-loop/replay
  case and the negative invalid-credential case) against a real provider API
  over HTTPS;
- one local entry point, `make e2e-real-api`, which takes the credential from
  the process environment (a local gitignored `.env` file, loaded by the
  target, may supply variables that are not already set);
- one manual workflow entry point, `.github/workflows/real-api-e2e.yml`, which
  is `workflow_dispatch`-only and takes the credential from the repository
  secret `REAL_API_E2E_PROVIDER_KEY`.

Nothing is superseded. [ADR 0039](0039-request-side-tool-advertisement.md)
keeps hermetic tests as the acceptance evidence; this record adds complementary
manual evidence and does not weaken, replace, or extend the blocking gates. Its
statement that the change adds no live provider dependency remains true for the
blocking and hermetic suites.

## Decision

1. The channel is exactly one `#[ignore]`d integration test target,
   `crates/intention-daemon/tests/real_api_e2e.rs`, carrying two ignored test
   functions: the positive live tool-loop/replay case and the negative
   invalid-credential case. `#[ignore]` is the repository's single convention
   for a test that needs a real credential or live provider work; no live test
   is added un-ignored.
2. The tests run only when `INTENTION_REAL_API_E2E=1` is present together
   with `INTENTION_REAL_API_KEY` (the provider credential) and
   `INTENTION_REAL_API_MODEL` (the model identifier). The default provider kind
   is `generic-chat-completion-api`, `openrouter` is selectable through the
   optional `INTENTION_REAL_API_KIND`, and the optional
   `INTENTION_REAL_API_ENDPOINT` overrides the endpoint for self-hosted
   providers. These variables are test-process inputs only: no production crate
   reads them, and no provider default changes because of them.
3. Two entry points exist, and both are opt-in:
   - `make e2e-real-api` requires `INTENTION_REAL_API_KEY` and
     `INTENTION_REAL_API_MODEL` from the environment (or from a local gitignored
     `.env` file loaded by the target), exports the
     `INTENTION_REAL_API_E2E=1` opt-in itself, forwards the optional provider
     kind and endpoint selectors, and runs only the ignored target;
   - `.github/workflows/real-api-e2e.yml` is manual-only (`workflow_dispatch`,
     never triggered by `push`, `pull_request`, or `schedule`) and supplies the
     credential only from the repository secret `REAL_API_E2E_PROVIDER_KEY`.
4. The live run proves the ordinary production tool loop end to end against a
   real provider: the daemon builds and sends a request that advertises the six
   active registered tools (ADR 0039); the provider returns a real tool call;
   the daemon validates and executes it through the real typed registry under
   `WorkspaceRoot`; the durable `ToolCallRecorded` and `ToolResultRecorded`
   facts commit before publication; the exchange continues and the run reaches
   a terminal outcome.
5. The test also proves durability and replay: restarting the daemon and
   replaying the run returns the recorded tool call and result and never
   re-executes the tool.
6. Credential handling is closed. The credential travels only from the
   environment (or the CI repository secret) into a private temporary daemon
   configuration file created in a platform-native temporary directory, with
   `0600` permissions on Unix, is passed to the spawned daemon for the duration
   of the run, and is removed with the temporary directory. It is never logged,
   never persisted in durable state, and never uploaded; the only uploaded
   artifact is the credential-free run report under
   `quality/reports/real-api-e2e/`, which is gitignored locally. The test
   asserts the credential's absence from durable facts, snapshots, daemon logs,
   and state bytes.
7. A negative credential case uses a recognizable invalid credential and
   asserts a typed failure mapping: the failure surfaces as a typed daemon or
   provider error and never as an untyped panic, a hang, or an echoed
   credential.
8. Policy treatment is explicit and additive. `quality/architecture.toml`
   declares the `real_api_e2e` target for `intention-daemon` (the architecture
   checker still requires declared targets to equal Cargo metadata). Ignored
   tests are compiled by every test build but are never executed by
   `make test`, `make coverage`, or `make verify`. The channel adds no blocking
   CI job, no required status check, no coverage tier, no exclusion, no feature
   profile, and no dependency.

## Invariants

1. Opt-in only. No default target, no push/pull-request/schedule trigger, no
   required check, and no Makefile path by which `make quick`, `make check`,
   `make verify`, `make ci`, or any `ci-*` alias invokes the live channel.
2. `#[ignore]` is the only live-test convention. A test that needs a real
   credential or live provider work must be ignored and must never enter the
   hermetic blocking suite.
3. The channel observes the existing production path; it introduces no
   test-only production branch, feature flag, second loop, or provider driver.
4. The credential remains an operator-supplied test input. Environment (or CI
   secret) to private temporary configuration to daemon process is the only
   flow; nothing durable or published carries it.
5. The nine required status checks and the blocking Quality workflow remain
   exactly as documented; the live channel never becomes one of them and never
   alters the `ci-*` verification aliases.
6. Additive only: no local protocol, public DTO schema, wire format, storage
   schema, canonical record, tag, or production error-code change.

## Compatibility

- Local protocol 1.1, public DTO schema 1.1, TOML configuration schema 1, and
  the single live SQLite schema are unchanged. The channel is test-side
  verification, not a contract change.
- M3/M4/M5 recorded history, replay, durable facts, snapshots, and evidence are
  unchanged, and a live run creates only ordinary current-version run state in
  its disposable test database.
- M5+ exit invariants are preserved: no M6-M9 boundary behavior, no second
  runtime, registry, scheduler, persistence authority, or sandbox, and the
  ADR 0036 Slice 3 reservations (`tool-descriptor-revision` `0x0301`,
  `tool-registry-revision` `0x0302`, `model-tool-loop-v1` `0x0303`) remain
  `ReservedForSlice3`.
- The [ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md)
  single-version policy is unaffected: the channel opens no older schema and
  carries no migration or compatibility branch.

## Security and failure behavior

- The channel performs no live work unless the opt-in environment is present;
  `make e2e-real-api` refuses to run without `INTENTION_REAL_API_KEY` and
  `INTENTION_REAL_API_MODEL`, and there is no silent fallback to a fake
  provider, a different provider, or a different model.
- An invalid credential fails closed with a typed error, and the negative
  credential case asserts that mapping. No raw credential value appears in the
  error, log, or assertion output.
- Provider network failures, timeouts, and refusals follow the existing
  normalized safe-error path and the production attempt policy; the test
  reports the failure category, never the credential or provider request
  headers.
- The temporary configuration file is private (`0600` where the platform
  supports it), lives under a platform-native temporary directory, and is
  removed after the run; no credential is written to the repository or the
  workspace.
- A failed live run fails only the manual invocation that started it. It cannot
  fail a pull request, block a merge, or change a required status check.

## Non-goals

This decision does not add:

- a required or scheduled live-provider check, or any live work in
  `make quick`, `make check`, `make verify`, `make ci`, or a `ci-*` alias;
- a new provider driver, an OpenAI Responses driver, or adapter behavior beyond
  the ADR 0039 advertisement translation;
- credential persistence, keychain integration, rotation, or controlled
  reload;
- recording of prompts, completions, tool content, provider traffic, or
  credentials; only the run summary metadata (date, commit, provider, model,
  and workflow run URL) is recorded;
- coverage credit: live runs never contribute to crate coverage thresholds and
  never justify a coverage exclusion;
- changes to the hermetic acceptance gates that remain the required evidence.

## Affected documents

- `decisions/README.md`
- `architecture/09-configuration-security-and-observability.md`
- `architecture/10-test-driven-delivery-and-verification.md`
- `architecture/11-implementation-roadmap.md`
- `architecture/12-quality-gates-and-makefile.md`
- `reconciliation/README.md`
- `reconciliation/source-of-truth-matrix.md`
- `reconciliation/evidence-register.md`
- `quality/architecture.toml`
- `quality/self_test.py`
- `Makefile` (`e2e-real-api`)
- `.github/workflows/real-api-e2e.yml`
- `crates/intention-daemon/tests/real_api_e2e.rs`

## Evidence

| Requirement | Evidence anchor |
| --- | --- |
| Ignored live target declared and enforced as opt-in | `quality/architecture.toml` (`intention-daemon` `real_api_e2e`); `quality/self_test.py` `test_adr_0040_live_provider_e2e_record_exists_and_is_indexed`, `test_real_api_e2e_workflow_is_manual_only`, `test_real_api_e2e_target_is_opt_in_only` |
| Manual-only workflow | `.github/workflows/real-api-e2e.yml` (`workflow_dispatch` only; no `push`, `pull_request`, or `schedule` trigger) |
| Local opt-in entry point | `Makefile` `e2e-real-api`, never a prerequisite of `quick`, `check`, `verify`, `ci`, or a `ci-*` alias |
| Live tool loop, durable facts, restart replay, credential absence | `crates/intention-daemon/tests/real_api_e2e.rs` (ignored; opt-in via `INTENTION_REAL_API_E2E=1`, `INTENTION_REAL_API_KEY`, `INTENTION_REAL_API_MODEL`) |
| Reconciliation evidence | EVD-063 (`reconciliation/evidence-register.md`, `Planned` until a live run URL is recorded) |

Required gates remain `make quick`, `make verify`, `docs-check`, and the
Linux/Windows CI matrix; the live channel is never one of them.

## Research provenance

The channel is delivery-integrity provenance for the ADR 0039 request-side
advertisement completion (PR-E2E workstream E3), not a new research direction:
[`m4plus_concept.md`](../m4plus_concept.md) does not require a live-provider
test. It follows the manual-only, non-blocking `quality-benchmark` workflow
precedent recorded in architecture 12 and the open-text provider credential
protections in architecture 09 (user-only file permissions; credentials never
in durable or published output). It activates no slice and changes no ADR
0035-0039 decision.
