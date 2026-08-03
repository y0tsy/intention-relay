---
name: plan-intention-relay-milestone
description: Prepare an approved implementation specification for one named Intention Relay roadmap milestone through repository research, detailed user decision briefs, a visible pre-draft, and a consolidated draft. Use when the user asks to execute, prepare, or plan a specific milestone from docs/intention-relay. Do not use for implementation from an approved specification, generic planning outside this repository, review-only work, or M7 physical plan artifacts.
allowed-tools:
  - Read
  - Grep
  - Glob
  - LS
  - Execute
  - AskUser
  - TodoWrite
  - ExitSpecMode
enabled: true
user-invocable: true
disable-model-invocation: false
compatibility: droid
version: 1.0.0
metadata:
  owner: intention-relay
---

# Plan an Intention Relay milestone

Prepare the decision record and implementation specification for **one explicitly named milestone** in `docs/intention-relay/architecture/11-implementation-roadmap.md`. The output of this skill is an approved implementation specification, not production implementation.

Use this skill when the user asks to execute, prepare, or plan a named Intention Relay milestone and has not already supplied an approved, complete implementation specification.

## Boundaries

Do **not** use this skill for:

- implementing a milestone from an already approved specification;
- a generic plan that is not governed by this repository's Intention Relay documentation;
- a read-only audit, code review, or a request only to enumerate milestones;
- a physical M7 plan artifact, which is product behavior owned by `intention-plans`, not a Droid planning document;
- an unrelated Factory, BYOK, CI, or environment failure unless the user explicitly includes it in the milestone scope.

Do not treat a legacy implementation detail as a target-architecture decision. The architecture documents override legacy implementation details; the selected legacy baseline is only user-facing behavior evidence.

## Completion criteria

Finish only when all of the following are true:

1. The exact milestone and its predecessor status are known.
2. Applicable source documents and the current repository baseline have been inspected.
3. Every unresolved material decision has either been selected by the user or recorded as a blocker.
4. The visible pre-draft and consolidated draft distinguish decided, implementation-required, deferred, and out-of-scope items.
5. The implementation specification names affected boundaries, test-first evidence, quality gates, acceptance outcomes, non-goals, and risks without inventing requirements.
6. In Spec mode, the specification has been submitted with `ExitSpecMode`. Outside Spec mode, the user has explicitly approved it before any implementation begins.

## 1. Identify scope and prepare discovery

1. Create a todo list before broad discovery. Keep exactly one item in progress.
2. Parse the request for:
   - milestone identifier, such as `M4` or `Milestone 4`;
   - requested outcome, correction, constraints, and explicit exclusions;
   - whether the user asks only for a specification or also explicitly authorizes later implementation.
3. If the milestone is absent, ambiguous, or multiple milestones are requested, ask the user to select **one** milestone or a deliberately bounded sequence before drafting. Do not guess.
4. State the observable success criteria for the planning work, then inspect the repository without mutating it.
5. Check `git status --short --branch` before any later write. Treat existing changes as user work.

## 2. Read authoritative sources and current baseline

Read the common sources and the milestone-specific sources listed in [references.md](references.md). At minimum, read:

- `AGENTS.md`;
- `docs/intention-relay/architecture/README.md`;
- the roadmap, TDD/TTD policy, and quality-gate policy;
- the applicable prior closeout evidence, when the milestone has a predecessor or may already be closed;
- relevant current code, tests, machine-readable policies, workspace manifests, and call sites.

Confirm that crates, modules, test targets, policies, and prior baseline claims still exist before placing them in a specification. Inspect enough surrounding code and tests to avoid naming stale paths or proposing duplicate mechanisms.

Determine and report:

- predecessor milestones that must be accepted first;
- whether the target milestone is unstarted, in progress, closed, or has an evidence/implementation gap;
- architecture contracts, quality tiers, feature profiles, and known deferrals that constrain the milestone;
- requirements that are already decided, implementation-required, or deferred.

### Closed or corrective milestones

If closeout evidence says the milestone is closed, do not reopen or reimplement it by default. Compare its recorded immutable baseline and acceptance evidence with the user-reported gap. Define a focused **remediation slice** only for confirmed missing behavior, evidence, or documentation. Preserve accepted scope and defer unrelated enhancements.

If the predecessor is not accepted or its evidence is missing, report the dependency blocker. Ask whether the user wants a predecessor remediation specification, a combined sequence with explicit ordering, or to defer the target milestone.

## 3. Build detailed decision briefs

Only ask the user about choices that are not already decided by authoritative documentation, current accepted evidence, or the user's request.

Before every `AskUser` call, write a decision brief in the conversation. For each question, include:

