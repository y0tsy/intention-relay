# Provider Session Selection and Profiles Protocol

**Approved future design. Not implemented; activation requires an activating specification.** Architecture 29 owns the
provider session-selection and profiles protocol layer: session default selection, per-turn and fork overrides,
profile-keyed usage, and safe presentation. The M5+ Slice 2 activation of this layer was reverted; re-introduction
requires a new activating specification (activation sequence: [ADR
0004](../decisions/0004-m5plus-complete-foundation-activation.md), [ADR
0010](../decisions/0010-instruction-sources-and-system-context.md)). The unavailable-queue promotion and reconciliation
and the held-run admission path were removed from the direction by [ADR
0014](../decisions/0014-limits-by-precedent-and-no-content-scanning.md), and the negotiated capability plane is removed
by [ADR 0011](../decisions/0011-local-json-rpc-2-0-transport.md).

Owner: architecture 29. Decisions: ADR 0011, ADR 0014. Research: `m4plus_concept.md`.

## Ownership and non-authorities

Architecture 14 owns run-execution meaning and historical
compatibility; its canonical codec was removed by [ADR 0012](../decisions/0012-typed-serde-json-contracts.md).
Architecture 15 owns the registry and tool loop, and architecture 23 session branching.
Architecture 22 owns provider kinds, profiles, catalogs, selections, and driver compatibility;
architecture 25 owns the configuration/provider control plane.

This document owns only the session-selection and presentation layer. A session default, override, usage aggregate, or
protocol frame is not a second runtime, registry, persistence authority, catalog, or sandbox, and creates no `RunId`,
lifecycle transition, tool permission, registry slot, child, MCP capability, bridge grant, kernel epoch, context
projection, branch, or reconciliation result.

## Session selection, runs, and usage

