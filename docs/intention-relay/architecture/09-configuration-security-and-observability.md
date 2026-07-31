# Configuration, Security, and Observability

## Scope

This document defines TOML-only configuration, automatic configuration persistence, open-text credential handling, redaction, configuration snapshots, daemon diagnostics, and operational observability.

It does not introduce remote authentication, cloud secrets management, or multi-user configuration.

## TOML-only configuration

`intention-config` owns:

- TOML parsing;
- schema validation;
- defaults and resolved configuration;
- configuration migrations;
- revisioning;
- `ResolvedConfigDto` and immutable `ConfigSnapshotDto`.

No YAML, JSON, database-only, or UI-only configuration source is authoritative in v1. YAML is reserved for internal plan frontmatter, not application configuration.

## Configuration lifecycle

```mermaid
flowchart LR
  TF[TOML file] --> PA[Parse validate]
  PA --> RC[Resolved config]
  RC --> RV[Config revision]
  RV --> DS[Daemon state]
  DS --> RS[Run snapshot]
  RS --> RT[Run actor]
```

### Rules

- A valid persisted configuration revision is recorded automatically when configuration changes are accepted.
- A new run receives an immutable config snapshot.
- Existing runs do not silently change provider, model, tool policy, VFR, Headroom, workspace, or timeout behavior due to a configuration edit.
- Configuration behavior is documented as either **new-run**, **daemon-restart**, or **future live-reload**. It must never be implied.
- The exact TOML path and platform migration rules are implementation-required before implementation.

## Open-text provider credentials

Provider credentials may be stored in TOML in open text by explicit product decision. This is not equivalent to allowing them to leak through the system.

### Required protections

- configuration files have user-only permissions where the platform supports them, such as `0600` on Unix;
- creation and update code warns or refuses unsafe permissions according to a documented platform policy;
- secrets are excluded from transport DTOs, domain events, run snapshots, plan frontmatter, UI DTOs, tool results, and normal logs;
- errors, logs, and diagnostic bundles use centralized redaction;
- configuration displays do not log values while rendering or validation fails;
- test fixtures use fake credentials only.

## Data classification

| Class | Examples | May persist | May emit to adapters | May log |
| --- | --- | --- | --- | --- |
| Public operational | run status, model ID, plan status, usage. | Yes. | Yes. | Yes, policy-limited. |
| Workspace-sensitive | file path, source/result content. | Yes, scoped. | Only through user-visible tool/session policy. | Redacted or minimized. |
| Secret | API keys, auth headers, credentials. | TOML only by decision. | No. | No. |
| Internal diagnostic | socket path, correlation ID, backtrace. | Limited/audited. | Safe identifier only. | Safe/redacted. |

## Redaction boundary

Redaction is a central reusable policy, not duplicated in providers, tools, Tauri, and TUI.

Every crossing below must apply safe projection before output:

```text
config -> provider
provider error -> event/log
process output -> tool result
storage record -> snapshot
snapshot/event -> transport
transport -> Tauri/TUI presentation
```

A redaction failure is a security defect and must have regression coverage.

## Observability

Daemon-owned observability must expose typed, safe operational data:

- daemon health/readiness/version;
- protocol compatibility status;
- connection/subscription health;
- session/run states and durations;
- queue depth;
- provider/model identity without credentials;
- usage where reported;
- tool lifecycle and policy outcomes;
- hook/VFR/Headroom decisions with safe metadata;
- plan revision/status;
- typed failure/correlation identifiers.

Adapters render observations. They do not infer daemon health from presentation timing.

## Logging and audit

- Domain events are the durable audit of application facts.
- Structured logs are operational diagnostics, not a replacement for domain events.
- Logs have correlation IDs and safe context DTOs.
- No sensitive prompt/configuration content is emitted merely for diagnostics.
- Tool output logging is size-bounded and subject to content policy.
- Audit retains plan/permission/policy facts required to explain an action without claiming proof of external process atomicity.

## Required tests and outcomes

| Requirement | Test evidence | Observable outcome |
| --- | --- | --- |
| TOML validation | Parser/migration fixture tests. | Invalid config returns typed errors without partial state replacement. |
| Snapshot immutability | Run/config integration test. | Active run behavior remains on its starting config revision. |
| New-run configuration | Config update then run test. | Next run receives updated immutable snapshot. |
| Permission safety | Filesystem permission test on Unix. | Created config is user-readable only or fails safely. |
| Redaction | Table-driven secret injection tests. | Secret is absent from every event, snapshot, error, log, and presentation DTO. |
| Safe observability | Daemon status contract test. | Health/usage/tool state is visible without credentials. |
| Persistence | Config revision transaction test. | Accepted TOML revision and event/projection are consistent. |

## Quality-gate integration

`intention-config` is a Tier A coverage target. TOML parsing, migrations, snapshot immutability, permissions, redaction, and safe observability tests are blocking `make verify` inputs. A recognizable fake secret is a mandatory regression fixture across logs, errors, snapshots, events, and adapter DTOs. See [12 Quality Gates and Makefile](12-quality-gates-and-makefile.md).

## Open decisions

- exact TOML layout and include/import policy, if any;
- user experience for config edits and daemon-restart-required changes;
- credential rotation flow;
- event/log retention and diagnostic export policy;
- platform-specific config permission behavior outside Unix.
