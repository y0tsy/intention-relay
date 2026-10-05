# 0024: Post-M5 Provider Session Selection and Profiles Protocol Directions

## Status

Accepted 2026-08-30. Not implemented; activation requires an activating specification.

## Decision

The provider session-selection and profiles protocol layer from `m4plus_concept.md` is adopted as an accepted future
direction owned by [architecture 29](../architecture/29-provider-session-and-profiles-protocol.md):

- session default selection (`SetSessionProviderProfileCommandDto`,
`GetSessionProviderProfileQueryDto`);
- per-turn and fork overrides on `SendUserTurn`/`ForkSession`/`StartForkRun`,
persisting one `ResolvedRunProviderSelectionDto` per run;
- profile-keyed usage aggregation with no double counting;
- the profiles protocol surface: paginated catalog reads, catalog status,
session default query/command, safe overrides, resolved-selection projections, pending-removal accept/reject, and
readiness projection. The former negotiated `provider_profiles_v1` capability is removed by [ADR
0045](0045-local-json-rpc-2-0-transport.md); a re-introduced surface uses plain typed methods with no capability gate;
- startup-only application with pending-removal (30-minute lifetime) and
degraded read-only recovery.

The reverted Slice 2 activation also carried unavailable-queue promotion (8 per terminal transition) and reconciliation
(`ReconcileUnavailableQueueCommandDto`, 32 per page) plus held recovery-promoted run admission
(`AdmitRecoveredRunCommandDto`); those surfaces were reverted and removed from the direction by [ADR
0048](0048-limits-by-precedent-and-no-content-scanning.md), and no M3 queue, ticket, or promotion remains: a user
message accepted during an active run is a pending turn that joins that run's live context ([ADR
0055](0055-pending-turns-and-cooperative-interruption.md)).

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after its own activating specification, and
remains bound to Milestone 5+.

## Normative invariants

1. A session copies the global `ProviderProfileId` as a durable future
default; catalog changes never cascade.
2. A per-turn or fork override affects only that run; one profile per run, no
fallback chain.
3. Usage is keyed by exact profile identity and revision and is never
double-counted; no price, currency, or inferred cost.
4. Adapters never write TOML, edit profiles or kinds, enter credentials, or
receive configuration paths.
5. Profiles are startup-only; recovery never auto-schedules a run.
6. M3/M4 bytes, sessions, runs, events, snapshots, replay, and
recovery remain authoritative and unchanged.

## Failure semantics

- Registry failure returns `provider_profile_runtime_unavailable`; an
unavailable immutable selection terminalizes `Starting -> Failed` with `provider_configuration_unavailable` and a closed
detail, with no provider call.
- A degraded daemon rejects all provider state changes, admission, and
default changes with `execution_not_ready`, except accept/reject of the one pending candidate.
- A crash after acceptance stays `activation_recovery_required` until the
exact accepted catalog is active.

## Rationale

The layer was present in `m4plus_concept.md` but explicitly excluded by architecture 22 ("session defaults/overrides" in
the non-goals). The user directed that this exclusion is not accepted and the layer must be fully covered, so it is
adopted as an accepted future direction owned by a dedicated package.

## Compatibility and non-goals

This decision supersedes the "session defaults/overrides" exclusion in architecture 22's non-goals and the absence of a
disposition for the layer in the retired reconciliation registers. The closed M4 baseline, M3/M4 bytes, and existing
behavior remain unchanged, and no code changes are authorized by this decision.

Live reload, credential rotation, health testing, discovery, pricing, telemetry, profile UI, and production activation
remain outside this decision. M5-M9 are not renumbered.

Owner: architecture 29. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).