A session copies the global `ProviderProfileId` as a durable future default; catalog changes never cascade.
`SetSessionProviderProfileCommandDto` is user-initiated, idempotent, and optimistic: it takes the session, an enabled
profile ID, the expected session projection revision, and an operation ID; it changes only future intent and, when the
durable default changes, publishes the typed `SessionProviderProfileChanged` event to the validating session-event
boundary. No durable copy of that event and no durable session-event snapshot is written: the control-plane event family
has no durable append seam. Durable delivery remains future work, reserved to Milestone 6 in the roadmap's [reserved
declarations carried by M6-M9](11-implementation-roadmap.md#reserved-declarations-carried-by-m6-m9) and anchored as the
durable `SessionProviderProfileChanged` append layer. An existing-profile request is a successful `changed = false`
no-op that publishes no event. `GetSessionProviderProfileQueryDto` returns the durable intent, the current safe resolved
entry/revision or a closed unavailability reason, the session projection revision, and the global default; availability
is a daemon-computed read projection and never mass-rewrites sessions or turns.

`SendUserTurn`, `ForkSession`, and `StartForkRun` accept an optional safe profile ID and an optional expected profile
revision; a per-turn or fork override changes only that run, a mismatch is rejected before commit, and a registry
failure returns `provider_profile_runtime_unavailable`. Every accepted turn persists one
`ResolvedRunProviderSelectionDto`:

```text
ResolvedRunProviderSelectionDto
  selection_contract_revision
  profile_id
  provider_profile_revision_id
  kind_id
  kind_descriptor_revision_id
  model_id
  normalized_effective_endpoint
  credential_transport_mode
  credential_transport_safe_header_name
  declared_model_capability_subset
  resolved_reasoning_policy
  effective_execution_policy
  effective_loopback_policy_or_not_applicable
  provider_driver_contract_revision
  selection_source                 # immutable provenance, not part of the selection contract
```

One profile per run, no fallback chain. An unavailable exact selection fails the original `RunId` with
`provider_configuration_unavailable`, a closed detail, and recorded provenance, with no provider call, and is never
rerouted to a current default or new revision. Usage is keyed by exact profile identity and revision, aggregated by
profile into one bounded entry per `(revision, model)` identity and separately by revision/model, with no price,
currency, or estimated cost; different profiles sharing all safe fields remain independent clients, selection
identities, and usage groups. The fork override fields on `ForkSessionCommandDto` and `StartForkRunCommandDto` and their
resolution service were part of the reverted layer; the fork wire commands remain Slice 4 and were never activated by
Slice 2.

## Public protocol and presentation

The reverted profiles protocol served paginated catalog reads, catalog status, session default query/command, safe
per-turn and fork overrides, resolved-selection projections, and pending-removal accept/reject. It was the additive
negotiated capability `provider_profiles_v1`; that capability mechanism was removed by [ADR
0011](../decisions/0011-local-json-rpc-2-0-transport.md), so a re-introduced surface uses plain typed methods with no
capability or family gate. It did not imply live reload, configuration editing, profile testing, credential entry, or
model discovery. Configuration editing belonged to the [architecture 25](25-configuration-provider-control-plane.md)
atomic reload contract and was not part of this surface. The daemon advertises and serves no `provider_profiles_v1`
capability today.

A catalog list is bounded, paginated by an opaque token, sorted by stable `ProfileId`, carries the active
`CatalogRevisionId`, and returns `has_more`; a catalog change invalidates the token with a typed conflict/resync. An
entry includes profile/catalog revisions, display name, enabled state, kind ID, kind descriptor revision, exact model,
normalized endpoint where applicable, effective policy, capability subset, credential transport mode and safe header
name where applicable, `credential_configured`, deterministic driver-declared capabilities, and local readiness. The
closed readiness projection is `ready`, `disabled`, or `unavailable` (never claiming network or credential health).
`GetProviderCatalogStatusQueryDto` returns the closed activation state `preparing`, `active`, `pending_removal`, or
`activation_recovery_required`; the applicable closed degraded reason; active/candidate safe revisions; the default; and
safe validation/removal impact.

Adapters can only read safe state, set session defaults, supply user-originated overrides, and accept/reject pending
removal; they never write TOML, create/edit/enable/disable profiles or kinds, enter credentials, or receive
configuration paths.

## Startup-only application and degraded recovery

Profiles v1 are startup-only: no watcher, polling, auto-restart, or restart protocol command exists. Startup
auto-accepts additions, new user-kind additions, execution edits, enable/disable, and display changes; an existing user
kind never accepts an edited composition under an old ID. A semantic-equal catalog writes no new revision but
reconstructs the private registry, and credential-only changes are invisible durable state (a deferred
credential-rotation limitation). An omitted profile or unreferenced kind becomes a process-local pending-removal
candidate (not auto-tombstoned); a profile pointing to an omitted kind is invalid. Degraded mode is admin/read only.

`AcceptProviderCatalogRemovalCommandDto` (idempotent) takes a candidate handle, expected active/candidate revisions, and
an operation ID; it atomically accepts removals, creates tombstones, records ordered audit, and activates the registry.
`RejectProviderCatalogCandidateCommandDto` drops the private candidate and pending status, records
`ProviderCatalogCandidateRejected`, and leaves degraded read-only with `removal_candidate_rejected`. At most one
candidate exists, with a lifetime of **30 minutes** from `ProviderCatalogRemovalPending`; expiry produces
`ProviderCatalogCandidateExpired` and degraded read-only with `removal_candidate_expired`.

Startup opens storage first, interrupts unfinished runs before any read response, then prepares and activates the
catalog. A degraded daemon serves health, safe catalog status/validation, and session/run/tree reads; all provider state
changes, admission, and default changes are rejected with `execution_not_ready`, the only exceptions being accept/reject
of the one pending candidate. A crash after acceptance produces `ProviderCatalogActivationRecoveryRequired` before
reconstruction and `ProviderCatalogRecoveryCompleted` only after the exact accepted catalog is active; a mismatch stays
`activation_recovery_required`. The closed degraded reasons are `removal_candidate_pending`,
`removal_candidate_rejected`, `removal_candidate_expired`, and `activation_recovery_required`. The reverted layer also
held a recovery-promoted `Starting` run for explicit `AdmitRecoveredRunCommandDto` admission; [ADR
0014](../decisions/0014-limits-by-precedent-and-no-content-scanning.md) removed that held-run admission path, and the
ordinary recovery path is again the only one.

## Compatibility and historical preservation

M3/M4 bytes, sessions, runs, events, snapshots, replay, recovery, and `ToolCallRecorded ->
tool_execution_unavailable` remain authoritative and unchanged, and no historical selection gains a synthetic
profile/catalog/session-default state. A per-turn or fork override affects only its run; existing persisted runs retain
their recorded immutable selection. All directions affect fresh runs only; the Slice 2 activation was reverted and
re-introduction requires a new activating specification.

## Dependencies and non-goals

This document depends on architectures 14, 15, 22, 23, and 25.
Non-goals: a `responses` SDK/driver, user-kind parser, catalog database, profile picker/editor
presentation, credential entry/keychain, health test, discovery, pricing, telemetry, live reload, multimodal or
structured output, plugin drivers, remote continuation, and production behavior. The catalog database, the single
current storage schema (logical version 1), credential rotation, health checks, discovery, pricing, controlled live
reload, and typed header policy belonged to the reverted Slice 2; the typed server-side-parser and preservation-control
contracts were removed as unconsumed by the unconsumed-surface audit (2026-09), so no parser-configuration surface is
activated.

Evidence: activating specification per [architecture 12](12-quality-gates-and-makefile.md).
