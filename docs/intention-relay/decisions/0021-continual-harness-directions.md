# ADR 0021: Post-M5 Continual-Harness Directions

## Status

Accepted 2026-08-30. This decision records the continual-harness model as an
accepted future direction. It does not activate implementation: the model is
bound to [Milestone 5+](../architecture/11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment)
and is implemented only through a separately accepted activating specification
at the start of that milestone.

## Decision

The continual-harness model from
[`m4plus_concept2.md`](../m4plus_concept2.md) is adopted as the accepted future
direction and owned by the new [Continual Harness](../architecture/26-continual-harness.md)
package:

- user-managed durable rules at project or ordinary-user-session scope, each
  owning a separate daemon-owned service session with at most one active run;
- typed rule lifecycle (create/read/update-as-new-revision/pause/resume/explicit
  launch/cancel-active-run/archive), where archive is pause with retention;
- closed trigger sources (explicit launch, project-time-zone calendar, fixed
  equal interval, selected known terminal outcome), durable pre-admission
  capture, coalescing, and at most one catch-up reason;
- schedule/time rules: minimum one-minute equal interval, project time zone,
  DST nearest-valid-local-time, clock-change no-repeat;
- two-layer dossiers, verified checkpoints, and safe conclusions, each bounded
  at 512 KiB and rejected rather than truncated;
- read-and-delegate execution classes (`Light`/`Medium`/`Heavy`) that may only
  narrow inherited limits, with `sub_agent` admitted only through a
  user-confirmed typed corridor under architecture 27;
- code-owned bounds (64 rules, 16 concurrent, 16 sources, depth 8, 256 launches,
  and related) classified as intrinsic/capacity/product, never Mandate quotas;
- cancellation cascade, restart `Interrupted`, no-resume recovery, durable
  journal, and post-commit reread publication.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after
a later activating specification, and is bound to Milestone 5+ in the roadmap.

## Rationale

The authoritative package review of 2026-08-30 confirmed the continual-harness
model is present in `m4plus_concept2.md` but absent from the authoritative
documentation: no architecture, ADR, or reconciliation row adopted it, and the
deferred/excluded register had no row for the model itself (EXC-011 covers only
calendar/interval/time-zone semantics). This decision adopts the model as an
accepted future direction so the authoritative documentation no longer leaves
the feature unmapped, while preserving the project rule that no feature is
documented as implemented without code evidence.

## Normative invariants

1. A continual harness is not an autonomous agent, persistent process, or
   second runtime authority; it is a user-managed set of durable rules.
2. M3/M4 queue tickets, sessions, runs, events, snapshots, replay, and recovery
   remain authoritative; no harness rule becomes a queue ticket or Mandate
   reason.
3. Every trigger is durably captured before admission with a stable reason
   identity; redelivery never creates a second run; at most one coalesced
   catch-up reason is admitted after downtime.
4. Harness classes narrow inherited limits only and never weaken WorkspaceRoot,
   Plan/Build, hooks, confirmation, redaction, admission, or
   `model_stream_progress_timeout_v1`.
5. `sub_agent` is admitted only through the user-confirmed typed corridor under
   architecture 27; a harness never calls `ask_user` or receives fallback
   authorization.
6. Harness bounds are intrinsic/capacity/product-classified and never become
   Mandate admission quotas.
7. Recovery never resumes, retries, reattaches, or reruns old work; a later
   attempt is a separately admitted launch with new identities.

## Failure semantics

- Limit failures are known typed pre-effect rejections; waiting for a free
  concurrency slot retains the coalesced reason rather than dropping it.
- Oversized dossiers, checkpoints, or conclusions are rejected rather than
  truncated; an older checkpoint is never presented as the state of the current
  run.
- Daemon restart marks unfinished harness runs `Interrupted`; no external
  action resumes, retries, or reruns.

## Compatibility and supersession

This decision supersedes the "continual-harness ownership remain deferred or
historical-only" wording in the reconciliation supersession index and the
calendar/interval deferral in EXC-011 only to the extent needed to record the
model as an adopted future direction. The closed M4 baseline, M3/M4 bytes, and
existing behavior remain unchanged. Activation remains deferred: no code
changes are authorized by this decision.

## Security and residual risk

The harness remains trusted-local. Dossiers, checkpoints, and conclusions are
bounded and credential-free; redaction stays central and every activating
specification must pass the fake-secret regression suite.

## Affected documents

- [`architecture/11-implementation-roadmap.md`](../architecture/11-implementation-roadmap.md)
- [`architecture/26-continual-harness.md`](../architecture/26-continual-harness.md) (new)
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

This decision does not implement the harness; it does not change M3/M4
behavior; it does not renumber M5--M9; it does not activate a crate, schema,
migration, protocol, or feature. Autonomous continuation, post-disconnect
requeue, multimodal payloads, plugin/MCP installation, process supervision, and
deletion/GC remain excluded pending separate future decisions.
