# 0034: Post-M5 Accepted Directions — Kernel Output Projection, Retention Policy, Supervision Topology, Calendar Semantics, and Activity Limit Classification

## Status

Accepted 2026-08-30. Not implemented; activation requires an activating specification.

Amended 2026-09-30 by [ADR 0048](0048-limits-by-precedent-and-no-content-scanning.md): item 5, the activity
numeric-limit classification, is superseded. The other four directions stand.

## Decision

The following items, recorded as `Defer` in the retired deferred/excluded register (EXC-008, EXC-009, EXC-010, EXC-011,
and EXC-015), are adopted as accepted future directions for execution in Milestone 5+, except item 5, which is
superseded as recorded below:

**Kernel output projection (owner: architecture 20):**
1. **Rich MIME/raw kernel output** — a future bounded, credential-free kernel
output projection surface for rich MIME and raw display values, never substituting for the closed text-only safe
projection, never crossing public or durable boundaries unredacted, and never becoming authority. Raw Jupyter frames,
binary data, arbitrary display metadata, raw tracebacks, Python objects, and resources remain private in the first
scope.

**Retention, deletion, and garbage collection (owner: architecture 04):**
2. **Physical deletion/GC of historical work** — a future explicit,
user-authorized retention, deletion, and garbage-collection policy for historical work, never rewriting or corrupting
history, never destructive to descendants or audit dependencies, and never a silent automatic cleanup. Archive-only
retention remains the first-scope default.

**Worker and process supervision topology (owner: architecture 03):**
3. **Worker/process supervision topology** — a future production supervision
topology for long-lived workers and processes, never a second runtime, registry, scheduler, persistence authority, or
sandbox. No production supervisor is activated by documentation.

**Calendar, interval, time-zone, and DST semantics (owner: architecture 16):**
4. **Calendar/interval/time-zone/DST semantics** — a future typed
calendar/interval/time-zone/DST semantics package for Mandate scheduler triggers, never a product ceiling and never a
Mandate admission quota.

**Activity numeric limit classification (owner: architecture 24):**
5. **Activity numeric product ceilings** — superseded by
[ADR 0048](0048-limits-by-precedent-and-no-content-scanning.md): a numeric activity bound is introduced only with a
recorded precedent, never by a classification exercise and never as a Mandate quota or child-graph limit.

Each direction keeps M3/M4 behavior authoritative for existing persisted runs and snapshots, affects fresh runs only
after its own activating specification, and remains bound to Milestone 5+.

## Normative invariants

1. Every direction is non-authorizing until its activating specification: it
creates no `RunId`, reason, lifecycle transition, scheduler candidate, tool permission, child edge, verifier authority,
MCP capability, bridge grant, kernel epoch, context projection, branch, or reconciliation result.
2. M3/M4 startup-only configuration, recorded revisions, persisted snapshots,
sessions, runs, events, and bytes remain authoritative and unchanged.
3. Fresh-run-only: each direction affects only future runs after activation.
4. No direction rewrites historical bytes, assigns new meaning to a closed
variant, or reconstructs missing meaning from current state.
5. Rich MIME/raw kernel output never crosses public or durable boundaries
unredacted; the closed text-only safe projection remains authoritative.
6. Physical deletion/GC is explicit user-authorized and never destructive to
descendants or audit dependencies; archive-only retention remains the first-scope default.
7. Supervision topology never becomes a second runtime, registry, scheduler,
persistence authority, or sandbox.
8. Calendar/interval/time-zone/DST semantics are typed and never a Mandate
admission quota.
9. A numeric activity value requires a recorded precedent before it exists and
never becomes a Mandate quota or child-graph limit.

## Failure semantics

- Each direction fails closed before effect when its future contract is
unsupported or inconsistent; no partial projection or partial deletion is delivered.
- Recovery never resumes, retries, reattaches, or reruns work under any of the
five directions.
- Retention or deletion that cannot be applied atomically and safely fails
closed and leaves historical bytes readable.

## Rationale

The five items were recorded as `Defer` in the retired deferred/excluded register with reconsideration triggers (kernel
projection contract, storage retention decision, runtime supervision design, scheduler calendar package, and M6 activity
activation) but had no Milestone 5+ delivery home. Adopting them schedules the directions for execution in Milestone 5+
rather than indefinite deferral, without documenting any feature as implemented.

## Compatibility and non-goals

This decision supersedes the "Defer" wording for the five items (EXC-008, EXC-009, EXC-010, EXC-011, and EXC-015) and
the corresponding deferral wording in architectures 03, 04, 16, 20, and 24. Item 5 is superseded by ADR
0048; the other four directions stand. The closed M4 baseline, M3/M4 bytes, and existing behavior remain unchanged, and
no code changes are authorized by this decision. The directions remain non-authorizing until their M5+ activating
specifications, and M5-M9 are not renumbered.

Owner: architectures 03, 04, 16, 20, and 24. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).
