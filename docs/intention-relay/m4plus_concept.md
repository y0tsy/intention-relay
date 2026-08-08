# M4+ Concept: Provider Profiles and Per-Run Selection

## Status

**Research concept, not an approved implementation scope.** This document
preserves an investigation performed during M4 work and retained after M4
closure. It does not amend the closed M4 charter or implementation baseline,
authorize code changes, alter accepted M4 behavior, or claim that provider
profiles, configuration reload, credential rotation, or a provider-selection
user interface are delivered.

"M4+" is a shorthand for this unapproved research direction, not an approved
milestone or project phase. This document records possible future constraints;
it does not add a roadmap entry, crate, quality-policy target, or implementation
authorization.

The closed M4 record remains [`m4.md`](m4.md), with its immutable
implementation baseline and acceptance evidence recorded in [M4 Closure
Evidence](closeout/m4-closure-evidence.md). The closed M4 baseline accepts only
`openrouter` and `generic-chat-completion-api`, applies TOML at daemon startup,
records one immutable credential-free selection per run, and explicitly
excludes configuration live reload, credential rotation, and M6 UI work.

## Prime Agent research and long-term direction

The following preserved research informs the wider M4+ direction:

- [Prime Agent runtime reference](../reference/prime-agent-research/prime-agent-runtime-reference.md);
- [RLM, IPython, and continual-harness integration analysis](../reference/prime-agent-research/rlm-ipython-harness-integration-analysis.md).

In the longer term, Intention Relay should implement the overall capability
described by that research: RLM-style recursive orchestration, persistent
IPython control-plane support, durable child-agent operation, and a continual
harness. Any future approved work in this direction must establish the
architectural and durable-runtime foundations first, rather than claim to
deliver the complete capability.

Before that scope is approved for implementation, the architecture
documentation, milestone roadmap, crate map, quality policy, and related
decision records require comprehensive replanning around the new direction.
This concept records only that intent; it does not amend M4 or authorize those
broader documentation or implementation changes.

## Conceptual execution direction: IPython and a unified Rust-owned capability plane

The long-term IPython direction is an optional programmable orchestration
control plane, not a second daemon, persistence system, or independent tool
implementation. The Rust daemon remains the sole authority for session and run
identity, provider selection, tool policy, child-agent admission, durable
state, event publication, and recovery. A persistent Python namespace may hold
variables, helper functions, skill wrappers, and child-agent handles, but it is
recoverable convenience state rather than authoritative application progress.

Future IPython code should call a thin Python facade over a typed daemon host
bridge. A later provider-native direct model tool may expose an equivalent
capability, but both paths must adapt to the same Rust-owned invocation service
and registered base tool. They must not independently perform equivalent file,
process, network, or other agent actions.

```mermaid
flowchart LR
  PY[IPython facade] --> GW[Tool gateway]
  MT[Model tool] --> GW
  GW --> PL[Policy hooks]
  PL --> BT[Base Rust tool]
  BT --> DS[Durable state]
  DS --> MV[Safe model view]
```

<!-- IPython and direct provider tool calls share one Rust-owned capability path. -->

The shared future invocation path must:

1. bind the request to daemon-held active session/run context rather than trust
   caller-supplied identity;
2. validate a typed tool request and apply WorkspaceRoot, mode, risk,
   confirmation, and hook policy;
3. execute the registered Rust base tool;
4. durably commit the tool outcome and policy evidence before publication; and
5. return a safe, size-bounded model-visible projection, preserving VFR,
   Headroom, retrieval, redaction, and observability policy.

A future direct-tool descriptor is a deliberate model-facing projection of a
Rust tool contract, not automatic exposure of every internal tool. Provider
function schemas, Python facade calls, daemon protocol commands, durable tool
events, and model-visible result projections must derive from compatible typed
Rust DTO contracts rather than manually maintained divergent schemas.

