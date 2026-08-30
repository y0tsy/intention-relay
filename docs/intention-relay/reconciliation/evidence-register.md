# Accepted Autopilot direction

The product decision and activation scope are recorded in ADR 0017 and ADR
0018. Implementation evidence remains pending until the separately activated
slice declares crate ownership, DTO/wire/storage versions, feature profiles,
coverage targets, tests, and observed `make verify` results.

# Evidence Register

## Status and authority

This register is a coordination index, not a normative architecture source.
`Verified` means an existing artifact or command result is cited. `Planned`
means an obligation for a later implementation package and is not acceptance
evidence. `Deferred`, `Not applicable`, and `Blocked` are explicit non-verified
states.

## Current evidence status

| Evidence ID | Topic/package | Required artifact or outcome | Status | Exact anchor/reference | Owner |
| --- | --- | --- | --- | --- | --- |
| EVD-001 | Foundation | Architecture ownership and precedence review | Verified | `architecture/README.md#Post-M4-authority-foundation`; `reconciliation/README.md#Authority-and-precedence` | Foundation |
| EVD-002 | Foundation | Identity, UUID, digest and sequence matrix | Verified | `architecture/README.md#Cross-domain-identity-and-sequence-summary`; `architecture/02-dto-and-contract-policy.md#UUID-roles-and-non-conversion` | Foundation |
| EVD-003 | Execution meaning | Canonical bytes, digest and negative decoder fixtures | Planned | `architecture/14-run-execution-meaning-and-historical-compatibility.md#Required-evidence-before-implementation` | Architecture 14 |
| EVD-004 | Mandate lifecycle | Transition, race, recovery and uncertainty outcomes | Planned | `architecture/13-mandate-domain-and-durable-lifecycle.md#Required-evidence-before-implementation` | Architecture 13 |
| EVD-005 | Scheduler | Readiness retention, ordering and admission handoff | Planned | `architecture/16-mandate-scheduler-and-readiness-driven-admission.md#Required-evidence-before-implementation` | Architecture 16 |
| EVD-006 | Tools and workspace | Ordinary containment versus Mandate observation | Planned | `architecture/05-tools-workspace-and-hooks.md#Execution-kind-scope`; `architecture/15-tool-registry-and-mandate-tool-loop.md` | Architectures 05/15 |
| EVD-007 | Child/verifier | Stale baseline and authority race fixtures | Planned | `architecture/17-mandate-child-graph-and-delegated-verifier-authority.md#Required-evidence-before-implementation` | Architecture 17 |
| EVD-008 | Kernel | Run-exclusive epoch and no namespace reuse | Planned | `architecture/20-ipython-kernel-lifecycle.md#Epoch-isolation-invariant` | Architecture 20 |
| EVD-009 | MCP/bridge | Dynamic acquisition, grant, replay and no-bypass fixtures | Planned | `architecture/18-mandate-mcp-capability-lifecycle.md`; `architecture/19-mandate-gateway-rlm-bridge.md` | Architectures 18/19 |
| EVD-010 | Activity/UI | Compatibility projection, notification and adapter parity fixtures | Planned | `architecture/24-activity-ui-and-adapters.md#Historical-projections-and-limits` | Architecture 24 |
| EVD-011 | Documentation quality | Full Markdown/link/status/claim inventory checks | Planned | Future documentation validation command and CI artifact | Reconciliation |
| EVD-012 | Configuration/provider control plane (M5+) | Reload fault injection, rotation redaction, health/discovery non-authority, pricing classification, control-plane safe-projection, and M3/M4 preservation fixtures | Planned | `architecture/25-configuration-provider-control-plane.md#Dependencies-and-non-goals` | Architecture 25 |
| EVD-013 | Continual harness (M5+) | Rule lifecycle, trigger/coalescing/catch-up, schedule/time, dossier/checkpoint/conclusion bounds, corridor admission, limit/concurrency, cancellation/recovery, and M3/M4 preservation fixtures | Planned | `architecture/26-continual-harness.md#Dependencies-and-non-goals` | Architecture 26 |
| EVD-014 | Programmatic-caller policy (M5+) | Root-origin/provenance, policy scope/narrowing, decision/corridor, lifecycle/draft, reservation/calendar, run-selection compatibility, closed safe-failure, and M3/M4 preservation fixtures | Planned | `architecture/27-programmatic-caller-policy-and-admission.md#Dependencies-and-non-goals` | Architecture 27 |

## Interpretation rule

No `Planned` row satisfies implementation authorization, release readiness,
coverage, or `make verify` evidence. An activating specification must replace or
supplement each applicable planned row with an exact artifact and observed result.
