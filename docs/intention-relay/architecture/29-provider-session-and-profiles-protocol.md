# Provider Session Selection and Profiles Protocol

## Status and scope

## Traceability

- Normative owner: architecture 29.
- Decision record: [`0024`](../decisions/0024-provider-session-and-profiles-protocol-directions.md).
- Reconciliation topics: `PSS-001..008`.
- Research provenance: [`m4plus_concept.md`](../m4plus_concept.md).
- Status: the M5+ Slice 2 activation ([ADR 0037](../decisions/0037-m5plus-slice2-control-plane.md)) was reverted by [ADR 0044](../decisions/0044-revert-of-m5plus-slice2-control-plane.md); the layer is again an accepted direction awaiting a new activating specification.

**Reverted by ADR 0044; the Slice 2 activation (ADR 0037) is withdrawn.** This
document is the sole detailed owner for the provider session-selection and
profiles protocol layer: session default selection, per-turn and fork
overrides, profile-keyed usage, and safe presentation. The Slice 2 activation
delivered that layer
and was reverted, so no session-selection or catalog-serving surface
exists in the tree and a new activating specification is required to
re-introduce it. The unavailable-queue promotion and reconciliation and the
held-run admission path were removed from the direction by
[ADR 0048](../decisions/0048-limits-by-precedent-and-no-content-scanning.md);
the negotiated capability plane is removed by
[ADR 0045](../decisions/0045-local-json-rpc-2-0-transport.md). The document does
not authorize a `responses` SDK/driver,
user-kind parser, catalog database, profile picker/editor presentation,
credential entry/keychain, health test, discovery, pricing, telemetry, live
reload, or production behavior.

It applies to future fresh runs only. M3/M4 bytes, queue tickets, sessions,
runs, events, snapshots, replay, recovery, and `ToolCallRecorded ->
tool_execution_unavailable` retain their recorded ordinary semantics. The
underlying provider kinds, profiles, catalogs, selections, driver compatibility,
and reasoning semantics are owned by [architecture 22](22-provider-evolution-profiles-and-reasoning.md);
the configuration/provider control plane is owned by
[architecture 25](25-configuration-provider-control-plane.md).

## Ownership and non-authorities

Architecture 13 owns Mandate lifecycle and fresh admission. Architecture 14
owns run-execution meaning and historical compatibility; its canonical codec
was removed by
[ADR 0046](../decisions/0046-typed-serde-json-contracts.md). Architecture 15
owns the registry and tool loop. Architecture 16 owns scheduler readiness
reevaluation. Architecture 22
owns provider kinds, profiles, catalogs, selections, and driver compatibility.
Architecture 23 owns session branching. Architecture 25 owns the configuration/
provider control plane. Architecture 27 owns programmatic-caller policy.

This document owns only the session-selection and presentation layer. A
session default, override, usage aggregate, or protocol frame cannot create a
`RunId`, Mandate reason, lifecycle transition,
scheduler candidate, tool permission, registry slot, child edge, verifier
authority, MCP capability, bridge grant, kernel epoch, context projection,
branch, or reconciliation result. It is not a second runtime, registry,
scheduler, persistence authority, catalog, or sandbox.

## Session selection, runs, and usage

**Reverted by ADR 0044; the Slice 2 activation of ADR 0037 is withdrawn.** The
following records the direction as it was activated and then reverted; none of
these surfaces exists in the tree.