An unrestricted IPython kernel executes with its operating-system permissions.
It can bypass the facade through `pathlib`, `os`, or `subprocess`, so it is not
a technical enforcement boundary for WorkspaceRoot, Plan mode, hooks, or
audit. The facade is therefore the intended architecture and ergonomics path,
not a sandbox by itself. Enforcing those boundaries for Python requires a later
OS-level restricted sidecar or sandbox design where filesystem, process, and
network access are available only through the daemon bridge. Current trusted
local v1 assumptions neither provide nor claim such isolation, and this concept
does not decide its packaging, permission model, or delivery scope.

A full `model -> tool -> model` loop, including typed tool-result messages, is
a prerequisite for either IPython-driven or direct-tool agent execution. M4's
closed `tool_execution_unavailable` behavior remains unchanged. Cancellation,
kernel failure, and daemon restart must not silently resume external work;
existing no-resume recovery semantics continue to apply.

## Research question

The investigated product direction is:

- retain more than one configured provider target;
- allow several Generic Chat Completions endpoints with distinct models and
  credentials;
- use different targets in different sessions;
- allow a session default and an explicit future-turn override.

The direction is technically feasible but is a cross-cutting configuration,
persistence, composition, runtime, protocol, and presentation feature. It is
not a small adjustment to the current single-provider configuration.

## Closed M4 baseline

The closed M4 baseline has one daemon-startup provider selection:

```toml
[provider]
kind = "openrouter" # or "generic-chat-completion-api"
model = "..."
endpoint = "..." # required for generic Chat Completions
credential = "..."
```

`ConfigSnapshotDto` retains a credential-free representation of that one
selection. A started or queued turn retains its immutable snapshot and
`ConfigRevisionId`; queue promotion must keep the original selection. Runtime
compares the persisted and currently available safe selection before a provider
call. A mismatch fails safely with `provider_configuration_unavailable` and
makes no outbound request.

This is a useful foundation for profiles because immutable per-run selection,
queue promotion, recovery-before-ready, and provider-neutral runtime contracts
already exist. It is not sufficient for a catalog because a safe selection has
no logical profile identity. Two targets with otherwise equal kind, model,
endpoint, and execution policy may still represent different intended
credentials, accounts, or routing policy.

## Recommended future design

### Provider profiles

A configured provider profile is a stable non-secret identifier plus a
revisioned credential-free safe selection. Profile identity must never be
derived from a model name, endpoint, or credential.

```mermaid
flowchart LR
  C[Profile catalog] --> D[Session default]
  D --> T[Turn override]
  T --> R[Resolved run profile]
  R --> Q[Queued turn snapshot]
  Q --> X[Selected private driver]

```

<!-- A profile is resolved at durable turn acceptance, not at queue promotion. -->

The future DTO family should include the equivalent of:

```text
ProviderProfileId
ProviderProfileRevisionId
ProviderProfileRevisionDto
  profile_id
  revision_id
  display_name
  protocol
  model_id
  endpoint
  execution_policy
  credential_configured

ResolvedRunProviderSelectionDto
  profile_id
  profile_revision_id
  protocol
  model_id
  endpoint
  execution_policy
```

All of these values are credential-free. A profile identifier should be a
validated stable slug or typed ID; it must reject whitespace, path syntax,
ambiguous names, and arbitrary endpoint/credential input from a client.

### Configuration shape

A later schema may hold a catalog of profiles rather than a single provider:

```toml
schema_version = 2

[defaults]
profile = "openrouter-main"

[profiles.openrouter-main]
kind = "openrouter"
model = "anthropic/claude-sonnet-4"
credential = "..."

[profiles.local-vllm]
kind = "generic-chat-completion-api"
endpoint = "https://vllm.example.invalid/v1"
model = "llama-3.3-70b"
credential = "..."

[profiles.company-proxy]
kind = "generic-chat-completion-api"
endpoint = "https://proxy.example.invalid/v1"
model = "deployment-a"
credential = "..."
```

