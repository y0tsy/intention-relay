# Configuration and Provider Control Plane

## Status and scope

## Traceability

- Normative owner: architecture 25.
- Decision record: [`0020`](../decisions/0020-configuration-provider-control-plane-directions.md).
- Detail decision: [`0033`](../decisions/0033-accepted-m5plus-execution-directions.md) (raw-TOML editing and configuration editing).
- Reconciliation topics: `CFG-001..010`.
- Research provenance: `m4plus_concept.md`.
- Status: the M5+ Slice 2 activation ([ADR 0037](../decisions/0037-m5plus-slice2-control-plane.md)) was reverted by [ADR 0044](../decisions/0044-revert-of-m5plus-slice2-control-plane.md); the cluster is again an accepted direction awaiting a new activating specification.

**Reverted by ADR 0044; the Slice 2 activation (ADR 0037) is withdrawn.** This
document is the sole detailed owner for the configuration and provider
control-plane cluster: controlled configuration live reload, credential
rotation, provider health checks, provider/model discovery, pricing and budget
policy, and the configuration control-plane surface. The Slice 2 activation
delivered reload, rotation, health checks, discovery, pricing, and
raw-TOML/configuration editing as specified below; that delivery was reverted,
so none of those surfaces exists in the tree and a new activating specification
is required to re-introduce them. The document does not authorize a reload
watcher, keychain or secret store, health-service topology, discovery client,
pricing engine, profile picker/editor presentation, or production
configuration behavior.

The authoritative package review of 2026-08-30 confirmed that the
`m4plus_concept.md` research directions are otherwise
covered by architectures 13--24 and decisions 0001--0019; this package closes
the only identified gap by adopting this cluster as accepted future
directions. It applies to future fresh runs only. M3/M4 startup-only TOML
application, `ConfigSnapshotDto` revisions, persisted run snapshots, provider
kinds, retries, model facts, cursors, replay, and recovery retain their
recorded ordinary semantics.

## Ownership and non-authorities

Architecture 09 owns TOML parsing, schema validation, configuration discovery,
redaction, and startup-only application. Architecture 22 owns future provider
kinds, profiles, catalogs, selections, and driver compatibility. Architecture
14 owns run-execution meaning and historical compatibility; its canonical codec
was removed by
[ADR 0046](../decisions/0046-typed-serde-json-contracts.md). Architecture 13 owns
Mandate lifecycle and fresh admission. Architecture 15 owns the tool loop.
Architecture 24 owns activity/UI projections and adapter behavior.

This document owns only the accepted future control-plane directions listed
below. A reload, rotation, health observation, discovery result, price, or
control-plane action cannot create a Mandate reason, `RunId`, lifecycle
transition, scheduler candidate, tool permission, child edge, verifier
authority, MCP capability, bridge grant, kernel epoch, context projection,
branch, or reconciliation result. It is not a second runtime, registry,
scheduler, persistence authority, or sandbox.

## Controlled configuration live reload

**Reverted by ADR 0044; the Slice 2 activation of ADR 0037 is withdrawn.**

M3/M4 apply TOML only at daemon startup; existing runs retain their recorded
immutable snapshot/revision. Controlled live reload was the activated direction
(reverted by ADR 0044) that applied a validated TOML change to a running
daemon:

- reload is an explicit command, contract, transaction, and outcome test: the
  daemon re-parses and validates a candidate snapshot against the current
  single configuration shape, atomically commits a new accepted revision, and
  applies it to fresh runs only; configuration has no migration path and
  unversioned or legacy documents fail closed under ADR 0038; there is no
  watcher, polling, or auto-restart, and no automatic re-application;
- an edit that cannot be applied atomically fails closed and leaves the
  running daemon on its recorded snapshot;
- a reload candidate that changed catalog-affecting configuration was rejected
  with `catalog_change_requires_restart` in the reverted Slice 2 activation;
  the recorded recovery was that the next daemon restart re-derived the active
  catalog from the startup document through the catalog prepare and accept
  path, so restarting applied the change;
- existing persisted runs, admitted runs, and recorded snapshots are never
  mutated, re-selected, or rewritten by a reload;
- the reload command was the only activation path for a running daemon; with
  the revert, daemon restart is again the only configuration activation path.

## Credential rotation

**Reverted by ADR 0044; the Slice 2 activation of ADR 0037 is withdrawn.**

Credential rotation was the activated direction (reverted by ADR 0044) that
replaced private credential material without altering frozen meaning:

- rotation replaces only the opaque private material in composition state and
  never changes a recorded selection, endpoint,
  capability subset, or execution meaning;
- a rotation that would change frozen meaning rejects before replacement with
  `credential_rotation_frozen_meaning_mismatch`;
- rotation fails closed with `credential_rotation_source_unavailable` when no
  configured credential source exists;
- replacement may supply fresh private material after restart when every safe
  selected field still matches; it is never in-place mutation of an admitted
  run;
- rotation never resumes, retries, reattaches, or replays old work;
- credentials remain non-serde, non-`Debug`, and absent from durable/public
  surfaces under the architecture 09 redaction law.

In the reverted Slice 2 activation the daemon's own configuration file was the
configured private credential source. The composition captured the startup
credential inside its private loading boundary at open and retained it in a
non-serde, non-`Debug` in-memory slot; a rotation command re-read the file
through the same private loading boundary, replaced the composition's private
material only when the frozen-meaning checks passed, and rebuilt the executing
provider driver's private client. The rebuild kept the driver options that the
composition's provider-option seam applied at construction (PR24-057): rotation
replaced only the private SDK client, and it preflighted the active profile's
declared options through the seam, so rotation could never silently drop or
ignore declared driver options. Facades opened without a file-backed source
(test-support hosts) had no configured source and kept the fail-closed
`credential_rotation_source_unavailable` behavior. ADR 0044 removed this
composition path; no credential handling beyond the M3/M4 startup boundary
exists now, and no credential, file content, or source path may appear in a
DTO, error, log, snapshot, projection, or durable surface.

## Provider health checks

**Reverted by ADR 0044; the Slice 2 activation of ADR 0037 is withdrawn.**

A provider health-check service was the activated direction (reverted by
ADR 0044) that produced typed operational readiness evidence:

- health results are non-authorizing live evidence, never authority, meaning,
  or a fallback selector;
- unavailability retains the exact reason and creates no `RunId`,
  retry counter, or quota; restoration only permits
  architecture-16 reevaluation;
- health checks never perform provider/model selection, routing, pricing,
  discovery, or credential testing beyond the declared contract;
- the removed health-evidence DTO carried the provider identity (`provider_id`)
  and reported no profile revision while the catalog was not wired into the
  health path: `provider_profile_revision_id` stayed absent, and no synthesized
  `health-profile-<hex>` identity was fabricated; ADR 0044 removed the DTO with
  the rest of the Slice 2 surface.

## Provider and model discovery

**Reverted by ADR 0044; the Slice 2 activation of ADR 0037 is withdrawn.**

Discovery was the activated direction (reverted by ADR 0044) for enumerating
provider/model capabilities as typed non-authorizing records:

- discovery results never select a provider kind, endpoint, driver,
  capability, profile, or execution kind; model identifiers never route
  behavior;
- discovery is an independently identified external attempt with its own
  before-start/started/terminal evidence and no automatic continuation;
- discovered records are additive and cannot reconstruct, repair, or replace
  an immutable selection.

## Pricing and budget policy

**Reverted by ADR 0044; the Slice 2 activation of ADR 0037 is withdrawn.**

Pricing and budget policy was the activated product direction (reverted by
ADR 0044):

- it is product/budget policy, never a Mandate admission ceiling, quota, or
  entitlement;
- it cannot gate direct Mandate admission, tool admission, scheduler
  eligibility, or capacity outcomes;
- numeric values were classified Intrinsic/Capacity/Product as recorded by the
  superseded Slice 2 activating specification; pricing is never an admission
  ceiling.

## Profile UI and configuration control plane

A provider profile UI and configuration control plane is the accepted future
presentation direction:

- it consumes only the shared typed client and existing safe projections; it
  cannot become a second authority, transport, registry, or adapter path;
- it renders safe projections and never raw TOML, credentials, private
  endpoint material, SDK objects, or live resources;
- profile edits surface through the reload and catalog contracts above and
  affect fresh runs only.

## Raw-TOML editing and configuration editing

**Reverted by ADR 0044; the Slice 2 activation of ADR 0037 is withdrawn.**

Raw-TOML editing and a validated configuration-editing surface are accepted
directions under [ADR 0033](../decisions/0033-accepted-m5plus-execution-directions.md).
They were executed in M5+ Slice 2 and reverted by ADR 0044; they are again
documentation-only directions awaiting a new activating specification:

- a safe, validated raw-TOML editing surface over the shared typed client
  produces a new candidate snapshot through the same atomic reload contract;
  it is never adapter authority and never in-place mutation of an admitted
  run or recorded snapshot;
- raw-TOML editing is accepted server-side only: the daemon validates the
  candidate and credentials are never echoed through any edit response or
  projection;
- configuration editing surfaces validated edits that fail closed when they
  cannot be applied atomically, leaving the running daemon on its recorded
  snapshot;
- typed configuration edits reconstruct a credential-free candidate document
  server-side and restore the composition's retained private credential into
  it inside the private loading boundary, so the candidate validates and
  commits without the credential ever crossing a wire, DTO, error, log, or
  durable surface;
- edits affect fresh runs only and never expose credentials, private endpoint
  material, SDK objects, or raw provider payloads on durable/public surfaces.

## Instruction-fragment editing and preview

**Accepted direction for the fifth Milestone 5+ slice by [ADR 0043](../decisions/0043-instruction-sources-and-system-context.md).**

The instruction configuration surface of
[architecture 30](30-instruction-sources-and-system-context.md) is edited through
this control plane:

- list, create, edit, duplicate, enable, disable, reorder, and re-scope
  instruction fragments under the typed-edit and atomic-reload rules above;
- validate an edit before it commits and reject an invalid, inconsistent, or
  over-bound edit with a typed failure, leaving the running daemon on its
  recorded profile revision;
- show the profile revision identity, and preview the
  effective instruction projection for a chosen session, policy, and mode
  without admitting a run, creating a reason or selection, or writing durable
  instruction state;
- keep the surface credential-free, never echo a credential or private material
  through an edit response, and expose fragment text only where the user edits
  it;
- edit configuration only: an admitted run's projection is immutable, and a
  later edit affects fresh runs only.

The direction activates nothing by itself: the fragment editing and preview
surface ships with the fifth Milestone 5+ slice and its activating
specification.

## Compatibility and historical preservation

- M3/M4 startup-only application, recorded revisions, and persisted run
  snapshots remain authoritative and unchanged.
- No direction rewrites historical bytes, assigns new meaning to a closed
  variant, or reconstructs missing meaning from current state.
- M3/M4 provider kinds, retries, model facts, cursors, snapshots, replay,
  recovery, and `ToolCallRecorded -> tool_execution_unavailable` retain their
  recorded ordinary semantics.
- All directions affect fresh runs only; they were activated under Milestone 5+
  and then reverted by [ADR 0044](../decisions/0044-revert-of-m5plus-slice2-control-plane.md),
  so they await a new activating specification.

## Dependencies and non-goals

This document depends on architectures 09, 14, 16, 22, and 24 plus decisions
0003, 0014, and 0020. It does not define a reload watcher/transport, keychain
or secret store, standalone health-service topology, standalone discovery
client, pricing engine, profile picker/editor implementation, OS
notifications, remote transport, multi-user access, sandbox/container
isolation, or production activation beyond the accepted directions.
In the reverted Slice 2 activation the reload, rotation, health, discovery,
pricing, and raw-TOML/typed editing surface was served through the daemon
facade with typed commands and queries; ADR 0044 removed that serving surface
and no replacement exists now.

A later activating specification must declare exact crates, dependencies, test
targets, coverage declarations under [ADR 0049](../decisions/0049-base-coverage-threshold.md), feature profiles, storage/wire schema, retention, and
bounds, then pass `make quick`, `make docs-check`, `make architecture`,
`make verify`, and Linux/Windows CI.
[ADR 0037](../decisions/0037-m5plus-slice2-control-plane.md) was the Slice 2
activating specification; it is superseded by
[ADR 0044](../decisions/0044-revert-of-m5plus-slice2-control-plane.md) and its
declared test targets no longer exist. The per-direction evidence obligations
return to the accepted direction and a re-introduction must restore them:

- reload transaction fault injection: atomic commit or fail-closed, no
  partial snapshot, no mutation of existing runs;
- rotation redaction and no-frozen-meaning-change fixtures;
- health/discovery non-authority fixtures: no RunId/reason/selection created,
  no model-name routing, no fallback;
- pricing non-ceiling classification fixtures;
- control-plane safe-projection fixtures: no raw TOML/credentials/resources
  cross public or durable boundaries;
- M3/M4 byte/meaning/replay/recovery preservation and fake-secret regression
  across logs, errors, snapshots, events, and adapter DTOs.