1. **Decision:** what must be chosen and why it matters now.
2. **Context:** the relevant milestone contract and current baseline.
3. **Each viable option:** what it is, how it works, benefits, drawbacks, affected boundaries, testing consequences, quality/documentation impact, and long-term trade-offs.
4. **Recommendation:** when one option clearly best preserves the architecture, say why. Do not present a recommendation as an already made decision.
5. **Scope boundary:** identify options that would pull later-milestone work into this milestone.

Then use `AskUser` with concise option labels. Ask no more than four genuinely independent questions in one round. Do not ask questions whose answer is already decided by source documentation.

### Answer handling

- Record every answer exactly enough to preserve its intent.
- If the user asks for more detail, advantages, disadvantages, or a repeated question, it is **not a decision**. Expand the brief, repeat the question, and do not infer an answer.
- If the user supplies a custom answer, restate its technical interpretation and ask a narrow follow-up only if it remains materially ambiguous.
- If choices conflict with an invariant or a later-milestone boundary, explain the conflict and ask whether to choose a compliant alternative or explicitly define a new decision/sequence. Never silently weaken an invariant.

## 4. Maintain the visible pre-draft

After discovery begins, maintain a visible session-local **pre-draft** in chat. Update it after each user decision and whenever repository evidence changes the scope. It is a planning record, not a repository artifact.

Use this structure:

```markdown
## Pre-draft: M<n> <title>

### Source constraints
- <architecture, roadmap, quality, baseline, and dependency facts>

### Confirmed decisions
| Topic | Decision | Rationale | Affects |
| --- | --- | --- | --- |
| ... | ... | ... | DTOs, crates, tests, docs, gates |

### Implementation-required decisions
- <item that is deliberately not yet designed>

### Open questions or blockers
- <item, required owner or evidence, and consequence>

### Risks and failure behavior
- <expected failure path and desired typed/observable outcome>

### Explicit deferrals and non-goals
- <later milestone or excluded work>
```

Do not create, edit, or store a pre-draft in the repository during discovery. Do not confuse it with M7 physical plans.

## 5. Consolidate the draft

Once material choices are resolved, turn the pre-draft into a consolidated **draft** in the conversation. Reconcile it against source documents and current code.

The draft must:

- state the milestone objective, scope, owners, dependencies, invariants, and non-goals;
- separate **decided**, **implementation-required**, **deferred**, and **blocked** items;
- name DTO and crate/process boundaries, never implementation resources as cross-boundary contracts;
- describe data flow, lifecycle, or state transitions when they affect correctness;
- define observable outcomes and failure behavior, not only code structure;
- identify test-first DTO, domain, contract, architecture, integration, and outcome evidence appropriate to the slice;
- map every quality requirement to the root Makefile contract and machine-readable policy changes where applicable;
- preserve established coverage tiers and feature-profile rules;
- list repository documentation that must change in the same slice.

Use a Mermaid diagram only where a data flow, lifecycle, or relationship would otherwise be unclear. Keep labels short enough for terminal rendering.

## 6. Produce the implementation specification

Transform the consolidated draft into one concrete implementation specification. Do not leave unresolved alternatives in the specification.

For each implementation step, specify:

1. **Purpose and owning boundary**: crate, policy, test area, or documentation surface.
2. **Exact behavior**: DTOs, commands/events, invariants, state transitions, errors, persistence, or adapter isolation as applicable.
3. **Files and symbols**: existing paths only after verifying them; label genuinely new files as new.
4. **Tests first**: failing/updated contract, domain, architecture, integration, and outcome tests appropriate to the milestone.
5. **Validation**: focused commands while iterating, then `make quick` and `make verify` as required by the project policy.
6. **Acceptance evidence**: observable user or operational outcome, documentation/closeout evidence where relevant.
7. **Non-goals and deferrals**: keep later-milestone work out of scope.

Group work into atomic commits only when the user requests commit structure. Each group must remain technically coherent and independently reviewable; do not invent commits merely to create a fixed count.

## 7. Approval and handoff

Before implementation, present:

- the final draft and resolved decision log;
- assumptions that could still affect implementation;
- scope exclusions, risks, and blockers;
- the concrete specification and validation plan.

When Spec mode is active, submit only the final concrete plan through `ExitSpecMode`. Otherwise ask for explicit approval. Do not implement, commit, or modify project sources as part of this skill unless the user separately authorizes that next phase.

## Interruption and resume

If the user interrupts or cancels the work, stop. In the next response, retain and show the last confirmed pre-draft, list unresolved questions, and identify the next safe discovery step. Do not claim a specification is approved because research was completed.

## External errors

Classify unrelated failures, such as Factory BYOK/provider configuration, unavailable marketplaces, or a separate CI incident, as external context. Keep them outside the milestone specification unless the user explicitly asks to include them. If included, record their ownership, prerequisite, and validation separately from repository code changes.

## Final self-check

Read [checklists.md](checklists.md) before submitting the specification. Verify that every claimed file, command, acceptance criterion, and deferred boundary is supported by current repository evidence.