The literal TOML credentials remain private configuration input. A safe
catalog DTO may contain profile ID, protocol kind, model, endpoint, effective
execution policy, and `credential_configured`, but never secret text,
authorization headers, raw TOML, or configuration paths.

### Configuration change awareness and controlled reload

A later approved scope should provide configuration change awareness and a
controlled configuration reload workflow. A local file watcher should report
that the configuration file changed, but it must not apply the changed file
automatically.

The user must be able to inspect the detected change, discard it by restoring
the prior configuration, or explicitly accept the changed configuration and
request a reload. A successful reload applies only to future work; it must not
silently change active runs, queued turns, their immutable selections, or
retry behavior.

This direction requires future architecture, lifecycle, protocol, persistence,
security, and presentation decisions. It does not decide the watcher,
comparison, confirmation, reload, rollback, error, or credential-rotation
mechanisms, and it does not amend M4's closed startup-only configuration
behavior.

### Adapter configuration control plane

A later approved scope should give adapters and their shared client a safe
configuration control plane, so users do not need to edit TOML directly for
ordinary configuration work. The daemon remains the sole authority for reading,
validating, writing, retaining, and reloading configuration; TOML remains the
persistent authoritative source.

Adapters should be able to obtain safe runtime configuration status, inspect a
safe semantic preview of a detected or proposed change, request validation,
explicitly request reload, and edit non-secret settings through daemon-owned
workflows. The control plane should let users understand which configuration is
active, whether a change awaits review, and whether a requested change can
affect future work, without exposing raw TOML, configuration paths, credentials,
authorization headers, or provider SDK resources.

This direction includes future support for user review, explicit acceptance or
rejection, and safe non-secret editing. It does not decide public DTOs,
protocol commands, client APIs, presentation layouts, TOML-writing fidelity,
change-preview format, concurrency behavior, or audit semantics.

Credential entry, secret storage, and persistent restoration of prior
secret-bearing TOML are separate future security decisions. Rejecting a changed
candidate must at minimum mean that it is not applied to the active runtime;
it does not by itself decide how a prior persistent configuration is restored.

### Selection lifecycle

1. A session has a durable default profile for **future** accepted turns.
2. A user turn may inherit that default or supply a safe explicit profile ID.
3. The daemon resolves the profile at acceptance and persists the full safe
   resolved profile revision in the turn/run snapshot.
4. A queued turn retains that exact resolved selection. Changing a session
   default or profile later cannot rewrite queued or active runs.
5. On execution, the daemon-private registry looks up the persisted profile
   revision and verifies exact safe compatibility before provider work.
6. A missing, disabled, deleted, incompatible, or credential-unavailable
   profile fails `Starting -> Failed` with
   `provider_configuration_unavailable`, with no provider call.
7. Restart recovery remains unchanged: pre-existing unfinished external work
   becomes `Interrupted` and is never resumed automatically.

Persisting only `ProviderProfileId` is insufficient. Profile edits or deletion
would make queued work drift. The immutable safe profile revision must remain
attached to the accepted turn/run.

### Private driver registry

Composition, and only composition, should construct one private driver per
startup-loaded profile and retain a registry conceptually equivalent to:

```text
ProviderProfileId -> private driver entry
  OpenRouter(OpenRouterDriver)
  GenericChat(GenericChatDriver)
```

Each Generic Chat profile creates its own private SDK client from its endpoint
and credential. This supports multiple compatible endpoints safely, including
profiles with equal model IDs but different endpoints. The registry must not
cross a DTO, persistence, transport, protocol, runtime public API, or adapter
boundary. Credentials and provider SDK types remain private implementation
resources.

A future daemon host must prove its concrete driver resources meet its chosen
async executor's ownership requirements. It must not use a mutable daemon-wide
"current provider", because different sessions may execute against different
profiles concurrently.

## Session and run semantics

The existing one-active-run invariant is per session, not per daemon. Different
sessions can therefore use distinct profile selections concurrently once the
daemon host supports concurrent model work.

