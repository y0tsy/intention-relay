# ADR 0020: Post-M5 Configuration and Provider Control-Plane Directions

## Status

Accepted 2026-08-30. This decision records the post-M5 configuration and
provider control-plane cluster as accepted future directions. It does not
activate implementation: each direction is bound to
[Milestone 5+](../architecture/11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment)
and is implemented only through a separately accepted activating specification
at the start of that milestone.

## Decision

The following directions are accepted as future product/architecture
directions and are moved out of the "excluded" set of architectures 09 and 22
into the new [Configuration and Provider Control Plane](../architecture/25-configuration-provider-control-plane.md)
package:

- controlled configuration live reload: applying a validated TOML change to a
  running daemon through an explicit contract, transaction, and outcome test,
  affecting fresh runs only;
- credential rotation: replacing private credential material without altering
  frozen per-run meaning or selection;
- provider health-check service: non-authorizing typed readiness evidence;
- provider/model discovery: non-authorizing discovery, never model-name
  routing;
- pricing and budget policy: product policy, never a Mandate admission
  ceiling;
- provider profile UI and configuration control plane: presentation over the
  shared typed client, never adapter authority.

Each direction:

- keeps M3/M4 startup-only configuration authoritative for existing persisted
  runs and snapshots;
- affects fresh runs only after a later activating specification;
- is bound to Milestone 5+ in the roadmap, where all retrospective changes to
  already-implemented code are consolidated;
- requires its own activating specification (crates, DTO/wire/storage
  versions, quality policy, feature profiles, migration declarations, tests,
  and outcome evidence) accepted at the start of the milestone.

## Rationale

The authoritative package review of 2026-08-30 confirmed that architectures
13--24 and decisions 0001--0019 already cover the
[`m4plus_concept.md`](../m4plus_concept.md) research directions, with one
exception: the configuration/provider control-plane cluster (live reload,
rotation, health checks, discovery, pricing, profile UI) was excluded or
deferred across architectures 09 and 22 and the reconciliation register. This
decision adopts that cluster as accepted future directions so the
authoritative documentation no longer states the cluster as permanently
excluded, while preserving the project rule that no feature is documented as
implemented without code evidence.

## Normative invariants

1. M3/M4 startup-only TOML application remains authoritative for existing
   runs; no direction alters a recorded snapshot/revision or rewrites
   history.
2. Any live reload applies through an explicit contract, transaction, and
   outcome test; it is never implied by M3/M4 behavior.
3. Rotation replaces private material only and never changes frozen meaning,
   selection, digest, or canonical bytes.
4. Health checks, discovery, and pricing are non-authorizing: they create no
   RunId, reason, lifecycle transition, scheduler candidate, tool permission,
   child edge, verifier authority, MCP capability, bridge grant, kernel
   epoch, context projection, branch, or reconciliation result.
5. Profile UI and control plane consume only the shared typed client and
   existing projections; they cannot become a second authority or transport.
6. Retrospective changes to implemented code are consolidated in Milestone 5+
   and each is activated by its own specification.

## Failure semantics

- A reload contract that cannot be applied atomically fails closed and leaves
  the running daemon on its recorded snapshot.
- Rotation that would alter frozen meaning rejects before replacement.
- Health, discovery, and pricing failures are typed non-authorizing outcomes;
  they never produce fallback routing or silent selection changes.

## Compatibility and supersession

This decision supersedes the "excluded" wording for the listed cluster in
architectures 09 and 22 and the corresponding rows (EXC-001..004) of the
[deferred/excluded register](../reconciliation/deferred-excluded-register.md).
The closed M4 baseline, M3/M4 bytes, and existing behavior remain unchanged.
Activation remains deferred: no code changes are authorized by this decision.

## Security and residual risk

The directions remain trusted-local. Reload and rotation introduce new
credential-handling surface; every activating specification must keep
credentials credential-free in durable/public surfaces, keep redaction
central, and pass the fake-secret regression suite.

## Affected documents

- [`architecture/09-configuration-security-and-observability.md`](../architecture/09-configuration-security-and-observability.md)
- [`architecture/11-implementation-roadmap.md`](../architecture/11-implementation-roadmap.md)
- [`architecture/22-provider-evolution-profiles-and-reasoning.md`](../architecture/22-provider-evolution-profiles-and-reasoning.md)
- [`architecture/25-configuration-provider-control-plane.md`](../architecture/25-configuration-provider-control-plane.md) (new)
- [`reconciliation/README.md`](../reconciliation/README.md)
- [`reconciliation/source-of-truth-matrix.md`](../reconciliation/source-of-truth-matrix.md)
- [`reconciliation/contradiction-register.md`](../reconciliation/contradiction-register.md)
- [`reconciliation/concept-supersession-index.md`](../reconciliation/concept-supersession-index.md)
- [`reconciliation/deferred-excluded-register.md`](../reconciliation/deferred-excluded-register.md)
- [`reconciliation/evidence-register.md`](../reconciliation/evidence-register.md)
- [`decisions/README.md`](README.md)

## Required evidence

No implementation evidence is claimed. Each direction's activating
specification must declare exact crates, DTO/wire/storage versions, feature
profiles, coverage tiers, fixtures, and outcome evidence, and pass `make
quick`, `make verify`, and Linux/Windows CI before acceptance.

## Non-goals

This decision does not implement reload, rotation, health checks, discovery,
pricing, or profile UI; it does not change M3/M4 behavior; it does not
renumber M5--M9; it does not activate a crate, schema, migration, protocol, or
feature.
