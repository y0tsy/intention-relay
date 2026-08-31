# ADR 0023: Post-M5 Goal Domain and Verification Directions

## Status

Accepted 2026-08-30. This decision records the Goal aggregate domain and
verification model as an accepted future direction. It does not activate
implementation: the model is bound to
[Milestone 5+](../architecture/11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment)
and is implemented only through a separately accepted activating specification
at the start of that milestone.

## Decision

The Goal aggregate domain from
[`m4plus_concept.md`](../m4plus_concept.md) is adopted as the accepted future
direction and owned by the new [Goal Domain and Verification](../architecture/28-goal-domain-and-verification.md)
package:

- Goal identity, scope, and tree (project/session scopes, obligatory children,
  DAG integrity, bounds 256/64/16/32/64);
- Goal lifecycle, readiness, and user decision (`Active`/`NeedsRework`/
  `Paused`/`Stopped`/`Archived`, `Ready`, `AcceptedWithException`);
- leading-goal run selection (`GoalRunSelectionV1` in
  `run-execution-meaning-v4`);
- delegated Verification Mandates (authority, target sets, operation matrix,
  reconciliation);
- verification gates and evidence (`ReferenceGate`, `ExecutableGate`);
- working memory, roles, and templates (`MemoryKindDto`, reusable `sub_agent`
  roles);
- model proposals and user confirmation (`RefinementDraftDto`);
- the conversation-compaction working form (`ConversationSummaryDto`);
- Goal-domain bounds and the closed `goal_*`/`memory_*`/`skill_*`/
  `delegation_role_*`/`compaction_*`/`refinement_draft_*` safe failures.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after
a later activating specification, and is bound to Milestone 5+ in the roadmap.
For new Mandate work, Goals remain acceptance/evidence records, not the
work-authorization plane.

## Rationale

The authoritative package review of 2026-08-30 confirmed the Goal aggregate
domain is present in `m4plus_concept.md` but absent from the authoritative
documentation: architecture 21 covers only `GoalContextSelectionV1` and the
Skill model, while the Goal identity/lifecycle/readiness/selection, verification
gates, working memory, proposals, and compaction working form had no owner. This
decision adopts the domain as an accepted future direction so the authoritative
documentation no longer leaves the feature unmapped, while preserving the
project rule that no feature is documented as implemented without code evidence.

## Normative invariants

1. A Goal is a user-managed, immutable revisioned acceptance/evidence record,
   not an instruction channel or execution authority.
2. M3/M4 queue tickets, sessions, runs, events, snapshots, replay, and recovery
   remain authoritative; no historical record gains synthetic Goal state.
3. A project Goal enters a session only through an explicit durable link; every
   child is an obligatory component; the tree is a DAG with no cycles.
4. `Ready` requires exact effective revision plus every obligatory child plus
   every required gate; an exception names only a known failed/unavailable/
   expired/ambiguous gate and never creates success.
5. A run is ordinary or goal-directed with exactly one leading Goal; admission
   is atomic and never reconstructs a selection from current state.
6. Verifier mutation requires exact issued, revisioned, target-scoped authority;
   user commands win conflicts; `ResolveUnknownEffect` yields only `Active` or
   `Stopped`.
7. Recovery never resumes, retries, reattaches, or reruns Goal/gate/memory/
   proposal work; a later attempt is fresh.

## Failure semantics

- Bound failures are known typed pre-effect rejections; no content is
  truncated or partly committed.
- A stale base for a proposal or mutation is a typed conflict; rejection
  changes no active record.
- A started verifier or gate action without durable terminal proof remains
  `ExternalEffectUnknown` and is never retried.

## Compatibility and supersession

This decision supersedes the absence of a disposition for the Goal aggregate
domain in the reconciliation registers. Architecture 21 retains context
selection and projection; detailed Goal-domain semantics are owned here. The
closed M4 baseline, M3/M4 bytes, and existing behavior remain unchanged.
Activation remains deferred: no code changes are authorized by this decision.

## Security and residual risk

The domain remains trusted-local. Cards, summaries, proposals, and failures are
bounded and credential-free; redaction stays central and every activating
specification must pass the fake-secret regression suite.

## Affected documents

- [`architecture/11-implementation-roadmap.md`](../architecture/11-implementation-roadmap.md)
- [`architecture/21-goals-skills-context-memory-and-compaction.md`](../architecture/21-goals-skills-context-memory-and-compaction.md)
- [`architecture/28-goal-domain-and-verification.md`](../architecture/28-goal-domain-and-verification.md) (new)
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

This decision does not implement the Goal domain; it does not change M3/M4
behavior; it does not renumber M5--M9; it does not activate a crate, schema,
migration, protocol, or feature. Autonomous continuation, post-disconnect
requeue, multimodal payloads, dynamic extensions, physical deletion, and
worker administration remain excluded pending separate future decisions.