A session copied the global `ProviderProfileId` as a durable future default;
catalog changes never cascade. `SetSessionProviderProfileCommandDto` was
user-initiated, idempotent, and optimistic: it took the session, an enabled
profile ID, the expected session projection revision, and an operation ID;
it changed only future intent and, when the durable default changed, published
the typed `SessionProviderProfileChanged` event to the validating session-event
boundary. The reverted Slice 2 kept no durable copy of that event and wrote no
durable session-event snapshot for it: the control-plane event family had no
durable append seam, so the committed event was validated at the boundary and
recorded nowhere. ADR 0044 removed the event DTO with the rest of the layer, so
no session-default change exists to produce it. Durable delivery is again
declared future work, reserved to Milestone 6 in the roadmap's
[reserved declarations carried by M6-M9](11-implementation-roadmap.md#reserved-declarations-carried-by-m6-m9)
and anchored as the durable
`SessionProviderProfileChanged` append layer. An
existing-profile request was a successful `changed = false` no-op that
published no event.
`GetSessionProviderProfileQueryDto` returned the durable intent, the current
safe resolved entry/revision or a closed unavailability reason, the session
projection revision, and the global default; availability was a daemon-computed
read projection and never mass-rewrote sessions or queues.

`SendUserTurn`, `ForkSession`, and `StartForkRun` accepted an optional safe
profile ID and an optional expected profile revision; a per-turn or fork
override changed only that run; a mismatch rejected before commit; a registry
failure returned `provider_profile_runtime_unavailable`. Every accepted turn
persisted one `ResolvedRunProviderSelectionDto`:

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

One profile per run, no fallback chain. An unavailable exact selection failed
the original `RunId` with `provider_configuration_unavailable`, a closed detail,
and recorded provenance, with no provider call, and never rerouted to a current
default or new revision. The unavailable-queue promotion and reconciliation
machinery (`ReconcileUnavailableQueueCommandDto`, its durable reconciliation
marker, and its per-transition and per-page selection bounds) was removed from
the direction by
[ADR 0048](../decisions/0048-limits-by-precedent-and-no-content-scanning.md)
and is not part of this document. Usage
was keyed by exact profile identity and revision; aggregated by profile into
one bounded entry per `(revision, model)` identity and separately by
revision/model; no price, currency, or estimated cost; different profiles
sharing all safe fields remained independent clients, selection identities, and
usage groups.

The fork override fields existed on the fork DTOs (`ForkSessionCommandDto` and
`StartForkRunCommandDto`) and the resolution service was implemented in the
reverted Slice 2; ADR 0044 removed both. The fork wire commands remain Slice 4
and were never activated by Slice 2.

## Public protocol and presentation

**Reverted by ADR 0044; the Slice 2 activation of ADR 0037 is withdrawn.**
The Slice 2 profiles protocol served paginated catalog reads, catalog status,
session default query/command, safe per-turn and fork overrides,
resolved-selection projections, and pending-removal accept/reject. It was the
additive negotiated capability `provider_profiles_v1`; that capability mechanism
was removed by [ADR 0045](../decisions/0045-local-json-rpc-2-0-transport.md), so
a re-introduced surface uses plain typed methods with no capability or family
gate. It did not imply live reload,
configuration editing, profile testing, credential entry, or model discovery.
Configuration editing was activated for M5+ Slice 2 by
[ADR 0037](../decisions/0037-m5plus-slice2-control-plane.md) through the
atomic reload contract of [architecture 25](25-configuration-provider-control-plane.md);
it was not part of this surface, and ADR 0044 removed both. The daemon no longer
advertises or serves `provider_profiles_v1`, and the held-run admission path was
removed by [ADR 0048](../decisions/0048-limits-by-precedent-and-no-content-scanning.md).

A catalog list was bounded, paginated by an opaque token, sorted by stable
`ProfileId`, carried the active `CatalogRevisionId`, and returned `has_more`; a
catalog change invalidated the token with a typed conflict/resync. An entry
included profile/catalog revisions, display name, enabled state, kind ID, kind
descriptor revision, exact model, normalized endpoint where applicable,
effective policy, capability subset, credential transport mode and safe header
name where applicable, `credential_configured`, deterministic driver-declared
capabilities, and local readiness. The closed readiness projection was `ready`,
`disabled`, or `unavailable` (never claiming network or credential health).
`GetProviderCatalogStatusQueryDto` returned the closed activation state
`preparing`, `active`, `pending_removal`, or `activation_recovery_required`;
the applicable closed degraded reason; active/candidate safe revisions; the
default; and safe validation/removal impact.

Adapters could only read safe state, set session defaults, supply
user-originated overrides, accept/reject pending removal, and admit a held
recovered run; they never wrote TOML, created/edited/enabled/disabled profiles
or kinds, entered credentials, or received configuration paths.

## Startup-only application and degraded recovery

**Reverted by ADR 0044; the Slice 2 activation of ADR 0037 is withdrawn.**
Profiles v1 were startup-only; there was no watcher, polling, auto-restart, or
restart protocol command. Startup auto-accepted additions, new user-kind
additions, execution edits, enable/disable, and display changes; an existing
user kind never accepted an edited composition under an old ID. A
semantic-equal catalog wrote no new revision but reconstructed the private
registry; credential-only changes were invisible durable state (a deferred
credential-rotation limitation). An omitted profile or unreferenced kind
became a process-local pending-removal candidate (not auto-tombstoned); a
profile pointing to an omitted kind was invalid. Degraded mode was admin/read
only.

`AcceptProviderCatalogRemovalCommandDto` (idempotent) took a candidate handle,
expected active/candidate revisions, and an operation ID; it atomically
accepted removals, created tombstones, recorded ordered audit, and activated
the registry. `RejectProviderCatalogCandidateCommandDto` dropped the
private candidate and pending status, recorded
`ProviderCatalogCandidateRejected`, and left degraded read-only with
`removal_candidate_rejected`. At most one candidate existed; its lifetime was
**30 minutes** from `ProviderCatalogRemovalPending`; expiry produced
`ProviderCatalogCandidateExpired` and degraded read-only with
`removal_candidate_expired`.

In the reverted activation, startup opened storage first, interrupted unfinished
runs before any read response, then prepared and activated the catalog. A
degraded daemon served health, safe catalog status/validation, and
session/run/tree reads; all provider state changes, admission, and
default changes were rejected with `execution_not_ready`; the only exceptions
were accept/reject of the one pending candidate. A crash after acceptance
produced `ProviderCatalogActivationRecoveryRequired` before reconstruction and
`ProviderCatalogRecoveryCompleted` only after the exact accepted catalog was
active; a mismatch stayed `activation_recovery_required`. The closed degraded
reasons were `removal_candidate_pending`, `removal_candidate_rejected`,
`removal_candidate_expired`, and `activation_recovery_required`. ADR 0044
removed this startup path, the readiness gate, and every catalog table.

The reverted Slice 2 also held a recovery-promoted `Starting` run for explicit
`AdmitRecoveredRunCommandDto` admission; ADR 0048 removed that held-run
admission path from the direction, and the ordinary recovery path is again the
only one.

## Compatibility and historical preservation

- M3/M4 bytes, queue tickets, sessions, runs, events, snapshots, replay, and
  recovery remain authoritative and unchanged; no historical selection gains a
  synthetic profile/catalog/session-default state.
- A per-turn or fork override affected only its run; existing persisted runs
  retain their recorded immutable selection.
- The directions affect fresh runs only; they were activated under Milestone 5+
  and then reverted by ADR 0044.

## Dependencies and non-goals

This document depends on architectures 13, 14, 15, 16, 22, 23, 25, and 27 plus
decisions 0001, 0003, 0008, 0014, 0015, 0020, 0022, and 0024. It does not
define a Responses SDK/driver, user-kind parser, profile picker/editor
presentation, credential entry/keychain, telemetry, multimodal or structured
output, plugin drivers, or remote continuation; the catalog database, the
single current storage schema (logical version 1),
credential rotation, health checks, discovery, pricing, controlled
live reload, and typed header policy were activated by Slice 2
([ADR 0037](../decisions/0037-m5plus-slice2-control-plane.md)) and were removed
by [ADR 0044](../decisions/0044-revert-of-m5plus-slice2-control-plane.md); the
typed server-side-parser and preservation-control contracts were removed as
unconsumed by the unconsumed-surface audit (2026-09), so no parser-configuration
surface is activated. UI, Cargo,
Makefile/CI, or production activation beyond the accepted directions
remain outside this document.

A later activating specification must declare exact crates, dependencies, test
targets, coverage declarations under [ADR 0049](../decisions/0049-base-coverage-threshold.md), feature profiles, storage/wire schema, retention, and
bounds, then pass `make quick`, `make docs-check`, `make architecture`,
`make verify`, and Linux/Windows CI.
[ADR 0037](../decisions/0037-m5plus-slice2-control-plane.md) was the Slice 2
activating specification; it is superseded by
[ADR 0044](../decisions/0044-revert-of-m5plus-slice2-control-plane.md) and its
declared test targets no longer exist. The per-direction evidence obligations
return to the accepted direction and a re-introduction must restore them:

- session default/override command and query fixtures with idempotency, a
  `changed = false` no-op that publishes no event, and one validated boundary
  `SessionProviderProfileChanged` publication per committed change (no durable
  copy in Slice 2);
- per-turn/fork override binding and mismatch-rejection fixtures;
- profile-keyed usage aggregation and no-double-count fixtures;
- profiles-method pagination, catalog-status, and readiness-projection
  fixtures;
- pending-removal accept/reject/expiry and degraded-mode fixtures;
- M3/M4 byte/meaning/replay/recovery preservation and fake-secret regression
  across logs, errors, snapshots, events, and adapter DTOs.
