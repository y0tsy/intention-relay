# Provider Discovery and Typed Configuration Editing

**Approved future design. Not implemented; activation requires an activating specification.** Architecture 25 owns the
provider/model discovery direction and the typed configuration-editing direction. The slice is not activated; activation
requires an activating specification (activation sequence: [architecture
11](11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment), [architecture
30](30-instruction-sources-and-system-context.md)).

Owner: architecture 25.

## Ownership and non-authorities

Architecture 09 owns TOML parsing, schema validation, configuration discovery, redaction, and startup-only application.
Architecture 22 owns future provider kinds, profiles, catalogs, selections, and driver compatibility. Architecture 15
owns the tool loop, and architecture 03 owns the flat activity journal, the notification list, and adapter behavior.

This document owns only the accepted future directions below: no second runtime, registry, persistence authority, or
sandbox, and no `RunId`, lifecycle transition, tool permission, child, kernel epoch,
context projection, branch, or reconciliation result from a discovery result or a configuration edit. All directions
apply to future fresh runs only, and configuration activation remains daemon-startup-only.

## Directions removed by the owner decision (2026-10-13)

Controlled configuration live reload, in-process credential rotation, provider health checks, pricing and budget policy,
and raw-TOML configuration editing are removed from the accepted directions:

- no command, contract, transaction, DTO, facade, or test obligation survives for any of them, and none of them may be
reintroduced by an activating specification without a new owner decision;
- configuration activation stays daemon-startup-only: the typed editing direction below produces a document that the
next daemon start applies, and a running daemon has no second activation path;
- credential material reaches the daemon only through the startup loading boundary of [architecture
09](09-configuration-security-and-observability.md): there is no rotation command and no in-process replacement of
private material;
- there is no health-check service, no pricing or budget surface, and no raw-TOML editing surface; typed configuration
editing replaces the last of them.

## Provider and model discovery

Discovery enumerates provider/model capabilities as typed non-authorizing records:

- discovery results never select a provider kind, endpoint, driver, capability,
profile, or execution kind; model identifiers never route behavior;
- discovery is an independently identified external attempt with its own
before-start/started/terminal evidence and no automatic continuation;
- discovered records are additive and cannot reconstruct, repair, or replace an
immutable selection.

## Typed configuration editing

A validated typed configuration-editing surface is an accepted direction awaiting a new activating specification:

- a safe, validated typed editing surface over the shared typed client produces a new candidate configuration document;
a raw-TOML editing surface does not exist, and every configuration edit is expressed through the typed surface;
- editing is accepted server-side only: the daemon validates the candidate
against the current single configuration shape, credentials are never echoed through any edit response or projection, and
a validated edit that fails closed leaves the daemon on its recorded configuration revision;
- typed configuration edits reconstruct a credential-free candidate document
server-side and restore the composition's retained private credential into it inside the private loading boundary, so
the candidate validates and commits without the credential ever crossing a wire, DTO, error, log, or durable surface;
- the committed document applies at the next daemon start: a running daemon never adopts an edit, a restart is required,
and the user may restart the daemon at any time; the precise restart experience remains the open decision of
[architecture 09](09-configuration-security-and-observability.md);
- edits affect fresh runs only: they never mutate a persisted run or a recorded configuration revision, and they never
expose credentials, private endpoint material, SDK objects, or raw provider payloads on durable/public surfaces.

## Presentation and protocol surface

The provider profile UI and the typed configuration-editing surface's presentation and protocol surface are owned by
[architecture 29](29-provider-session-and-profiles-protocol.md) (public protocol and presentation).

## Compatibility and historical preservation

No direction rewrites historical records, assigns new meaning to a closed variant, or reconstructs missing meaning
from current state. All directions affect fresh runs only; the slice is not activated and activation requires an
activating specification.

## Dependencies and non-goals

This document depends on architectures 03, 09, and 22. Non-goals: a reload
watcher or transport, keychain or secret store, standalone discovery topology, profile
picker/editor implementation, instruction-fragment editing or preview (instruction configuration is filesystem-level,
[architecture 30](30-instruction-sources-and-system-context.md)), OS notifications, remote transport, multi-user access,
sandbox/container isolation, and production activation beyond the accepted directions. The daemon-facade serving
surface for discovery and typed configuration editing does not exist in the tree; the removed directions have no
serving surface either and are not a deferred dependency of any milestone.

Evidence: activating specification per [architecture 12](12-quality-gates-and-makefile.md).
