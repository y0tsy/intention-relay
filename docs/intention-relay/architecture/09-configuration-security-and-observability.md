# Configuration, Security, and Observability

## Scope

This document defines TOML-only configuration, later configuration persistence, open-text credential handling, redaction, configuration snapshots, daemon diagnostics, and operational observability.

It does not introduce remote authentication, cloud secrets management, or multi-user configuration.

## TOML-only configuration

`intention-config` owns:

- TOML parsing;
- schema validation;
- defaults and resolved configuration;
- configuration migrations;
- the M1 `ConfigRevisionId` and credential-free `ResolvedConfigDto`/`ConfigSnapshotDto` contract foundation.

M1 accepts only `openrouter` and `generic-chat-completion-api` provider kinds. A future `openai` kind is not implied: it requires a separately declared OpenAI Responses driver crate and architecture decision.

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

### M1 contract foundation and M3 startup lifecycle

M1 parses, migrates, validates, and projects configuration. It defines the
immutable, serializable `ConfigSnapshotDto` shape. M3 makes that DTO the
canonical credential-free configuration selection for durable storage and runs:
the daemon composition receives one validated startup snapshot, records it by
`ConfigRevisionId`, and every accepted or terminally promoted run persists its
own immutable selected snapshot/revision.

M3 applies TOML **only at daemon startup**. It neither watches TOML nor applies
an edit to an already-running daemon. A changed TOML file therefore takes effect
only after a restart; the new startup snapshot applies to new runs, while
existing persisted runs retain their recorded revision. Live reload is future
work and must be introduced by an explicit contract, transaction, and outcome
test.

### M3 lifecycle rules

- `ConfigSnapshotDto` is the canonical persisted, credential-free configuration selection; raw TOML and credentials do not enter storage, events, snapshots, or protocol DTOs.
- The composition root accepts one valid snapshot per daemon startup and persists it before recovery/readiness.
- An accepted or promoted run receives an immutable copy of its selected snapshot/revision.
- Existing runs do not silently change provider, model, tool policy, VFR, Headroom, workspace, or timeout behavior due to a configuration edit.
- TOML application is **daemon-restart-only** in M3. The precise user experience for detecting or requesting the restart remains open; future live reload must never be implied.
- Configuration discovery remains platform-standard with a validated explicit absolute-path override; it never falls back to process CWD.

### M4 provider execution policy and startup material

The optional TOML table `[provider.execution]` resolves into the credential-free `ProviderExecutionPolicyDto` included in `ResolvedConfigDto` and therefore in every `ConfigSnapshotDto`. `attempt_timeout_seconds` defaults to `30` and must be in `1..=60`; `max_attempts` defaults to `2` and must be in `1..=2`. Missing policy fields and M3 snapshots lacking the additive policy field decode to those defaults. Runtime owns the fixed 250 ms retry delay, not TOML.

`parse_startup_material` additionally creates opaque `StartupProviderMaterial` for composition. It has no `Debug`, `Display`, serde implementation, or credential accessor and may only be consumed by a selected provider constructor. Safe resolved/snapshot DTOs, persistence, events, protocol, diagnostics, logs, and adapter projections remain credential-free.

These M4 configuration and credential-isolation rules are implemented and
verified at the M4 closure baseline. They remain startup-only behavior; a
follow-on milestone must not imply live reload, credential persistence, or
rotation without new contracts and outcome evidence.

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
| Workspace-sensitive | logical relative workspace path, source/result content. | Yes, scoped. | Only through user-visible tool/session policy. | Redacted or minimized. |
| Secret | API keys, auth headers, credentials. | TOML only by decision. | No. | No. |
| Internal diagnostic | socket path, canonical `CorrelationIdDto`, backtrace. | Limited/audited. | Safe identifier only. | Safe/redacted. |
| Local configuration path | `ConfigPathDto`. | Local configuration operation only. | No, including resolved config/snapshot projections. | No. |

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
| M3 canonical snapshot persistence | Config/storage fixture. | Only a validated credential-free `ConfigSnapshotDto` is accepted and stored by revision. |
| Startup/restart-only application | Daemon composition lifecycle fixture. | The startup snapshot is recorded before recovery/readiness; an on-disk TOML change requires restart and cannot mutate an active run. |
| Run snapshot immutability | Accepted-turn and terminal-promotion integration fixtures. | Started and promoted runs retain their selected immutable config revision. |
| Path selection | Config and platform-state location fixtures. | Config/storage locations use explicit absolute override or platform locations, never CWD. |
| Permission safety | Filesystem permission test on Unix. | Created config is user-readable only or fails safely. |
| Redaction | Table-driven secret injection plus raw SQLite persistence fixtures. | Recognizable fake credentials are absent from configuration-revision JSON, session/run snapshot JSON, event envelopes, errors, logs, and presentation DTOs. |
| Safe observability | Daemon status contract test. | Health/usage/tool state is visible without credentials. |

## Quality-gate integration

`intention-config` remains a Tier A coverage target. TOML parsing, migrations,
M1 snapshot serialization, permissions, redaction, and safe observability tests
are blocking `make verify` inputs. M3 adds canonical snapshot-persistence,
restart-only application, and per-run snapshot integration coverage. A
recognizable fake secret is a mandatory regression fixture across logs, errors,
snapshots, events, and adapter DTOs. See [12 Quality Gates and
Makefile](12-quality-gates-and-makefile.md).

## Open decisions

- exact TOML layout and include/import policy, if any;
- user experience for config edits and daemon-restart-required changes;
- credential rotation flow;
- event/log retention and diagnostic export policy;
- platform-specific config permission behavior outside Unix.
