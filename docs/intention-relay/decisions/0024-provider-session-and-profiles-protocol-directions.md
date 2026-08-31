# ADR 0024: Post-M5 Provider Session Selection and Profiles Protocol Directions

## Status

Accepted 2026-08-30. This decision records the provider session-selection and
profiles protocol layer as an accepted future direction. It does not activate
implementation: the layer is bound to
[Milestone 5+](../architecture/11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment)
and is implemented only through a separately accepted activating specification
at the start of that milestone.

## Decision

The provider session-selection and profiles protocol layer from
[`m4plus_concept.md`](../m4plus_concept.md) is adopted as the accepted future
direction and owned by the new [Provider Session Selection and Profiles Protocol](../architecture/29-provider-session-and-profiles-protocol.md)
package:

- session default selection (`SetSessionProviderProfileCommandDto`,
  `GetSessionProviderProfileQueryDto`);
- per-turn and fork overrides on `SendUserTurn`/`ForkSession`/`StartForkRun`,
  persisting one `ResolvedRunProviderSelectionDto` per run;
- unavailable-queue promotion (8 per terminal transition) and reconciliation
  (`ReconcileUnavailableQueueCommandDto`, 32 per page);
- profile-keyed usage aggregation with no double counting;
- `provider_profiles_v1` public protocol: paginated catalog reads, catalog
  status, session default query/command, safe overrides, resolved-selection
  projections, pending-removal accept/reject, and readiness projection;
- startup-only application with pending-removal (30-minute lifetime) and
  degraded read-only recovery; and
- held recovery-promoted run admission (`AdmitRecoveredRunCommandDto`).

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after
a later activating specification, and is bound to Milestone 5+ in the roadmap.

## Rationale

The authoritative package review of 2026-08-30 confirmed the provider
session-selection and protocol layer is present in `m4plus_concept.md` but
explicitly excluded in architecture 22 ("session defaults/overrides" in the
non-goals). The user directed that this exclusion is not accepted and the layer
must be fully covered. This decision adopts the layer as an accepted future
direction owned by a dedicated package, while preserving the project rule that
no feature is documented as implemented without code evidence.

## Normative invariants

1. A session copies the global `ProviderProfileId` as a durable future default;
   catalog changes never cascade.
2. A per-turn or fork override affects only that run; one profile per run, no
   fallback chain.
3. Unavailable promotion proceeds FIFO through at most 8 selections per terminal
   transition; exhaustion writes a reconciliation-needed marker; reconciliation
   handles at most 32 per page and never reroutes to a current default.
4. Usage is keyed by exact profile identity and revision and is never
   double-counted; no price, currency, or inferred cost.
5. `provider_profiles_v1` is additive and negotiated; adapters never write
   TOML, edit profiles/kinds, enter credentials, or receive configuration paths.
6. Profiles are startup-only; a recovery-promoted run is never auto-scheduled
   and is admitted only through the idempotent
   `AdmitRecoveredRunCommandDto`.
7. M3/M4 bytes, queue tickets, sessions, runs, events, snapshots, replay, and
   recovery remain authoritative and unchanged.

## Failure semantics

- Registry failure returns `provider_profile_runtime_unavailable`; an
  unavailable immutable selection terminalizes `Starting -> Failed` with
  `provider_configuration_unavailable` and a closed detail, with no provider
  call.
- A degraded daemon rejects all provider state changes/admission/promotion/
  default changes with `execution_not_ready`, except accept/reject of the one
  pending candidate.
- A crash after acceptance stays `activation_recovery_required` until the
  exact accepted catalog is active.

## Compatibility and supersession

This decision supersedes the "session defaults/overrides" exclusion in
architecture 22's non-goals and the absence of a disposition for the layer in
the reconciliation registers. The closed M4 baseline, M3/M4 bytes, and existing
behavior remain unchanged. Activation remains deferred: no code changes are
authorized by this decision.

## Security and residual risk

The layer remains trusted-local. Selections, projections, and failures are
bounded and credential-free; redaction stays central and every activating
specification must pass the fake-secret regression suite.

## Affected documents

- [`architecture/11-implementation-roadmap.md`](../architecture/11-implementation-roadmap.md)
- [`architecture/22-provider-evolution-profiles-and-reasoning.md`](../architecture/22-provider-evolution-profiles-and-reasoning.md)
- [`architecture/29-provider-session-and-profiles-protocol.md`](../architecture/29-provider-session-and-profiles-protocol.md) (new)
- [`reconciliation/README.md`](../reconciliation/README.md)
- [`reconciliation/source-of-truth-matrix.md`](../reconciliation/source-of-truth-matrix.md)
- [`reconciliation/contradiction-register.md`](../reconciliation/contradiction-register.md)
- [`reconciliation/concept-supersession-index.md`](../reconciliation/concept-supersession-index.md)
- [`reconciliation/deferred-excluded-register.md`](../reconciliation/deferred-excluded-register.md)
- [`reconciliation/evidence-register.md`](../reconciliation/evidence-register.md)
- [`reconciliation/compatibility-register.md`](../reconciliation/compatibility-register.md)
- [`decisions/README.md`](README.md)

## Required evidence

No implementation evidence is claimed. The activating specification must declare
exact crates, DTO/wire/storage versions, feature profiles, coverage tiers,
fixtures, and outcome evidence, and pass `make quick`, `make verify`, and
Linux/Windows CI before acceptance.

## Non-goals

This decision does not implement the layer; it does not change M3/M4 behavior;
it does not renumber M5--M9; it does not activate a crate, schema, migration,
protocol, or feature. Live reload, credential rotation, health testing,
discovery, pricing, telemetry, profile UI, and production activation remain
outside this decision.
