# ADR 0034: Post-M5 Accepted Directions — Kernel Output Projection, Retention Policy, Supervision Topology, Calendar Semantics, and Activity Limit Classification

## Status

Accepted 2026-08-30. This decision records five deliberately deferred
register items as accepted future directions to be executed in
[Milestone 5+](../architecture/11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment).
It does not activate implementation: each direction is bound to Milestone 5+
and is implemented only through a separately accepted activating specification
at the start of that milestone.

## Decision

The following items, recorded as `Defer` in the
[deferred/excluded register](../reconciliation/deferred-excluded-register.md)
(EXC-008, EXC-009, EXC-010, EXC-011, and EXC-015), are adopted as accepted
future directions for execution in Milestone 5+:

**Kernel output projection (owner: architecture 20):**
1. **Rich MIME/raw kernel output** — a future bounded, credential-free kernel
   output projection surface for rich MIME and raw display values, never
   substituting for the closed text-only safe projection, never crossing
   public or durable boundaries unredacted, and never becoming authority. Raw
   Jupyter frames, binary data, arbitrary display metadata, raw tracebacks,
   Python objects, and resources remain private in the first scope.

**Retention, deletion, and garbage collection (owner: architecture 04):**
2. **Physical deletion/GC of historical work** — a future explicit,
   user-authorized retention/deletion/garbage-collection policy for
   historical work, never rewriting or corrupting history, never destructive
   to descendants or audit dependencies, and never a silent automatic cleanup.
   Archive-only retention remains the first-scope default.

**Worker and process supervision topology (owner: architecture 03):**
3. **Worker/process supervision topology** — a future production
   supervision topology for long-lived workers and processes,
   never becoming a second runtime, registry, scheduler, persistence
   authority, or sandbox. No production supervisor is activated by
   documentation.

**Calendar, interval, time-zone, and DST semantics (owner: architecture 16):**
4. **Calendar/interval/time-zone/DST semantics** — a future typed
   calendar/interval/time-zone/DST semantics package for Mandate scheduler
   triggers, never a product ceiling and never a Mandate admission quota.
   Harness schedule/time semantics remain owned by architecture 26.

**Activity numeric limit classification (owner: architecture 24):**
5. **Activity numeric product ceilings** — the future classification of
   activity/notification numeric values as intrinsic bounds, capacity
   availability, or ordinary product policy at M5+ activation, never Mandate
   quotas or child-graph limits.

Each direction:

- keeps M3/M4 behavior authoritative for existing persisted runs and
  snapshots;
- affects fresh runs only after a later activating specification;
- is bound to Milestone 5+ in the roadmap;
- requires its own activating specification (crates, DTO/wire/storage
  versions, quality policy, feature profiles, migration declarations, tests,
  and outcome evidence) accepted at the start of the milestone.

## Rationale

The authoritative package review of 2026-08-30 confirmed these five items are
recorded as `Defer` in the deferred/excluded register with reconsideration
triggers (kernel projection contract, storage retention decision, runtime
supervision design, scheduler calendar package, and M6 activity activation) but
have no Milestone 5+ delivery home and no accepted direction adopting them for
execution. This decision adopts them as accepted future directions so they are
scheduled for execution in Milestone 5+ rather than remaining indefinitely
deferred, while preserving the project rule that no feature is documented as
implemented without code evidence.

## Normative invariants

1. Every direction is non-authorizing until its activating specification:
   it creates no RunId, reason, lifecycle transition, scheduler candidate,
   tool permission, child edge, verifier authority, MCP capability, bridge
   grant, kernel epoch, context projection, branch, or reconciliation result.
2. M3/M4 startup-only configuration, recorded revisions, persisted snapshots,
   queue tickets, sessions, runs, events, and bytes remain authoritative and
   unchanged.
3. Fresh-run-only: each direction affects only future runs after activation.
4. No direction rewrites historical bytes, assigns new meaning to a closed
   variant, or reconstructs missing meaning from current state.
5. Rich MIME/raw kernel output never crosses public or durable boundaries
   unredacted; the closed text-only safe projection remains authoritative.
