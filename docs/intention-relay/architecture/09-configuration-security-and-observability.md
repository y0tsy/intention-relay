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
  DS --> RS[Run snapshot]
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
not enter storage, events, snapshots, or protocol DTOs.
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
Safe resolved/snapshot DTOs, persistence, events, protocol, diagnostics, logs, and adapter projections remain
credential-free.

These M4 configuration and credential-isolation rules are implemented and verified at the M4 closure baseline. They
remain startup-only behavior; a follow-on milestone must not imply live reload, credential persistence, or rotation
without new contracts and outcome evidence.

### M5+ provider context-window policy

The `[provider]` table carries the dynamic context-window policy as the optional `context_window_tokens` and
`context_capacity_tokens` fields. `context_window_tokens` defaults to `250000` and `context_capacity_tokens` defaults to
`1000000`; resolution requires a positive capacity and `0 < context_window_tokens < context_capacity_tokens`, and a
value outside that range fails closed with the typed `invalid_provider_context_window_tokens` or
`invalid_provider_context_capacity_tokens` validation error. The policy resolves into the credential-free
`ContextWindowPolicyDto` included in `ResolvedConfigDto` and therefore in every `ConfigSnapshotDto`. [Architecture
08](08-model-protocol-and-providers.md) owns the window mechanics that consume it ([ADR
0018](../decisions/0018-dynamic-context-window-and-prompt-caching.md)).

### M5+ typed-edit rendering and reload status (reverted)

The M5+ Slice 2 activation rendered typed-edit candidate documents inside `intention-config` (the accepted decision:
render from the safe snapshot AST inside the crate) and added a `ConfigurationProjectionDto.reload_status` vocabulary.
Slice 2 was reverted: the typed-edit renderer, the private credential-restore helper, the `configuration_edit_invalid`
failure, and the reload-status vocabulary are removed from the tree. Configuration editing is again an accepted future
direction ([architecture
25](25-configuration-provider-control-plane.md)) with no live implementation, and controlled reload is again only the
accepted future direction described
in the M3 lifecycle rules above. A re-introduction through a new activating specification must restore the recorded
rules:

-  a typed-edit candidate is rendered from the safe snapshot AST inside the configuration crate, and the composition
maps protocol typed-edit operations into that crate's credential-free edit-operation type without doing TOML rendering
itself;
-  values carrying TOML-significant characters are escaped by the serializer instead of producing an unparseable
document;
- configuration fields the edit does not name survive the edit unchanged;
-  a value the configuration shape cannot represent fails with a typed `configuration_edit_invalid` error instead of a
generic `invalid_config_toml` parse failure;
-  the rendered document stays credential-free by construction; only the private channel may re-insert
`provider.credential` as a TOML value before the candidate flows through the unchanged server-side reload contract
(`prepare`, `parse_candidate`, `reject_catalog_affecting_edits`), and a document-shape failure in the restore helper is
a typed validation error (`invalid_config_toml` or `invalid_config_schema`), never a credential-free document that a
caller could mistake for a configured one;
-  the reload status projection is a closed enum validated by serde at decode, where an unknown status is rejected with
`configuration_projection_invalid` and a consumer can match the status exhaustively.

## Open-text provider credentials

Provider credentials may be stored in TOML in open text by explicit product decision. This is not equivalent to allowing
them to leak through the system.

### Required protections

- configuration files have user-only permissions where the platform supports them, such as `0600` on Unix;
- creation and update code warns or refuses unsafe permissions according to a documented platform policy;
-  secrets are excluded from transport DTOs, domain events, run snapshots, plan frontmatter, UI DTOs, tool results, and
normal logs. `execute` is trusted-local and may inherit the invoking process environment; environment variables are not
name-filtered or copied into evidence or logs;
- errors, logs, and diagnostic bundles use centralized redaction;
- configuration displays do not log values while rendering or validation fails;
-  hermetic test fixtures use fake credentials only; the single opt-in live-provider e2e channel ([ADR
0007](../decisions/0007-opt-in-live-provider-e2e.md)) injects a real credential from the environment (or a CI repository
secret) into a private temporary configuration file only and remains subject to every protection in this section.

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
- pending turns and their count;
- provider/model identity without credentials;
- usage where reported;
- tool lifecycle and policy outcomes;
- hook/VFR/Headroom decisions with safe metadata;
- plan revision/status;
- typed failure/correlation identifiers.

Adapters render observations. They do not infer daemon health from presentation timing.

## Logging and audit

