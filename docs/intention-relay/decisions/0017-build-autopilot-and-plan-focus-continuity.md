# ADR 0017: Build Autopilot and Plan Focus Continuity

## Status

Accepted. This decision activates the product direction for the transition
from limited autonomy to near-full trusted-local autonomy. It does not grant
OS-level isolation and does not authorize implementation by itself; the owning
implementation specification and quality-policy updates remain required.

## Decision

Intention Relay exposes one autonomous product mode: **Build Autopilot**.

- Plan is a planning-focused mode, not a sandbox. Its `execute` capability is
  available and audited. A daemon-owned instruction asks the model to focus on
  planning and avoid intentional mutation, but that instruction is advisory.
- Ordinary typed Plan `write` and `edit` operations against project files remain
  hard-denied. Plan-artifact changes continue through the typed plan-owner
  workflow.
- Build Autopilot permits every configured active capability, including writes,
  deletion, bulk deletion, process execution, network, and external actions,
  without per-action confirmation. It does not create OS privileges or bypass
  the daemon-owned capability path.
- Approving a plan automatically starts a fresh Build Autopilot run in the same
  Session by default. The Session and safe conversational context remain; the
  Plan run is terminal and the Build run receives a new `RunId`.
- An optional implementation handoff may create a new Session from a frozen,
  credential-free snapshot of all available safe conversational context, the
  approved plan, and an execution prompt. It never transfers live runtime
  state, credentials, grants, queues, or unfinished effects.

## Rationale

The product should remove repetitive permission prompts and giant user-authored
policy prompts while retaining a clear Plan-to-Build decision. Plan is useful
because it focuses the model and produces a durable specification, not because
it can guarantee shell containment. Build Autopilot is the explicit trust
boundary at which the user delegates the configured Build surface.

Same-Session continuity preserves user intent and avoids forcing users to
restate a large plan. A new Run identity still preserves lifecycle, persistence,
recovery, and no-resume correctness.

## Normative invariants

1. Plan `execute` is available but never described as sandboxed or guaranteed
   read-only.
2. Plan project `write`/`edit` remain incompatible through ordinary typed tools.
3. Plan prompt guidance cannot create authority, change mode, or prevent shell
   side effects.
4. Build Autopilot has no per-action confirmation barrier for configured active
   capabilities.
5. Build Autopilot authority originates only from an explicit user Plan
   approval/Build start transition.
6. Plan approval records the exact `PlanId`, `PlanRevisionId`, and digest.
7. Same-Session continuation preserves `SessionId` but creates a new `RunId`.
8. The old Plan run, provider request, tool call, process, kernel, MCP, and
   bridge state are never resumed or reattached.
9. The Build run binds an immutable mode, Autopilot policy, plan reference, and
   safe context projection.
10. Optional handoff uses a bounded immutable safe snapshot and creates an
    independent Session; it does not transfer authority or live resources.
11. All effects remain behind the single daemon-owned typed capability path.
12. No external effect occurs inside a durable semantic transaction.
13. A started operation without terminal proof remains `ExternalEffectUnknown`;
    it is never automatically retried, resumed, or treated as rolled back.
14. Recovery always uses fresh admission and a new `RunId`.
15. Audit is evidence, not proof of rollback or absence of external effects.
16. No secret, raw provider resource, live handle, or hidden plan frontmatter
    crosses a public or durable projection.
17. Existing M3/M4 bytes and ordinary historical behavior remain unchanged.

## Compatibility and supersession

This decision supersedes only the conflicting future Plan/Build statements in
architecture 07 and the related future delivery statements. It does not amend
M3/M4 behavior, the closed M4 charter, or historical records. It preserves
ordinary typed Plan `write`/`edit` denial while changing Plan `execute` from a
prompt-directed limitation to an explicitly available, trusted-local,
advisory operation.

Same-Session continuity is not Run resumption. The existing one-active-run,
append-only history, commit-before-effect, unknown-effect, and no-resume rules
remain authoritative.

## Security and residual risk

This decision intentionally accepts:

- Plan `execute` may mutate project or system state despite the advisory prompt;
- Build Autopilot may delete many files and perform external actions without
  per-action confirmation;
- there is no OS-level sandbox, container, or process isolation;
- prompt injection and stale/adversarial context may influence model behavior;
- cancellation is not rollback.

The system must not claim that Plan is read-only, that WorkspaceRoot contains
shell descendants, or that audit proves an external effect did not occur.

## Affected documents

- `architecture/02-dto-and-contract-policy.md`
- `architecture/04-sessions-runs-events-and-storage.md`
- `architecture/05-tools-workspace-and-hooks.md`
- `architecture/07-plan-and-build-modes.md`
- `architecture/10-test-driven-delivery-and-verification.md`
- `architecture/11-implementation-roadmap.md`
- `architecture/14-run-execution-meaning-and-historical-compatibility.md`
- `architecture/15-tool-registry-and-mandate-tool-loop.md`
- `architecture/21-goals-skills-context-memory-and-compaction.md`
- `architecture/23-non-destructive-session-branching-and-regeneration.md`
- `architecture/24-activity-ui-and-adapters.md`
- `reconciliation/README.md`
- `reconciliation/source-of-truth-matrix.md`
- `reconciliation/contradiction-register.md`
- `reconciliation/evidence-register.md`
- `quality/architecture.toml`

## Required evidence

The activation must add typed contracts, architecture checks, fault-injection,
recovery, redaction, context-continuity, Plan execute, Build Autopilot, and
optional-handoff outcome tests. It must run `make quick`, `make verify`, and the
required Linux/Windows CI matrix. The implementation specification must declare
exact crate owners, wire/storage versions, features, coverage tiers, migration
behavior, and accepted residual risks.
