# Configuration and Provider Control Plane

**Activated in Slice 2.** Architecture 25 owns the
configuration and provider control-plane cluster: controlled configuration live reload, credential rotation, provider
health checks, provider/model discovery, pricing and budget policy, and the configuration control-plane surface. The
slice was activated by its accepted activating specification and its delivered contract is recorded in [architecture
11](11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment) and the [Slice 2
evidence](10-test-driven-delivery-and-verification.md#slice-2-evidence). The profile picker/editor presentation and the
instruction-fragment editing surface of [architecture 30](30-instruction-sources-and-system-context.md) remain not
activated.

Owner: architecture 25.

## Ownership and non-authorities

Architecture 09 owns TOML parsing, schema validation, configuration discovery, and redaction.
Architecture 22 owns provider kinds, profiles, catalogs, selections, and driver compatibility; the removed
run-execution meaning layer (its canonical codec was removed under [architecture
02](02-dto-and-contract-policy.md)) leaves no live path. Architecture 15 owns the tool loop, and architecture 03 owns the
flat activity journal, the notification list, and adapter behavior.

This document owns only the delivered directions below: no second runtime, registry, persistence authority, or
sandbox, and no `RunId`, lifecycle transition, tool permission, child, MCP capability, bridge grant, kernel epoch,
context projection, branch, or reconciliation result from a reload, rotation, health observation, discovery result,
price, or control-plane action. All directions apply to fresh runs only.

## Controlled configuration live reload

M3/M4 apply TOML at daemon startup; existing runs retain their recorded immutable configuration revision.
Controlled live reload is delivered and applies a validated TOML change to a running daemon:

- reload is an explicit command, contract, transaction, and outcome test: the
daemon re-parses and validates a candidate configuration revision against the current single configuration shape,
commits the new accepted revision in one transaction, and applies it to fresh runs only; configuration has no migration
path, unversioned or legacy documents fail closed under [architecture
00](00-principles-and-scope.md), and there is no watcher, polling,
auto-restart, or automatic re-application;
- a candidate that cannot be applied atomically fails closed and leaves the
daemon on its recorded configuration revision; a candidate that changes catalog-affecting configuration is rejected with
`catalog_change_requires_restart`, and the next daemon restart re-derives the active catalog from the startup document
through the catalog prepare and accept path;
- existing persisted runs, admitted runs, and recorded configuration revisions are never
mutated, re-selected, or rewritten by a reload;
- the reload command is the only activation path for a running daemon; a catalog-affecting change commits nothing
and takes effect only at the next daemon restart.

## Credential rotation

Credential rotation replaces private credential material without altering frozen meaning:

- rotation replaces only the opaque private material in composition state and
never changes a recorded selection, endpoint, capability subset, or execution meaning;
- a rotation that would change frozen meaning rejects before replacement with
`credential_rotation_frozen_meaning_mismatch`; with no configured credential source it fails closed with
`credential_rotation_source_unavailable`;
- replacement may supply fresh private material after restart when every safe
selected field still matches; it is never in-place mutation of an admitted run, and rotation never resumes, retries,
reattaches, or replays old work;
- credentials remain non-serde, non-`Debug`, and absent from durable/public
surfaces under the architecture 09 redaction law.

The configured private credential source is the daemon's own configuration file. The composition captures the startup
credential inside its private loading boundary at open and retains it in a non-serde, non-`Debug` in-memory slot; a
rotation command re-reads the file through that boundary, replaces the composition's private material only when the
frozen-meaning checks pass, and rebuilds the provider driver's private client. The rebuild keeps the driver options the
composition's provider-option seam applies at construction: rotation preflights the active profile's declared options
through the seam and replaces only the private SDK client, so it never silently drops or ignores declared driver
options. Facades opened without a file-backed source (test-support hosts) fail closed with the same typed error.
Credential handling stays inside this private loading boundary, and no credential, file content, or source path may
appear in a DTO, error, log, projection, or
durable surface.

## Provider health checks

A provider health-check service produces typed operational readiness evidence:

- health results are non-authorizing live evidence, never authority, meaning,
or a fallback selector, and health checks never perform provider/model selection, routing, pricing, discovery, or
credential testing beyond the declared contract;
- unavailability retains the exact reason and creates no `RunId`, retry
counter, or quota;
- the health-evidence DTO carries the provider identity (`provider_id`) and
reports no profile revision: `provider_profile_revision_id` stays
absent, and no synthesized `health-profile-<hex>` identity is fabricated.

## Provider and model discovery

Discovery enumerates provider/model capabilities as typed non-authorizing records:

- discovery results never select a provider kind, endpoint, driver, capability,
profile, or execution kind; model identifiers never route behavior;
- discovery is an independently identified external attempt with its own
before-start/started/terminal evidence and no automatic continuation;
- discovered records are additive and cannot reconstruct, repair, or replace an
immutable selection.

## Pricing and budget policy

Pricing and budget policy is product/budget policy, never an admission ceiling, quota, or entitlement:

- it cannot gate tool admission or capacity outcomes;
- numeric values are classified Intrinsic/Capacity/Product; pricing is never an
admission ceiling.

## Profile UI and configuration control plane

A provider profile UI and configuration control plane is the accepted future presentation direction:

- it consumes only the shared typed client and existing safe projections; it is
not a second authority, transport, registry, or adapter path;
- it renders safe projections and never raw TOML, credentials, private endpoint
material, SDK objects, or live resources;
- profile edits surface through the reload and catalog contracts above and
affect fresh runs only.

## Raw-TOML editing and configuration editing

Raw-TOML editing and the validated configuration-editing surface are delivered by the activated Slice 2:

- a safe, validated raw-TOML editing surface over the shared typed client
produces a new candidate revision through the same reload transaction; it is never adapter authority and never
in-place mutation of an admitted run or recorded configuration revision;
- editing is accepted server-side only: the daemon validates the candidate,
credentials are never echoed through any edit response or projection, and a validated edit that fails closed leaves the
daemon on its recorded configuration revision;
- typed configuration edits reconstruct a credential-free candidate document
server-side and restore the composition's retained private credential into it inside the private loading boundary, so
the candidate validates and commits without the credential ever crossing a wire, DTO, error, log, or durable surface;
- edits affect fresh runs only and never expose credentials, private endpoint
material, SDK objects, or raw provider payloads on durable/public surfaces.

## Instruction-fragment editing and preview

The instruction configuration surface of [architecture 30](30-instruction-sources-and-system-context.md) is edited
through this control plane; architecture 30
owns the fragment operations and the non-admitting preview, and the surface ships with the fifth Milestone 5+ slice and
its activating specification. Validate an edit before it commits and reject an invalid, inconsistent, or over-bound edit
with a typed failure, leaving the running daemon on its recorded profile revision; keep the surface credential-free,
never echo a credential or private material through an edit response, and expose fragment text only where the user edits
it.

## Compatibility and historical preservation

No direction rewrites historical records, assigns new meaning to a closed variant, or reconstructs missing meaning
from current state. All directions affect fresh runs only; the slice is activated and its delivered contract carries
the outcome evidence recorded in the [Slice 2 evidence](10-test-driven-delivery-and-verification.md#slice-2-evidence).

## Dependencies and non-goals

This document depends on architectures 03, 09, and 22. Non-goals: a reload
watcher/transport, keychain or secret store, standalone health-service or discovery topology, pricing engine, profile
picker/editor implementation, OS notifications, remote transport, multi-user access, sandbox/container isolation, and
production activation beyond the accepted directions. The daemon serves reload, rotation, health, discovery, pricing,
and raw-TOML/typed editing through the typed control-plane surface; the profile picker/editor presentation and the
instruction-fragment editing surface remain not activated.

Evidence: recorded [Slice 2 evidence](10-test-driven-delivery-and-verification.md#slice-2-evidence); activating
specification per [architecture 12](12-quality-gates-and-makefile.md).
