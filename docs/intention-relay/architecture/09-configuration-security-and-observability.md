# Configuration, Security, and Observability

**Current policy.**

This document defines TOML-only configuration, configuration persistence, open-text credential handling, redaction,
configuration snapshots, daemon diagnostics, and operational observability. It does not introduce remote authentication,
cloud secrets management, or multi-user configuration.

## TOML-only configuration

`intention-config` owns TOML parsing, schema validation, defaults and resolved configuration, and the M1
`ConfigRevisionId` and credential-free `ResolvedConfigDto`/`ConfigSnapshotDto` contract foundation.

M1/M4 accept only `openrouter` and `generic-chat-completion-api` provider kinds. A future canonical Responses kind is
`responses`, not `openai`; any `openai` spelling is only a future parse-time alias under architecture 22 and does not
amend M1/M4.

No YAML, JSON, database-only, or UI-only configuration source is authoritative in v1. YAML is reserved for internal plan
frontmatter, not application configuration.

## Configuration lifecycle

```mermaid
flowchart LR
  TF[TOML file] --> PA[Parse validate]
  PA --> RC[Resolved config]
  RC --> RV[Config revision]
  RV --> DS[Daemon state]
  DS --> RS[Run records]
  RS --> RT[Run actor]
```

### M1 contract foundation and M3 startup lifecycle

M1 parses, validates, and projects configuration. It defines the immutable, serializable `ConfigSnapshotDto` shape. M3
makes that DTO the canonical credential-free configuration selection for durable storage and runs: the daemon
composition receives one validated startup snapshot, records it by `ConfigRevisionId`, and every accepted run persists
its own immutable selected snapshot/revision.

M3 applies TOML **only at daemon startup**. It neither watches TOML nor applies an edit to an already-running daemon. A
changed TOML file therefore takes effect only after a restart; the new startup snapshot applies to new runs, while
existing persisted runs retain their recorded revision. Controlled live reload is the accepted future direction
([architecture 25](25-configuration-provider-control-plane.md)) and must be introduced by an explicit contract,
transaction, and
outcome test; it affects fresh runs only and never mutates a recorded snapshot.

### M3 lifecycle rules

-  `ConfigSnapshotDto` is the canonical persisted, credential-free configuration selection; raw TOML and credentials do
not enter committed records, frames, or protocol DTOs.
- The composition root accepts one valid snapshot per daemon startup and persists it before recovery/readiness.
- An accepted run receives an immutable copy of its selected snapshot/revision.
-  Existing runs do not silently change provider, model, tool policy, VFR, Headroom, workspace, or timeout behavior due
to a configuration edit.
-  TOML application is **daemon-restart-only** in M3: the daemon neither watches TOML nor applies an edit to an
already-running daemon, and a changed TOML file takes effect only after a restart, where the new startup snapshot
applies to new runs while existing persisted runs retain their recorded revision. The precise user experience for
detecting or requesting the restart remains open; controlled live reload is the accepted future direction and must
never be implied by M3/M4 behavior.
-  Configuration discovery remains platform-standard with a validated explicit absolute-path override; it never falls
back to process CWD.

### M4 provider execution policy and startup material

The optional TOML table `[provider.execution]` resolves into the credential-free `ProviderExecutionPolicyDto` included
in `ResolvedConfigDto` and therefore in every `ConfigSnapshotDto`. `attempt_timeout_seconds` defaults to `30` and must
be in `1..=60`; `max_attempts` defaults to `2` and must be in `1..=2`. Missing policy fields in a fresh document decode
to those defaults; `provider_execution` is a required field on the resolved and snapshot wire shapes, so persisted
snapshots always carry the effective policy explicitly. Runtime owns the fixed 250 ms retry delay, not TOML.

`parse_startup_material` additionally creates opaque `StartupProviderMaterial` for composition. It has no `Debug`,
`Display`, serde implementation, or credential accessor and may only be consumed by a selected provider constructor.
Safe resolved/snapshot DTOs, committed records, frames, protocol, diagnostics, logs, and adapter projections remain
credential-free.

