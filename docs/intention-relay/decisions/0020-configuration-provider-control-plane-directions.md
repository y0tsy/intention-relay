# 0020: Post-M5 Configuration and Provider Control-Plane Directions

## Status

Accepted 2026-08-30. Not implemented; activation requires an activating specification.

## Decision

The configuration and provider control-plane cluster is adopted as an accepted future direction owned by [architecture
25](../architecture/25-configuration-provider-control-plane.md):

- controlled configuration live reload: a validated TOML change applied to a
running daemon through an explicit contract, transaction, and outcome test, affecting fresh runs only;
- credential rotation: replacement of private credential material without
altering frozen per-run meaning or selection;
- provider health-check service: non-authorizing typed readiness evidence;
- provider/model discovery: non-authorizing discovery, never model-name
routing;
- pricing and budget policy: product policy, never a Mandate admission
ceiling;
- provider profile UI and configuration control plane: presentation over the
shared typed client, never adapter authority.

Each direction keeps M3/M4 startup-only configuration authoritative for existing persisted runs and snapshots, affects
fresh runs only after its own activating specification (crates, DTO/wire/storage versions, quality policy, feature
profiles, migration declarations, tests, and outcome evidence), and remains bound to Milestone 5+ in the roadmap.

## Normative invariants

1. M3/M4 startup-only TOML application remains authoritative for existing
runs; no direction alters a recorded snapshot or revision or rewrites history.
2. Live reload applies through an explicit contract, transaction, and outcome
test; it is never implied by M3/M4 behavior.
3. Rotation replaces private material only and never changes frozen meaning,
selection, digest, or canonical bytes.
4. Health checks, discovery, and pricing are non-authorizing: they create no
`RunId`, reason, lifecycle transition, scheduler candidate, tool permission, child edge, verifier authority, MCP
capability, bridge grant, kernel epoch, context projection, branch, or reconciliation result.
5. Profile UI and control plane consume only the shared typed client and
existing projections; they are never a second authority or transport.

## Failure semantics

- A reload contract that cannot be applied atomically fails closed and leaves
the running daemon on its recorded snapshot.
- Rotation that would alter frozen meaning rejects before replacement.
- Health, discovery, and pricing failures are typed non-authorizing outcomes;
they never produce fallback routing or silent selection changes.

## Rationale

Architectures 13-24 and decisions 0001-0019 already cover the `m4plus_concept.md` research directions except this
cluster, which architectures 09 and 22 had excluded or deferred. Adopting the cluster as an accepted direction removes
the permanent-exclusion statement without documenting any feature as implemented.

## Compatibility and non-goals

This decision supersedes the "excluded" wording for the cluster in architectures 09 and 22 and the corresponding
EXC-001..004 rows of the retired deferred/excluded register. The closed M4 baseline, M3/M4 bytes, and existing behavior
remain unchanged.

No direction is implemented here: no reload, rotation, health check, discovery, pricing, or profile UI, and no crate,
schema, migration, protocol, or feature activation. M5-M9 are not renumbered.

Owner: architecture 25. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).
