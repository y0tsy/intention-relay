# Provider Session Selection

**Approved future design. Not implemented; activation requires an activating specification.** Architecture 29 owns the
provider session-selection layer: the selected entry name per session, the bounded provider list, and the resolved
selection projection. The layer is not activated; activation requires an activating specification (activation sequence:
[architecture 11](11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment)). The negotiated capability
plane is removed under [architecture 03](03-daemon-transport-and-adapters.md).

Owner: architecture 29.

## Ownership and non-authorities

Architecture 22 owns provider entries, selection resolution, endpoints, credentials, and reasoning;
architecture 25 owns provider/model discovery and typed configuration editing; architecture 09 owns TOML parsing,
validation, and credential isolation; architecture 15 owns tool identity and the tool loop, and architecture 23 session
branching.

This document owns only the session-selection layer. A session selection, provider list, or protocol frame is not a
second runtime, registry, persistence authority, catalog, or sandbox, and creates no `RunId`, lifecycle transition,
tool selection, child, MCP connection, kernel epoch, context projection, branch, or reconciliation result.

## Session selection

A session stores at most one selected provider entry name; with no selection it follows the global default of
[architecture 22](22-provider-evolution-profiles-and-reasoning.md). The selection is plain durable intent:
`SetSessionProviderCommandDto { session_id, provider_name }` changes it for future runs only, last write wins, and it
carries no expected revision, operation ID, event, or `changed = false` no-op semantics. `GetSessionProviderQueryDto`
returns the selected name, the global default, and the availability of the selected entry (`available | missing`). A
selection that names an entry absent from the startup document fails the run with
`provider_configuration_unavailable`, a closed detail, and recorded provenance, with no provider call; it is never
mass-rewritten, and a session whose selection is cleared follows the global default again.

Every accepted run freezes exactly one resolved entry's safe fields into its immutable configuration snapshot
([architecture 09](09-configuration-security-and-observability.md)): entry name, kind, model, normalized endpoint, the
safe credential-transport mode and header name, and the reasoning-effort level when present. There is no profile
revision, no resolved-selection record with more fields than that snapshot, no per-turn or fork override, and no usage
aggregation keyed by selection: usage stays on the run row. `SendUserTurn`, `ForkSession`, and `StartForkRun` carry no
provider fields.

## Protocol and presentation

The layer serves a bounded provider list (name, kind, model, normalized endpoint, `credential_configured`,
availability) with no pagination, opaque token, or catalog revision; the session selection command and query; and the
resolved selection projection. It is not gated on any capability mechanism: the surface uses plain typed requests with
no capability or family gate, and the operations extend the single typed wire of [architecture
03](03-daemon-transport-and-adapters.md) when Slice 4 activates.

Adapters can only read safe state and set the session selection; they never write TOML, create or edit entries, enter
credentials, or receive configuration paths. The daemon advertises no provider capability today.

## Startup-only application

Provider entries are startup-only and activation stays daemon-startup-only ([architecture
22](22-provider-evolution-profiles-and-reasoning.md), [architecture
09](09-configuration-security-and-observability.md)); the layer adds no watcher, polling, restart protocol, or live
reload. The layer does not include the held-run admission path (`AdmitRecoveredRunCommandDto`); the ordinary recovery
path is the only one.

## Compatibility and historical preservation

No historical selection gains a synthetic entry or session-default state. All directions affect fresh runs only; the
layer is not activated and activation requires an activating specification.

## Dependencies and non-goals

This document depends on architectures 09, 15, 22, and 23. Non-goals: a `responses` SDK/driver, provider picker/editor
presentation, credential entry/keychain, discovery, telemetry, multimodal or structured output, plugin adapters, remote
continuation, and production behavior. Controlled live reload, credential rotation, health checks, pricing, and
raw-TOML editing were removed from Slice 4 ([architecture 25](25-configuration-provider-control-plane.md)).

Evidence: activating specification per [architecture 12](12-quality-gates-and-makefile.md).
