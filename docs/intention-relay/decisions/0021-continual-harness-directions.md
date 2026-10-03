# 0021: Post-M5 Continual-Harness Directions

## Status

Accepted 2026-08-30. Not implemented; activation requires an activating specification.

## Decision

The continual-harness model from `m4plus_concept.md` is adopted as an accepted future direction owned by [architecture
26](../architecture/26-continual-harness.md):

- user-managed durable rules at project or ordinary-user-session scope, each
owning a separate daemon-owned service session with at most one active run;
- typed rule lifecycle (create/read/update-as-new-revision/pause/resume/
explicit launch/cancel-active-run/archive), where archive is pause with retention;
- closed trigger sources (explicit launch, project-time-zone calendar, fixed
equal interval, selected known terminal outcome), durable pre-admission capture, coalescing, and at most one catch-up
reason;
- schedule and time rules: minimum one-minute equal interval, project time
zone, DST nearest-valid-local-time, clock-change no-repeat;
- two-layer dossiers, verified checkpoints, and safe conclusions, each bounded
at 512 KiB and rejected rather than truncated;
- read-and-delegate execution classes (`Light`/`Medium`/`Heavy`) that may only
narrow inherited limits, with `sub_agent` admitted only through the user-confirmed typed policy under architecture 27;
- code-owned bounds (64 rules, 16 concurrent, 16 sources, depth 8, 256
launches, and related) classified as intrinsic/capacity/product, never Mandate quotas;
- cancellation cascade, restart `Interrupted`, no-resume recovery, durable
journal, and post-commit reread publication.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after its own activating specification, and
remains bound to Milestone 5+.

## Normative invariants

1. A continual harness is not an autonomous agent, persistent process, or
second runtime authority; it is a user-managed set of durable rules.
2. M3/M4 queue tickets, sessions, runs, events, snapshots, replay, and
recovery remain authoritative; no harness rule becomes a queue ticket or Mandate reason.
3. Every trigger is durably captured before admission with a stable reason
identity; redelivery never creates a second run; at most one coalesced catch-up reason is admitted after downtime.
4. Harness classes narrow inherited limits only and never weaken
`WorkspaceRoot`, Plan/Build, hooks, confirmation, redaction, admission, or `model_stream_progress_timeout_v1`.
5. `sub_agent` is admitted only through the user-confirmed typed policy under
architecture 27; a harness never calls `ask_user` or receives fallback authorization.
6. Harness bounds are intrinsic/capacity/product-classified and never become
Mandate admission quotas.
7. Recovery never resumes, retries, reattaches, or reruns old work; a later
attempt is a separately admitted launch with new identities.

## Failure semantics

- Limit failures are known typed pre-effect rejections; waiting for a free
concurrency slot retains the coalesced reason rather than dropping it.
- Oversized dossiers, checkpoints, or conclusions are rejected rather than
truncated; an older checkpoint is never presented as the state of the current run.
- Daemon restart marks unfinished harness runs `Interrupted`; no external
action resumes, retries, or reruns.

## Rationale

The continual-harness model was present in `m4plus_concept.md` but absent from the authoritative documentation; only the
calendar/interval deferral EXC-011 existed. Adopting the model records it as an accepted future direction without
documenting any feature as implemented.

## Compatibility and non-goals

This decision supersedes the deferral wording of EXC-011 in the retired deferred/excluded register only to the extent
needed to record the model as an adopted future direction. The closed M4 baseline, M3/M4 bytes, and existing behavior
remain unchanged, and no code changes are authorized by this decision.

Autonomous continuation, post-disconnect requeue, multimodal payloads, plugin/MCP installation, process supervision, and
deletion/GC remain excluded pending separate future decisions. M5-M9 are not renumbered.

Owner: architecture 26. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).
