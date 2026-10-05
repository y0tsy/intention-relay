# 0023: Post-M5 Goal Domain and Verification Directions

## Status

Accepted 2026-08-30. Not implemented; activation requires an activating specification.

## Decision

The Goal aggregate domain from `m4plus_concept.md` is adopted as an accepted future direction owned by [architecture
28](../architecture/28-goal-domain-and-verification.md):

- Goal identity, scope, and tree (project/session scopes, obligatory children,
DAG integrity, bounds 256/64/16/32/64);
- Goal lifecycle, readiness, and user decision (`Active`/`NeedsRework`/
`Paused`/`Stopped`/`Archived`, `Ready`, `AcceptedWithException`);
- leading-goal run selection (`GoalRunSelectionV1`);
- delegated Verification Mandates (authority, target sets, operation matrix);
- verification gates and evidence (`ReferenceGate`, `ExecutableGate`);
- working memory, roles, and templates (`MemoryKindDto`, reusable `sub_agent`
roles);
- model proposals and user confirmation (`RefinementDraftDto`);
- the conversation-compaction working form (`ConversationSummaryDto`);
- Goal-domain bounds and the closed `goal_*`/`memory_*`/`skill_*`/
`delegation_role_*`/`compaction_*`/`refinement_draft_*` safe failures.

The former `run-execution-meaning-v4` carrier is removed with the canonical codec by [ADR
0046](0046-typed-serde-json-contracts.md); the selection records above are typed serde JSON records owned by
architecture 28.

For new Mandate work, Goals remain acceptance/evidence records, not the work-authorization plane. Each direction keeps
M3/M4 behavior authoritative, affects fresh runs only after its own activating specification, and remains bound to
Milestone 5+.

## Normative invariants

1. A Goal is a user-managed, immutable revisioned acceptance/evidence record,
not an instruction channel or execution authority.
2. M3/M4 sessions, runs, events, snapshots, replay, and
recovery remain authoritative; no historical record gains synthetic Goal state.
3. A project Goal enters a session only through an explicit durable link;
every child is an obligatory component; the tree is a DAG with no cycles.
4. `Ready` requires exact effective revision plus every obligatory child plus
every required gate; an exception names only a known failed, unavailable, expired, or ambiguous gate and never creates
success.
5. A run is ordinary or goal-directed with exactly one leading Goal; admission
is atomic and never reconstructs a selection from current state.
6. Verifier mutation requires exact issued, revisioned, target-scoped
authority; user commands win conflicts; an interrupted verifier execution yields a bounded `Partial` result and never
mutates the target.
7. Recovery never resumes, retries, reattaches, or reruns Goal, gate, memory,
or proposal work; a later attempt is fresh.

## Failure semantics

- Bound failures are known typed pre-effect rejections; no content is
truncated or partly committed.
- A stale base for a proposal or mutation is a typed conflict; rejection
changes no active record.
- A started verifier or gate action interrupted or lost before a final result commits a bounded `Partial` result with
its notice and is never retried; nothing pauses.

## Rationale

The Goal aggregate domain was present in `m4plus_concept.md` but absent from the authoritative documentation:
architecture 21 covered only `GoalContextSelectionV1` and the Skill model, while the Goal
identity/lifecycle/readiness/selection, verification gates, working memory, proposals, and compaction working form had
no owner. Adopting the domain records it as an accepted future direction without documenting any feature as implemented.

## Compatibility and non-goals

This decision supersedes the absence of a disposition for the Goal aggregate domain in the retired reconciliation
registers. Architecture 21 retains context selection and projection; detailed Goal-domain semantics are owned by
architecture 28. The closed M4 baseline, M3/M4 bytes, and existing behavior remain unchanged, and no code changes are
authorized by this decision.

Autonomous continuation, post-disconnect requeue, multimodal payloads, dynamic extensions, physical deletion, and worker
administration remain excluded pending separate future decisions. M5-M9 are not renumbered.

Owner: architecture 28. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).