6. Physical deletion/GC is explicit user-authorized and never destructive to
   descendants or audit dependencies; archive-only retention remains the
   first-scope default.
7. Supervision topology never becomes a second runtime, registry, scheduler,
   persistence authority, or sandbox.
8. Calendar/interval/time-zone/DST semantics are typed and never a Mandate
   admission quota; harness schedule/time semantics remain owned by
   architecture 26.
9. Activity numeric values require intrinsic/capacity/ordinary classification
   at activation and never become Mandate quotas or child-graph limits.

## Failure semantics

- Each direction fails closed before effect when its future contract is
  unsupported, unnegotiated, or over-limit; no partial projection, partial
  deletion, or partial classification is delivered.
- Recovery never resumes, retries, reattaches, or reruns work under any of
  the five directions.
- Retention/deletion that cannot be applied atomically and safely fails
  closed and leaves historical bytes readable.

## Compatibility and supersession

This decision supersedes the "Defer" wording for the five items in the
deferred/excluded register (EXC-008, EXC-009, EXC-010, EXC-011, and EXC-015)
and the corresponding deferral wording in architectures 03, 04, 16, 20, and
24 and in the non-goals of ADR 0021 and ADR 0030 ("process supervision, and
deletion/GC remain excluded pending separate future decisions"), and records
them as accepted future directions bound to Milestone 5+.
The closed M4 baseline, M3/M4 bytes, and existing behavior remain unchanged.
Activation remains deferred: no code changes are authorized by this decision.

## Security and residual risk

The directions remain trusted-local. Rich MIME/raw kernel output projection,
retention/deletion, and supervision topology introduce new content-handling
and process-handling surface; every activating specification must keep
credentials and raw content out of durable/public surfaces, keep redaction
central, and pass the fake-secret regression suite. Physical deletion must
never erase audit dependencies, and supervision must never become a second
authority.

## Affected documents

- [`decisions/0021-continual-harness-directions.md`](0021-continual-harness-directions.md)
- [`decisions/0030-continual-harness-safe-failures-and-selection-record-detail.md`](0030-continual-harness-safe-failures-and-selection-record-detail.md)
- [`architecture/03-daemon-transport-and-adapters.md`](../architecture/03-daemon-transport-and-adapters.md)
- [`architecture/04-sessions-runs-events-and-storage.md`](../architecture/04-sessions-runs-events-and-storage.md)
- [`architecture/11-implementation-roadmap.md`](../architecture/11-implementation-roadmap.md)
- [`architecture/16-mandate-scheduler-and-readiness-driven-admission.md`](../architecture/16-mandate-scheduler-and-readiness-driven-admission.md)
- [`architecture/20-ipython-kernel-lifecycle.md`](../architecture/20-ipython-kernel-lifecycle.md)
- [`architecture/24-activity-ui-and-adapters.md`](../architecture/24-activity-ui-and-adapters.md)
- [`reconciliation/README.md`](../reconciliation/README.md)
- [`reconciliation/source-of-truth-matrix.md`](../reconciliation/source-of-truth-matrix.md)
- [`reconciliation/contradiction-register.md`](../reconciliation/contradiction-register.md)
- [`reconciliation/concept-supersession-index.md`](../reconciliation/concept-supersession-index.md)
- [`reconciliation/deferred-excluded-register.md`](../reconciliation/deferred-excluded-register.md)
- [`reconciliation/evidence-register.md`](../reconciliation/evidence-register.md)
- [`reconciliation/compatibility-register.md`](../reconciliation/compatibility-register.md)
- [`reconciliation/ownership-and-dependency-map.md`](../reconciliation/ownership-and-dependency-map.md)
- [`decisions/README.md`](README.md)

## Required evidence

No implementation evidence is claimed. Each direction's activating
specification must declare exact crates, DTO/wire/storage versions, feature
profiles, coverage tiers, fixtures, and outcome evidence, and pass `make
quick`, `make verify`, and Linux/Windows CI before acceptance.

## Non-goals

This decision does not implement the five directions; it does not change M3/M4
behavior; it does not renumber M5--M9; it does not activate a crate, schema,
migration, protocol, or feature. The directions remain non-authorizing until
their M5+ activating specifications.
