# Milestone-planning checklists

Use these checklists with `plan-intention-relay-milestone/SKILL.md`. They make planning outputs comparable across milestones without replacing the current authoritative architecture documents.

## Pre-draft template

```markdown
## Pre-draft: M<n> <title>

### Request and success criteria
- Target milestone:
- User-requested outcome:
- Explicit constraints/exclusions:
- Planning completion criteria:

### Source constraints
- Roadmap:
- Applicable architecture documents:
- TDD/TTD and quality requirements:
- Predecessor/closeout status:
- Existing implementation and policy facts:

### Confirmed decisions
| Topic | Decision | Evidence or rationale | Affects |
| --- | --- | --- | --- |
| | | | |

### Implementation-required decisions
- [ ] <decision that implementation must make, owner, and required test evidence>

### Open questions and blockers
- [ ] <question/blocker, why it matters, and resolution owner>

### Risks and expected failure behavior
- <risk, detection, typed/observable failure, recovery or deferral>

### Explicit deferrals and non-goals
- <item and owning later milestone>
```

## Decision-record template

For each material user choice, record:

| Field | Required content |
| --- | --- |
| Decision | The exact choice, in neutral technical language. |
| Context | Why the target milestone needs the decision. |
| Source constraints | Roadmap, architecture, policy, accepted baseline, or current-code facts. |
| Considered options | Every option presented to the user, including custom answers. |
| Trade-offs | Benefits, drawbacks, implementation/test/quality impact, and later-scope effect. |
| Rationale | User reason or the agreed recommendation. |
| Affected surfaces | DTOs, crates, policies, tests, docs, adapters, CI, or closeout evidence. |
| Deferrals | Work explicitly retained by another milestone. |

## Discovery readiness

- [ ] One target milestone is named, or a bounded ordered sequence is explicitly approved.
- [ ] `AGENTS.md` and documentation index were read.
- [ ] Roadmap, TDD/TTD, and quality-gate policy were read.
- [ ] Milestone-specific architecture sources were read.
- [ ] Relevant closeout evidence was checked for predecessor status, immutable baseline, scope boundaries, and known exceptions.
- [ ] Existing crates, tests, policies, manifests, and call sites were inspected before naming them in the draft.
- [ ] Working-tree status was inspected and existing changes were treated as user work.
- [ ] The milestone is classified as unstarted, in progress, closed, or corrective.
- [ ] Any dependency blocker or source conflict is visible in the pre-draft.

## Question quality

- [ ] Every question is materially unresolved by source documentation or the user request.
- [ ] Before `AskUser`, the conversation explains what is being decided and why.
- [ ] Every option describes what it is, benefits, drawbacks, operational consequences, and scope impact.
- [ ] A recommendation is labeled as a recommendation, not an implicit decision.
- [ ] Each question round has no more than four independent decisions.
- [ ] The option labels passed to `AskUser` are short, mutually exclusive where needed, and consistent with the detailed brief.
- [ ] A request for more explanation triggers a revised brief and repeated question, not inference.
- [ ] Each confirmed response was captured in the pre-draft decision record.

## Draft readiness

- [ ] Objective, scope, ownership, dependencies, invariants, non-goals, and failure behavior are explicit.
- [ ] Decided, implementation-required, deferred, and blocked items are visibly distinct.
- [ ] Every cross-crate/process boundary is DTO-first and adapter isolation is preserved.
- [ ] No requirement bypasses `WorkspaceRoot`, runtime ownership, or other decided architectural policy.
- [ ] Tests are named by layer: DTO, domain, contract, architecture, storage/runtime/tool/provider/adapter as applicable, integration, and outcome.
- [ ] Observable acceptance outcomes are stated separately from implementation details.
- [ ] Quality tiers, Cargo feature profiles, pinning, and Makefile requirements are included when affected.
- [ ] Policy, quality, architecture, README, and closeout documentation updates are included where the planned change requires them.
- [ ] Risks and typed/observable failure behavior are stated.
- [ ] Deferrals name their owner milestone and are not described as delivered behavior.

## Final implementation-spec check

- [ ] Every existing file, crate, symbol, command, and policy path named in the specification was verified in the current repository.
- [ ] New files are explicitly marked new.
- [ ] Each step names purpose, boundary owner, behavior, tests first, validation, and acceptance evidence.
- [ ] The plan uses `make quick` during implementation and `make verify` before milestone acceptance, unless authoritative policy changes those commands.
- [ ] Any commit grouping was requested by the user and remains atomic/coherent.
- [ ] The plan does not include unrelated external configuration or incident work without explicit user direction.
- [ ] All material choices are resolved; remaining blockers prevent approval rather than hiding in a plan.
- [ ] The final specification is sent for explicit user approval, or via `ExitSpecMode` when Spec mode is active.
