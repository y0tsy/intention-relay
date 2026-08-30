# Provider Session Selection and Profiles Protocol

## Status and scope

## Traceability

- Normative owner: architecture 29.
- Decision record: [`0024`](../decisions/0024-provider-session-and-profiles-protocol-directions.md).
- Reconciliation topics: `PSS-001..008`.
- Research provenance: [`m4plus_concept2.md`](../m4plus_concept2.md).
- Status: documentation-approved; implementation-authorized work requires a later activating specification under [Milestone 5+](11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment).

**Approved future architecture, documentation-only.** This document is the
sole detailed owner for the future provider session-selection and profiles
protocol layer: session default selection, per-turn and fork overrides,
unavailable-queue promotion and reconciliation, profile-keyed usage,
`provider_profiles_v1` public protocol and presentation, and held
recovery-promoted run admission. It does not authorize a crate, implementation,
protocol, storage migration, configuration, network connection, local process,
or delivery scope.

It applies to future fresh runs only. M3/M4 bytes, queue tickets, sessions,
runs, events, snapshots, replay, recovery, and `ToolCallRecorded ->
tool_execution_unavailable` retain their recorded ordinary semantics. The
underlying provider kinds, profiles, catalogs, selections, driver compatibility,
and reasoning semantics are owned by [architecture 22](22-provider-evolution-profiles-and-reasoning.md);
the configuration/provider control plane is owned by
[architecture 25](25-configuration-provider-control-plane.md).

## Ownership and non-authorities

Architecture 13 owns Mandate lifecycle and fresh admission. Architecture 14
owns canonical framing and digests. Architecture 15 owns the registry and tool
loop. Architecture 16 owns scheduler readiness reevaluation. Architecture 22
owns provider kinds, profiles, catalogs, selections, and driver compatibility.
Architecture 23 owns session branching. Architecture 25 owns the configuration/
provider control plane. Architecture 27 owns programmatic-caller policy.

This document owns only the session-selection and presentation layer. A
session default, override, promotion, reconciliation, usage aggregate, or
protocol frame cannot create a `RunId`, Mandate reason, lifecycle transition,
scheduler candidate, tool permission, registry slot, child edge, verifier
authority, MCP capability, bridge grant, kernel epoch, context projection,
branch, or reconciliation result. It is not a second runtime, registry,
scheduler, persistence authority, catalog, or sandbox.

## Session selection, runs, queues, and usage

A session copies the global `ProviderProfileId` as a durable future default;
catalog changes never cascade. `SetSessionProviderProfileCommandDto` is
user-initiated, idempotent, and optimistic: it takes the session, an enabled
profile ID, the expected session projection revision, and an operation ID;
it changes only future intent and emits `SessionProviderProfileChanged`; an
existing-profile request is a successful `changed = false` no-op.
`GetSessionProviderProfileQueryDto` returns the durable intent, the current safe
resolved entry/revision or a closed unavailability reason, the session
projection revision, and the global default; availability is a daemon-computed
read projection and never mass-rewrites sessions or queues.

`SendUserTurn`, `ForkSession`, and `StartForkRun` accept an optional safe profile
ID and an optional expected profile revision; a per-turn or fork override
changes only that run; a mismatch rejects before commit; a registry failure
returns `provider_profile_runtime_unavailable`. Every accepted turn persists
one `ResolvedRunProviderSelectionDto`:

```text
ResolvedRunProviderSelectionDto
  selection_canonicalization_version
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
  selection_source                 # immutable provenance, outside execution digest
```

One profile per run, no fallback chain. Promotion of an unavailable exact
selection: the original `RunId`, `Starting -> Failed` with
`provider_configuration_unavailable`, a closed detail, and promotion
provenance, with no provider call. Unavailable promotion FIFO proceeds only
through **8** unavailable selections per terminal transition; exhaustion writes
a typed queue-reconciliation-needed marker. `ReconcileUnavailableQueueCommandDto`
(user-only, idempotent) handles at most **32** currently unavailable immutable
selections per page; it terminalizes only those, may promote the first
available item, and never reroutes to a current default or new revision. Usage
is keyed by exact profile identity and revision; aggregated by profile and
separately by revision/model; no price, currency, or estimated cost; different
profiles sharing all safe fields remain independent clients, selection
identities, and usage groups.

## Public protocol and presentation

`provider_profiles_v1` is one additive negotiated capability for paginated
catalog reads, catalog status, session default query/command, safe per-turn and
fork overrides, resolved-selection projections, pending-removal accept/reject,
and explicit admission of a held recovered run. It does not imply live reload,
configuration editing, profile testing, credential entry, or model discovery.
Configuration editing is an accepted post-M5 future direction under
[ADR 0033](../decisions/0033-accepted-m5plus-execution-directions.md), to be
executed in Milestone 5+ through the atomic reload contract of
[architecture 25](25-configuration-provider-control-plane.md); it is not
activated here.