These M4 configuration and credential-isolation rules are implemented and verified at the M4 closure baseline. They
remain startup-only behavior; a follow-on milestone must not imply live reload, credential persistence, or rotation
without new contracts and outcome evidence.

### M5+ provider context-window policy

The `[provider]` table carries the dynamic context-window policy as the optional `context_window_tokens` field. It
defaults to `250000`, and resolution requires a positive window; a value outside that range fails closed with the typed
`invalid_provider_context_window_tokens` validation error. The policy resolves into the credential-free
`ContextWindowPolicyDto` included in `ResolvedConfigDto` and therefore in every `ConfigSnapshotDto`. [Architecture
08](08-model-protocol-and-providers.md) owns the window mechanics that consume it.

## Open-text provider credentials

Provider credentials may be stored in TOML in open text by explicit product decision. This is not equivalent to allowing
them to leak through the system.

### Required protections

- configuration files have user-only permissions where the platform supports them, such as `0600` on Unix;
- creation and update code warns or refuses unsafe permissions according to a documented platform policy;
-  secrets are excluded from transport DTOs, the durable `runs`, `turns`, `messages`, `tool_results`, and
`configuration_revisions` records, plan frontmatter, UI DTOs, and normal logs. `execute` is trusted-local and may
inherit the invoking process environment; environment variables are not name-filtered or copied into evidence or logs;
- errors, logs, and diagnostic bundles use centralized redaction;
- configuration displays do not log values while rendering or validation fails;
-  hermetic test fixtures use fake credentials only; the single opt-in live-provider e2e channel
([12 Quality Gates and Makefile](12-quality-gates-and-makefile.md)) injects a real credential from the environment
(or a CI repository secret) into a private temporary configuration file only and remains subject to every protection
in this section.

## Limits by precedent

Every numeric value is classified before activation as an intrinsic representation or protocol bound, typed capacity
availability, or a liveness safeguard with recorded rationale. Product ceilings, hidden admission policy, retry budgets,
and truncation of a successful result to fit a ceiling are not permitted; a new limit needs a real demonstrated
precedent or a liveness reason, and the reasoning is recorded with the value. No runtime content scanning or filtering
for credential-shaped strings exists: secrets are protected structurally, and a recognizable fake secret is a
regression fixture rather than a scan target.

The current bounds are:

| Bound | Class and purpose |
| --- | --- |
| Transport message cap (`MAX_MESSAGE_BYTES`, 1 MiB) | Liveness: the single owner of the envelope bound; rejects an over-size frame before unbounded allocation, and an over-size correlated response is answered with the typed `local_protocol_message_too_large` failure instead of a silent close. |
| Tool read and output windows (`MAX_TOOL_OUTPUT_BYTES`, `MAX_EDIT_TARGET_BYTES`, `MAX_GREP_AGGREGATE_BYTES`) | Representation: bounds one tool read or rendered result; a cut is marked, never hidden. |
| Assistant-message bound (`MAX_ASSISTANT_CONTENT_BYTES`) | Representation: bounds one committed assistant message; long assistant text is split at this size. |
| Process timeout and drain windows (`EXECUTE_TIMEOUT` 30 s, `READER_DRAIN_GRACE` 5 s) | Liveness: a child process that stops producing progress or never exits cannot hang the loop. |
| Provider progress and retry timeouts | Liveness: a live provider request that stops producing progress fails typed. |
| Subscriber queue and write deadline (`SUBSCRIBER_QUEUE_CAPACITY` 64, `SUBSCRIBER_WRITE_DEADLINE` 10 s) | Liveness: isolates a slow local peer from execution, persistence, and healthy delivery. |

## Data classification

