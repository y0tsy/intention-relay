# Intention Relay planning references

Read the current source files named below. This map helps select documents; it does not replace their contents or their precedence.

## Authority and precedence

1. [`AGENTS.md`](../../../AGENTS.md) defines repository operating instructions.
2. [`docs/intention-relay/architecture/`](../../../docs/intention-relay/architecture/README.md) defines the approved target architecture and delivery constraints.
3. Explicit future product decisions override legacy implementation details.
4. [`docs/intention-relay/legacy-baseline/`](../../../docs/intention-relay/legacy-baseline/00-manifest.md) provides user-visible capability evidence until explicitly superseded.
5. Preserved broader audit material under `docs/reference/` is research only and cannot override selected baseline or architecture.

## Common sources for every milestone

| Purpose | Source |
| --- | --- |
| Documentation index and precedence | [`docs/intention-relay/README.md`](../../../docs/intention-relay/README.md), [`architecture/README.md`](../../../docs/intention-relay/architecture/README.md) |
| Non-negotiable v1 rules and scope | [`00-principles-and-scope.md`](../../../docs/intention-relay/architecture/00-principles-and-scope.md) |
| Crate ownership and dependency boundaries | [`01-workspace-and-crate-map.md`](../../../docs/intention-relay/architecture/01-workspace-and-crate-map.md) |
| DTO-first contract rules | [`02-dto-and-contract-policy.md`](../../../docs/intention-relay/architecture/02-dto-and-contract-policy.md) |
| Roadmap and milestone acceptance criteria | [`11-implementation-roadmap.md`](../../../docs/intention-relay/architecture/11-implementation-roadmap.md) |
| Test-first requirements and outcome evidence | [`10-test-driven-delivery-and-verification.md`](../../../docs/intention-relay/architecture/10-test-driven-delivery-and-verification.md) |
| Makefile, quality tiers, features, and supply chain | [`12-quality-gates-and-makefile.md`](../../../docs/intention-relay/architecture/12-quality-gates-and-makefile.md) |
| Authority, compatibility, ownership, evidence, and deferral registers | [`reconciliation/README.md`](../../../docs/intention-relay/reconciliation/README.md) |
| Accepted cross-document decisions and their rationale | [`decisions/README.md`](../../../docs/intention-relay/decisions/README.md) |
| Accepted phase status and immutable evidence | [`docs/intention-relay/closeout/`](../../../docs/intention-relay/closeout/) |

## Milestone-specific reading map

| Milestone | Read in addition to common sources |
| --- | --- |
| M0, reproducible quality foundation | `12-quality-gates-and-makefile.md`, applicable `quality/` policy/checker files, CI workflow, M0/M1 closeout evidence |
| M1, contracts/config/workspace skeleton | `01-workspace-and-crate-map.md`, `02-dto-and-contract-policy.md`, `08-model-protocol-and-providers.md`, `09-configuration-security-and-observability.md`, M0/M1 and M1+ evidence |
| M1+, quality hardening | `12-quality-gates-and-makefile.md`, `10-test-driven-delivery-and-verification.md`, M1+ closeout evidence, `quality/` policy/checker tests |
| M2, local protocol/client/daemon | `03-daemon-transport-and-adapters.md`, `02-dto-and-contract-policy.md`, M2 closeout evidence, transport/client/daemon/TUI contracts |
| M3, SQLite sessions/events/recovery | `04-sessions-runs-events-and-storage.md`, `09-configuration-security-and-observability.md`, `03-daemon-transport-and-adapters.md`, M3 closeout evidence |
| M4, model/provider/one run | `08-model-protocol-and-providers.md`, `04-sessions-runs-events-and-storage.md`, `09-configuration-security-and-observability.md`, M4 closeout evidence |
| M5, tools/WorkspaceRoot/hooks | `05-tools-workspace-and-hooks.md`, `04-sessions-runs-events-and-storage.md`, `07-plan-and-build-modes.md`, M5 closeout evidence |
| M5+, post-M5 retrospective alignment, the Slice 3/4 tails, and the Slice 5 instruction channel | `11-implementation-roadmap.md` (Milestone 5+), ADR 0035-0043, `30-instruction-sources-and-system-context.md`, `reconciliation/source-of-truth-matrix.md`, `reconciliation/evidence-register.md` |
| M6, Tauri bridge and primary desktop UI | `03-daemon-transport-and-adapters.md`, `01-workspace-and-crate-map.md`, `02-dto-and-contract-policy.md`, `24-activity-ui-and-adapters.md` |
| M7, Plan/Build policies, physical plans, and Build Autopilot | `07-plan-and-build-modes.md`, `05-tools-workspace-and-hooks.md`, `04-sessions-runs-events-and-storage.md`, `30-instruction-sources-and-system-context.md` (the `Mode` contribution) |
| M8, VFR and Headroom | `06-vfr-and-headroom.md`, `05-tools-workspace-and-hooks.md`, `08-model-protocol-and-providers.md`, `30-instruction-sources-and-system-context.md` (the `Vfr` contribution) |
| M9, hardening/acceptance | all architecture documents relevant to delivered behavior, all closeout evidence, and the complete result-oriented scenario matrix in `10-test-driven-delivery-and-verification.md` |
| M10, Mandate core — authority, admission, and scheduling | `13-mandate-domain-and-durable-lifecycle.md`, `14-run-execution-meaning-and-historical-compatibility.md`, `16-mandate-scheduler-and-readiness-driven-admission.md`, ADR 0001-0003/0006/0008, `reconciliation/contradiction-register.md` |
| M11, Mandate execution plane — tool loop, delegation, and bridge | `15-tool-registry-and-mandate-tool-loop.md`, `17-mandate-child-graph-and-delegated-verifier-authority.md`, `19-mandate-gateway-rlm-bridge.md`, ADR 0004/0007/0009/0011/0025 and the child/bridge directions of ADR 0027 |
| M12, Capabilities and context — MCP, kernel, and Skills/context | `18-mandate-mcp-capability-lifecycle.md`, `20-ipython-kernel-lifecycle.md`, `21-goals-skills-context-memory-and-compaction.md`, ADR 0010/0012/0013/0042 and the MCP/kernel directions of ADR 0027 |

## Source verification rules

Before a specification names a path or required action:

1. Confirm the file/crate/test/policy currently exists.
2. Read its surrounding definitions and consumers, not only a search hit.
3. Check whether closeout evidence has superseded, accepted, deferred, or corrected the roadmap language.
4. Keep claims bounded to evidence. If proof is absent, record an implementation-required decision, blocker, or test requirement rather than inventing a completion claim.
5. Link to the source of truth instead of copying changing architecture policy into a planning artifact.