A future session selection command affects only later turn acceptance. It must
not mutate:

- an active run;
- a queued turn's already persisted selection;
- a promoted run's original `RunId` or configuration revision; or
- a retry's selected driver/profile.

Retries stay within the same immutable profile revision. There is no fallback
selection, model-name heuristic, or re-routing to another provider after an
error. Cancellation remains scoped by `(SessionId, RunId)` and remains
independent of profile selection.

## Public workflow needed later

A complete product feature requires more than a configuration parser. Likely
public contracts include:

1. a query that lists safe configured selectable profiles;
2. a query for a session's effective future-turn default;
3. a command that chooses a profile for future turns in a session; and
4. an optional typed profile override on a future `SendUserTurn` command.

The same future public surface must also support safe configuration status,
change review, validation, controlled reload, and non-secret configuration
editing without making an adapter a raw TOML or credential transport channel.

These contracts require versioned protocol capability negotiation, client
methods, durable session projection/event support, and M6 presentation work.
They must never accept raw credentials, arbitrary endpoints, or SDK-specific
configuration from an adapter.

## Required verification portfolio

Any approved implementation of this concept must add evidence for:

- profile catalog TOML validation, identifier uniqueness, endpoint validation,
  credential redaction, and single-provider legacy migration;
- exact profile revision persistence per accepted and queued turn;
- queue promotion preserving the original profile revision;
- missing/ambiguous/incompatible profile behavior after restart with no
  provider call;
- private registry isolation across multiple Generic Chat endpoints;
- retry and cancellation remaining bound to one selected profile;
- concurrent sessions selecting different profiles without shared mutable
  selection state;
- protocol/client compatibility and safe presentation projections for profile
  list/default/override workflows;
- safe configuration status, change preview, validation, explicit
  accept/reject/reload, and non-secret editing workflows;
- equivalent WorkspaceRoot, mode, confirmation, hook, audit, and durable
  publication outcomes when the same future capability is reached through a
  Python facade or a direct model tool;
- typed host-request validation, daemon-bound run identity, cancellation,
  bounded model-visible output, and safe error/redaction behavior for a future
  Python bridge;
- proof that an unrestricted kernel is not represented as an enforcement
  boundary, plus separate evidence for any future restricted-sidecar claim;
- compatible direct-tool schema/DTO contracts with no provider SDK, Python
  object, raw Jupyter frame, credential, or implementation resource escaping a
  public boundary; and
- redaction and failure coverage proving that adapters, events, snapshots,
  logs, diagnostics, and previews do not disclose credential-bearing source
  material.

The root `make quick` and `make verify` contracts remain mandatory. Any new
production crate or integration target must be registered in the
machine-readable architecture and coverage policy before its production code
is accepted.

## Recommendation and sequencing

Do not reopen closed M4 with this catalog, session-default, UI, and reload work.

Recommended sequence:

1. Use the closed M4 single-startup-selection baseline as the compatibility and
   safety foundation for a dedicated follow-on specification for startup-loaded
   provider profiles among already supported provider protocols. It may include
   multiple Generic Chat endpoints, profile IDs, session defaults, per-turn
   overrides, configuration change awareness, and controlled user-approved
   reload, while continuing to defer credential rotation.
2. Add profile-selection presentation UX through the M6 adapter scope after the
   daemon/protocol contract exists.

Any later approved plan that adds IPython or direct model tools must first
establish the shared typed tool/policy and agent-loop contracts. It may then
design a Python sidecar/host bridge and direct-tool adapters over that one
capability plane. Persistent kernels, recursive child agents, continual
harness behavior, sandboxing, crates, milestones, roadmap changes, and
quality-policy entries remain separate future replanning decisions.

This sequencing preserves durable run immutability, queue correctness,
credential isolation, no-resume recovery, and provider SDK boundaries while
avoiding a broad redesign of the closed M4 baseline.