| Class | Examples | May persist | May emit to adapters | May log |
| --- | --- | --- | --- | --- |
| Public operational | run status, model ID, plan status, usage. | Yes. | Yes. | Yes, policy-limited. |
| Workspace-sensitive | logical relative workspace path, source/result content. | Yes, scoped. | Only through user-visible tool/session policy. | Redacted or minimized. |
| Secret | API keys, auth headers, credentials. | TOML only by decision. | No. | No. |
| Internal diagnostic | socket path, canonical `CorrelationIdDto`, backtrace. | Limited/audited. | Safe identifier only. | Safe/redacted. |
| Local configuration path | `ConfigPathDto`. | Local configuration operation only. | No, including resolved config/snapshot projections. | No. |

## Redaction boundary

Redaction is a central reusable policy, not duplicated in providers, tools, Tauri, and TUI. Every crossing below must
apply safe projection before output:

```text
config -> provider
provider error -> event/log
process output -> tool result
committed record -> snapshot
snapshot/frame -> transport
transport -> Tauri/TUI presentation
```

A redaction failure is a security defect and must have regression coverage.

## Observability

Daemon-owned observability must expose typed, safe operational data:

- daemon health/readiness with the current DTO schema version;
- protocol compatibility status;
- connection/subscription health;
- session/run states and durations;
- pending turns and their count;
- provider/model identity without credentials;
- usage where reported;
- tool lifecycle and policy outcomes;
- VFR/Headroom decisions with safe metadata;
- plan revision/status;
- typed failure/correlation identifiers.

Adapters render observations. They do not infer daemon health from presentation timing.

## Logging and audit

-  The committed current-state records (`runs`, `turns`, `messages`, `tool_results`, `configuration_revisions`) are
the durable audit of application facts; structured logs are operational diagnostics, not a replacement for them.
-  Logs have correlation IDs and safe context DTOs, and no sensitive prompt/configuration content is emitted merely for
diagnostics.
- Tool output logging is size-bounded and subject to content policy.
-  Audit retains plan/permission/policy facts required to explain an action without claiming proof of external process
atomicity.

## Required tests and outcomes

| Requirement | Test evidence | Observable outcome |
| --- | --- | --- |
| TOML validation | Parser fixture tests. | Invalid config returns typed errors without partial state replacement. |
| M3 canonical snapshot persistence | Config/storage fixture. | Only a validated credential-free `ConfigSnapshotDto` is accepted and stored by revision. |
| Startup/restart-only application | Daemon composition lifecycle fixture. | The startup snapshot is recorded before recovery/readiness; an on-disk TOML change requires restart and cannot mutate an active run. |
| Run snapshot immutability | Accepted-turn integration fixtures. | Started runs retain their selected immutable config revision. |
| Path selection | Config and platform-state location fixtures. | Config/storage locations use explicit absolute override or platform locations, never CWD. |
| Permission safety | Filesystem permission test on Unix. | Created config is user-readable only or fails safely. |
| Redaction | Table-driven secret injection plus raw SQLite persistence fixtures. | Recognizable fake credentials are absent from configuration-revision JSON, the committed `runs`, `turns`, `messages`, and `tool_results` rows, errors, logs, and presentation DTOs. |
| Safe observability | Daemon status contract test. | Health/usage/tool state is visible without credentials. |

## Quality-gate integration

`intention-config` remains subject to its `standard` tier floor. TOML parsing, M1 revision serialization, permissions,
redaction,
and safe observability tests are blocking `make verify` inputs; M3 adds canonical revision persistence, restart-only
application, and per-run configuration-selection coverage. A recognizable fake secret is a mandatory regression
fixture across logs, errors, persisted state, and adapter DTOs. See [12 Quality Gates and
Makefile](12-quality-gates-and-makefile.md).

## Open decisions

- exact TOML layout and include/import policy, if any;
- user experience for config edits and daemon-restart-required changes;
- credential rotation flow;
- transcript/log retention and diagnostic export policy;
- platform-specific config permission behavior outside Unix.
