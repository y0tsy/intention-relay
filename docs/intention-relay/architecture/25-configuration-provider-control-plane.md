# Configuration and Provider Control Plane

## Status and scope

## Traceability

- Normative owner: architecture 25.
- Decision record: [`0020`](../decisions/0020-configuration-provider-control-plane-directions.md).
- Detail decision: [`0033`](../decisions/0033-accepted-m5plus-execution-directions.md) (raw-TOML editing and configuration editing).
- Reconciliation topics: `CFG-001..010`.
- Research provenance: [`m4plus_concept.md`](../m4plus_concept.md).
- Status: activated for M5+ Slice 2 by [ADR 0037](../decisions/0037-m5plus-slice2-control-plane.md) (controlled live reload, credential rotation, health checks, discovery, pricing, raw-TOML/configuration editing).

**Activated for M5+ Slice 2 by ADR 0037.** This document is the sole detailed
owner for the configuration and provider control-plane cluster: controlled
configuration live reload, credential rotation, provider health checks,
provider/model discovery, pricing and budget policy, and the configuration
control-plane surface. Slice 2 activates reload, rotation, health checks,
discovery, pricing, and raw-TOML/configuration editing as specified below. It
does not authorize a reload watcher, keychain or secret store, health-service
topology, discovery client, pricing engine, profile picker/editor
presentation, or production configuration behavior beyond the activated Slice 2
contracts.

The authoritative package review of 2026-08-30 confirmed that the
[`m4plus_concept.md`](../m4plus_concept.md) research directions are otherwise
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
14 owns canonical framing, digests, and decode classes. Architecture 13 owns
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

**Activated for M5+ Slice 2 by ADR 0037.**

M3/M4 apply TOML only at daemon startup; existing runs retain their recorded
immutable snapshot/revision. Controlled live reload is the activated direction
that applies a validated TOML change to a running daemon:

- reload is an explicit command, contract, transaction, and outcome test: the
  daemon re-parses, migrates, and validates a candidate snapshot, atomically
  commits a new accepted revision, and applies it to fresh runs only; there is
  no watcher, polling, or auto-restart, and no automatic re-application;
- an edit that cannot be applied atomically fails closed and leaves the
  running daemon on its recorded snapshot;
- a reload candidate that changes catalog-affecting configuration is rejected
  with `catalog_change_requires_restart` in Slice 2;
- existing persisted runs, admitted runs, and recorded snapshots are never
  mutated, re-selected, or rewritten by a reload;
- the reload command is the only activation path for a running daemon.

## Credential rotation

**Activated for M5+ Slice 2 by ADR 0037.**

Credential rotation is the activated direction that replaces private
credential material without altering frozen meaning:

- rotation replaces only the opaque private material in composition state and
  never changes a recorded selection, digest, canonical bytes, endpoint,
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

## Provider health checks

**Activated for M5+ Slice 2 by ADR 0037.**

A provider health-check service is the activated direction that produces typed
operational readiness evidence:

- health results are non-authorizing live evidence, never authority, meaning,
  or a fallback selector;
- unavailability retains the exact reason and creates no `RunId`,
  reservation, retry counter, or quota; restoration only permits
  architecture-16 reevaluation;
- health checks never perform provider/model selection, routing, pricing,
  discovery, or credential testing beyond the declared contract.

## Provider and model discovery

**Activated for M5+ Slice 2 by ADR 0037.**

Discovery is the activated direction for enumerating provider/model
capabilities as typed non-authorizing records:

- discovery results never select a provider kind, endpoint, driver,
  capability, profile, or execution kind; model identifiers never route
  behavior;
- discovery is an independently identified external attempt with its own
  before-start/started/terminal evidence and no automatic continuation;
- discovered records are additive and cannot reconstruct, repair, or replace
  an immutable selection.

## Pricing and budget policy

**Activated for M5+ Slice 2 by ADR 0037.**

Pricing and budget policy is the activated product direction:

- it is product/budget policy, never a Mandate admission ceiling, quota,
  reservation, or entitlement;
- it cannot gate direct Mandate admission, tool admission, scheduler
  eligibility, or capacity outcomes;
- numeric values are classified Intrinsic/Capacity/Product as recorded by the
  Slice 2 activating specification; pricing is never an admission ceiling.

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

**Activated for M5+ Slice 2 by ADR 0037.**

Raw-TOML editing and a validated configuration-editing surface are activated
directions under [ADR 0033](../decisions/0033-accepted-m5plus-execution-directions.md),
executed in M5+ Slice 2:

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
- edits affect fresh runs only and never expose credentials, private endpoint
  material, SDK objects, or raw provider payloads on durable/public surfaces.

## Compatibility and historical preservation

- M3/M4 startup-only application, recorded revisions, and persisted run
  snapshots remain authoritative and unchanged.
- No direction rewrites historical bytes, assigns new meaning to a closed
  variant, or reconstructs missing meaning from current state.
- M3/M4 provider kinds, retries, model facts, cursors, snapshots, replay,
  recovery, and `ToolCallRecorded -> tool_execution_unavailable` retain their
  recorded ordinary semantics.
- All directions affect fresh runs only, activated under Milestone 5+.

## Dependencies and non-goals

This document depends on architectures 09, 14, 16, 22, and 24 plus decisions
0003, 0014, and 0020. It does not define a reload watcher/transport, keychain
or secret store, standalone health-service topology, standalone discovery
client, pricing engine, profile picker/editor implementation, OS
notifications, remote transport, multi-user access, sandbox/container
isolation, or production activation beyond the activated Slice 2 contracts.
The reload, rotation, health, discovery, pricing, and raw-TOML/typed editing
surface is served through the daemon facade with typed commands and queries.

A later activating specification must declare exact crates, dependencies, test
targets, coverage tiers, feature profiles, storage/wire schema, retention, and
bounds, then pass `make quick`, `make docs-check`, `make architecture`,
`make verify`, and Linux/Windows CI.
[ADR 0037](../decisions/0037-m5plus-slice2-control-plane.md) is the Slice 2
activating specification: it declares the exact test targets, the single
current-schema storage policy, and the per-direction evidence anchors. Required
evidence includes:

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