-  Domain events are the durable audit of application facts; structured logs are operational diagnostics, not a
replacement for domain events.
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
| Redaction | Table-driven secret injection plus raw SQLite persistence fixtures. | Recognizable fake credentials are absent from configuration-revision JSON, session/run snapshot JSON, event envelopes, errors, logs, and presentation DTOs. |
| Typed-edit rendering and reload status | Config and composition typed-edit fixtures. | Values with TOML-significant characters round-trip through the rendered document, fields the edit does not name survive, non-representable values yield typed edit errors, and an unknown reload status is rejected at decode. |
| Safe observability | Daemon status contract test. | Health/usage/tool state is visible without credentials. |

## Quality-gate integration

`intention-config` remains subject to its `standard` tier floor ([ADR
0016](../decisions/0016-per-crate-coverage-tiers.md)). TOML parsing, M1 snapshot serialization, permissions, redaction,
and safe observability tests are blocking `make verify` inputs; M3 adds canonical snapshot-persistence, restart-only
application, and per-run snapshot integration coverage. A recognizable fake secret is a mandatory regression fixture
across logs, errors, snapshots, events, and adapter DTOs. See [12 Quality Gates and
Makefile](12-quality-gates-and-makefile.md).

## Open decisions

- exact TOML layout and include/import policy, if any;
- user experience for config edits and daemon-restart-required changes;
- credential rotation flow;
- event/log retention and diagnostic export policy;
- platform-specific config permission behavior outside Unix.

## Post-M4 selection and observability boundary

This document preserves M3/M4 startup-only TOML application. The configuration/provider control-plane cluster
(controlled live reload, credential rotation, profile editing, discovery, pricing, and health behavior) is an accepted
future direction and the
[Implementation Roadmap](11-implementation-roadmap.md), and adds no implemented behavior. Future execution meanings and
attempt evidence must remain credential-free and exclude SDK objects, process handles, kernel state, bridge grants, raw
provider/MCP payloads, and private endpoint material.

Future observability may expose typed capacity outcomes and safe attempt references, but it cannot convert logs,
notifications, activity, or provider responses into authority. Redaction rules apply to every new persistence, protocol,
diagnostic, and identity surface.

## Execution-meaning historical record

Architecture 14 ([Run execution meaning and historical
compatibility](14-run-execution-meaning-and-historical-compatibility.md)) is the historical record of the removed
execution-meaning envelope, canonical codec, and decoders (ADR 0012); the removed machinery has no implementation path,
and its live owners now use typed serde JSON equivalents. This document retains M3/M4 startup-only configuration and the
redaction boundaries above, and no future meaning record may reinterpret an M4 selection.

## Post-M4 tool-loop observability consequence

Future registry/descriptor revisions and safe tool state may be observable, but canonical or public projections may not
expose credentials, raw typed inputs, unsafe absolute/canonical/symlink paths, process resources, provider-native call
identifiers, SDK values, or raw output. See [Tool registry and model-tool
loop](15-tool-registry-and-model-tool-loop.md).

## Post-M4 kernel observability consequence

Future kernel selections, checkpoint metadata, and safe output projections remain credential-free. Checkpoint payloads,
Python values, Jupyter frames, raw tracebacks, grants, endpoints, paths, handles, process details, and implementation
errors never enter storage, protocol, logs, diagnostics, adapters, or model context. Kernel availability is typed
operational evidence, not authority.

## Post-M4 provider-evolution configuration consequence

Architecture 22 owns future startup-only credential-free provider profile/catalog selection. Raw TOML, credentials,
private endpoint input, candidate material, and private clients remain outside persistence and public diagnostics.
Catalog/default state cannot reconstruct immutable provider meaning or imply live reload, credential rotation,
discovery, health testing, or a configuration control plane.

## Post-M4 session branching observability consequence

Future fork snapshots and lineage projections are credential-free safe records. They exclude raw provider/tool/kernel
data, workspace contents, implementation resources, and secrets. Lineage audit remains separate from Session/Run events
and cannot be used to infer rollback or external-effect proof.

## Post-M4 activity and notification observability consequence

Architecture 24 owns safe activity, notification, and acknowledgement projections. They remain credential-free and
exclude raw prompt/provider/tool/MCP/path/grant/resource data. Notifications are durable presentation evidence, not
authority, read-state by cursor, or operational diagnostics.

## Post-M5 instruction-source configuration and observability consequence

[Architecture 30](30-instruction-sources-and-system-context.md) owns the instruction channel ([ADR
0010](../decisions/0010-instruction-sources-and-system-context.md)). Its profile revision identity and workspace
instruction reference cross the configuration surface; instruction text stays on the editing surface where the user
reads and edits it ([architecture 25](25-configuration-provider-control-plane.md)).

Fragment configuration follows the TOML-only typed-edit rules above: an edit is a validated candidate edit that fails
closed and affects fresh runs only, and no credential, private endpoint material, SDK object, or raw provider payload
may be stored in, echoed from, or derived from an instruction source. Logs, activity, notification, and audit surfaces
carry revision identities only; fake-secret regression covers every one of them.