A catalog list is bounded, paginated by an opaque token, sorted by stable
`ProfileId`, carries the active `CatalogRevisionId`, and returns `has_more`; a
catalog change invalidates the token with a typed conflict/resync. An entry
includes profile/catalog revisions, display name, enabled state, kind ID, kind
descriptor revision, exact model, normalized endpoint where applicable,
effective policy, capability subset, credential transport mode and safe header
name where applicable, `credential_configured`, deterministic driver-declared
capabilities, and local readiness. The closed readiness projection is `ready`,
`disabled`, or `unavailable` (never claiming network or credential health).
`GetProviderCatalogStatusQueryDto` returns the closed activation state
`preparing`, `active`, `pending_removal`, or `activation_recovery_required`;
the applicable closed degraded reason; active/candidate safe revisions; the
default; safe validation/removal impact; and the negotiated capability state.

Adapters may only read safe state, set session defaults, supply user-originated
overrides, accept/reject pending removal, and admit a held recovered run; they
never write TOML, create/edit/enable/disable profiles or kinds, enter
credentials, or receive configuration paths.

## Startup-only application and degraded recovery

Profiles v1 are startup-only; there is no watcher, polling, auto-restart, or
restart protocol command. Startup auto-accepts additions, new user-kind
additions, execution edits, enable/disable, and display changes; an existing
user kind never accepts an edited composition under an old ID. A
semantic-equal catalog writes no new revision but reconstructs the private
registry; credential-only changes are invisible durable state (a deferred
credential-rotation limitation). An omitted profile or unreferenced kind
becomes a process-local pending-removal candidate (not auto-tombstoned); a
profile pointing to an omitted kind is invalid. Degraded mode is admin/read
only.

`AcceptProviderCatalogRemovalCommandDto` (idempotent) takes a candidate handle,
expected active/candidate revisions, an operation ID, and a source recheck;
it atomically accepts removals, creates tombstones, records ordered audit, and
activates the registry. `RejectProviderCatalogCandidateCommandDto` drops the
private candidate and pending status, records `ProviderCatalogCandidateRejected`,
and leaves degraded read-only with `removal_candidate_rejected`. At most one
candidate exists; its lifetime is **30 minutes** from
`ProviderCatalogRemovalPending`; expiry produces `ProviderCatalogCandidateExpired`
and degraded read-only with `removal_candidate_expired`.

Startup opens storage first, interrupts unfinished runs before any read
response, then prepares and activates the catalog. A degraded daemon serves
health, safe catalog status/validation, and session/run/tree reads; all
provider state changes, admission, promotion, and default changes are rejected
with `execution_not_ready`; the only exceptions are accept/reject of the one
pending candidate. A crash after acceptance produces
`ProviderCatalogActivationRecoveryRequired` before reconstruction and
`ProviderCatalogRecoveryCompleted` only after the exact accepted catalog is
active; a mismatch stays `activation_recovery_required`. The closed degraded
reasons are `removal_candidate_pending`, `removal_candidate_rejected`,
`removal_candidate_expired`, and `activation_recovery_required`.

A recovery-promoted `Starting` run is never auto-scheduled; it is held as a
durable active run. The user issues the idempotent `AdmitRecoveredRunCommandDto`
(exact session/run identities and an operation ID), which verifies the complete
immutable selection, the enabled exact registry entry, driver compatibility,
and active catalog readiness before scheduling; a repeat cannot schedule a
second task; failed verification returns a closed safe error and the run stays
held; the ordinary stop path terminalizes
`Starting -> Cancelling -> Cancelled` without provider/tool/external work.

## Compatibility and historical preservation

- M3/M4 bytes, queue tickets, sessions, runs, events, snapshots, replay, and
  recovery remain authoritative and unchanged; no historical selection gains a
  synthetic profile/catalog/session-default state.
- A per-turn or fork override affects only that run; existing persisted runs
  retain their recorded immutable selection.
- All directions affect fresh runs only, activated under Milestone 5+.

## Dependencies and non-goals

This document depends on architectures 13, 14, 15, 16, 22, 23, 25, and 27 plus
decisions 0001, 0003, 0008, 0014, 0015, 0020, 0022, and 0024. It does not
define a Responses SDK/driver, user-kind parser, catalog database, wire tags,
migrations, profile picker/editor, credential entry/keychain/rotation, health
test, discovery, pricing, telemetry, live reload, multimodal or structured
output, arbitrary headers, plugin drivers, remote continuation, provider-side
parser administration, UI, Cargo, Makefile/CI, or production activation.

A later activating specification must declare exact crates, dependencies, test
targets, coverage tiers, feature profiles, storage/wire schema, retention, and
bounds, then pass `make quick`, `make docs-check`, `make architecture`,
`make verify`, and Linux/Windows CI. Required evidence includes:

- session default/override command and query fixtures with idempotency and
  `changed = false` no-op;
- per-turn/fork override binding and mismatch-rejection fixtures;
- unavailable-queue promotion (8 per transition), reconciliation (32 per page),
  and no-reroute fixtures;
- profile-keyed usage aggregation and no-double-count fixtures;
- `provider_profiles_v1` pagination, catalog-status, and readiness-projection
  fixtures;
- pending-removal accept/reject/expiry and degraded-mode fixtures;
- held recovered-run admission (`AdmitRecoveredRunCommandDto`) idempotency and
  no-auto-schedule fixtures;
- M3/M4 byte/meaning/replay/recovery preservation and fake-secret regression
  across logs, errors, snapshots, events, and adapter DTOs.
