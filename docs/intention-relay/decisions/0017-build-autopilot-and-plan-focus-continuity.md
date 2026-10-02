# 0017: Build Autopilot and Plan Focus Continuity

## Status

Accepted.

## Decision

Intention Relay exposes one autonomous product mode: **Build Autopilot**.

- Plan is a planning-focused mode, not a sandbox. Its `execute` capability is
available and audited. A daemon-owned instruction asks the model to focus on planning and avoid intentional mutation,
but that instruction is advisory.
- Ordinary typed Plan `write` and `edit` operations against project files
remain hard-denied. Plan-artifact changes continue through the typed plan-owner workflow.
- Build Autopilot permits every configured active capability, including
writes, deletion, bulk deletion, process execution, network, and external actions, without per-action confirmation. It
creates no OS privileges and bypasses no part of the daemon-owned capability path.
- Approving a plan starts a fresh Build Autopilot run in the same Session by
default. The Session and safe conversational context remain; the Plan run is terminal and the Build run receives a new
`RunId`.
- An optional implementation handoff may create a new Session from a frozen,
credential-free snapshot of all available safe conversational context, the approved plan, and an execution prompt. It
transfers no live runtime state, credentials, grants, queues, or unfinished effects.

## Rationale

The product should remove repetitive permission prompts and giant user-authored policy prompts while retaining a clear
Plan-to-Build decision. Plan is useful because it focuses the model and produces a durable specification, not because it
contains shell execution. Build Autopilot is the explicit trust boundary at which the user delegates the configured
Build surface. Same-Session continuity preserves user intent and avoids forcing users to restate a large plan; the new
Run identity still preserves lifecycle, persistence, recovery, and no-resume correctness.

## Normative invariants

1. Plan `execute` is available but never described as sandboxed or guaranteed
read-only.
2. Plan project `write`/`edit` remain incompatible through ordinary typed
tools.
3. Plan prompt guidance cannot create authority, change mode, or prevent shell
side effects.
4. Build Autopilot has no per-action confirmation barrier for configured
active capabilities.
5. Build Autopilot authority originates only from an explicit user Plan
approval or Build start transition.
6. Plan approval records the exact `PlanId`, `PlanRevisionId`, and digest.
7. Same-Session continuation preserves `SessionId` but creates a new `RunId`.
8. The old Plan run, provider request, tool call, process, kernel, MCP, and
bridge state are never resumed or reattached.
9. The Build run binds an immutable mode, Autopilot policy, plan reference,
and safe context projection.
10. Optional handoff uses a bounded immutable safe snapshot and creates an
independent Session; it transfers no authority or live resources.
11. All effects remain behind the single daemon-owned typed capability path.
12. No external effect occurs inside a durable semantic transaction.
13. A started operation without terminal proof remains `ExternalEffectUnknown`;
it is never automatically retried, resumed, or treated as rolled back.
14. Recovery always uses fresh admission and a new `RunId`.
15. Audit is evidence, not proof of rollback or absence of external effects.
16. No secret, raw provider resource, live handle, or hidden plan frontmatter
crosses a public or durable projection.
17. Existing M3/M4 bytes and ordinary historical behavior remain unchanged.

## Compatibility and non-goals

This decision supersedes the conflicting future Plan/Build statements in [architecture
07](../architecture/07-plan-and-build-modes.md) and the related future delivery statements. It changes Plan `execute`
from a prompt-directed limitation to an explicitly available trusted-local advisory operation while preserving ordinary
typed Plan `write`/`edit` denial. It does not amend M3/M4 behavior, the closed M4 charter, or historical records.

Same-Session continuity is not Run resumption: one-active-run, append-only history, commit-before-effect,
unknown-effect, and no-resume rules remain authoritative. No sandboxing, rollback, implicit authority, or automatic
retry is added.

## Security and residual risk

This decision intentionally accepts:

- Plan `execute` may mutate project or system state despite the advisory
prompt;
- Build Autopilot may delete many files and perform external actions without
per-action confirmation;
- there is no OS-level sandbox, container, or process isolation;
- prompt injection and stale or adversarial context may influence model
behavior;
- cancellation is not rollback.

The system must not claim that Plan is read-only, that `WorkspaceRoot` contains shell descendants, or that audit proves
an external effect did not occur.

Owner: architecture 07. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).
