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
| EVD-015 | Goal domain and verification (M5+) | Goal scope/tree/lifecycle/readiness, leading-goal selection, verifier authority/matrix, gates, memory/roles/templates, proposals, compaction, bounds, and M3/M4 preservation fixtures | Planned | `architecture/28-goal-domain-and-verification.md#Dependencies-and-non-goals` | Architecture 28 |
| EVD-016 | Provider session selection and profiles protocol (M5+) | Session default/override, promotion/reconciliation, usage, `provider_profiles_v1`, pending-removal/degraded, held-run admission, and M3/M4 preservation fixtures | Planned | `architecture/29-provider-session-and-profiles-protocol.md#Dependencies-and-non-goals` | Architecture 29 |
| EVD-017 | Provider reasoning and catalog detail (M5+) | Reasoning history/usage/paged delivery, dialect catalog, catalog limits/tombstones/audit, legacy M4 bridge, and M3/M4 preservation fixtures | Planned | `architecture/22-provider-evolution-profiles-and-reasoning.md#Dependencies-non-goals-and-evidence` | Architecture 22 |
| EVD-018 | Child/kernel/bridge/MCP detail (M5+) | `sub_agent` classes/bounds/clarification, kernel limits/checkpoints, bridge limits/grants, MCP bounded gateway, closed safe failures, and M3/M4 preservation fixtures | Planned | `architecture/17-mandate-child-graph-and-delegated-verifier-authority.md`, `architecture/20-ipython-kernel-lifecycle.md`, `architecture/19-mandate-gateway-rlm-bridge.md`, `architecture/18-mandate-mcp-capability-lifecycle.md` | Architectures 17/20/19/18 |
| EVD-019 | Goals/Skills/context/memory/compaction (arch 21) | Goal/Skill selection, manifest/projection, memory/compaction, no-current-state reconstruction, and M3/M4 preservation fixtures | Planned | `architecture/21-goals-skills-context-memory-and-compaction.md#Dependencies-non-goals-and-evidence` | Architecture 21 |
| EVD-020 | Session branching (arch 23) | Canonical v1/v2 goldens, boundary eligibility, transaction fault injection, idempotency/preview races, migration byte preservation, tree paging, and M3/M4 preservation fixtures | Planned | `architecture/23-non-destructive-session-branching-and-regeneration.md#Required-evidence-before-implementation` | Architecture 23 |
| EVD-021 | Activity and notification detail (M5+) | Canonical identity/message/journal/notification vectors, direct-pair/order/clarification failures, atomic fault injection, urgent dedup, archive terminality, closed safe failures, and M3/M4 preservation fixtures | Planned | `architecture/24-activity-ui-and-adapters.md#Required-evidence-before-implementation` | Architecture 24 |
| EVD-022 | Mandate DTO family and attempt evidence (M5+) | Mandate DTO/revision/disposition/limit-class/capacity-outcome and shared attempt-evidence DTO fixtures, classification matrix, and M3/M4 preservation fixtures | Planned | `architecture/13-mandate-domain-and-durable-lifecycle.md#Required-evidence-before-implementation` | Architecture 13 |
| EVD-023 | Continual-harness safe failures and selection record (M5+) | Closed `harness_*` failure fixtures, selection-record goldens, and M3/M4 preservation fixtures | Planned | `architecture/26-continual-harness.md#Selection-record-and-closed-safe-failures` | Architecture 26 |
| EVD-024 | Reasoning normalized-stream/capability-slice/Responses detail (M5+) | `provider_reasoning_stream_invalid` and `reasoning_output_limit_exceeded` fixtures, DTO mapping goldens, effort/mode and resolved-policy fixtures, automatic-summary fixtures, and M3/M4 preservation fixtures | Planned | `architecture/22-provider-evolution-profiles-and-reasoning.md#Reasoning-capability-slice-and-bounded-responses-v1` | Architecture 22 |
| EVD-025 | Activity child-operations and model-exchange detail (M5+) | `RlmMessageExchangeDto`/`RlmChildMessageOperation` binding, journal-order delivery, terminal-recipient rejection, and M3/M4 preservation fixtures | Planned | `architecture/24-activity-ui-and-adapters.md#Child-operations-delivery-and-model-exchanges` | Architecture 24 |
| EVD-026 | Durable-fact and historical-version rules (M5+) | No-new-sequence, hook-before-transaction, publication-gate, crate version-ownership, Skill-decoder, and `fork-model-context-v1` trilemma fixtures | Planned | `architecture/04-sessions-runs-events-and-storage.md#Common-durable-fact-rules`; `architecture/14-run-execution-meaning-and-historical-compatibility.md#Common-historical-version-policy` | Architectures 04/14/23 |
| EVD-027 | Autonomous continuation direction (M5+) | Build-mode Mandate default continuation fixtures, known-outcome classification, and M3/M4 preservation fixtures | Planned | `architecture/13-mandate-domain-and-durable-lifecycle.md#Autonomous-continuation` | Architecture 13 |
| EVD-028 | Accepted deferred directions: activity metadata, content inspection, per-call cancellation (M5+) | Tree-level-metadata, semantic-content-inspection, and per-call-cancellation boundary fixtures; no-authority and M3/M4 preservation fixtures | Planned | `architecture/24-activity-ui-and-adapters.md#Child-operations-delivery-and-model-exchanges`; `architecture/22-provider-evolution-profiles-and-reasoning.md`; `architecture/19-mandate-gateway-rlm-bridge.md` | Architectures 24/22/19 |
| EVD-029 | Tool-loop detail: descriptor schema availability, group-invalid shapes, no step limit, typed references, combined gate (M5+) | `model_schema_availability`, `provider_tool_group_invalid` shape matrix, no-numeric-step-limit, typed-reference, and combined publication-gate fixtures | Planned | `architecture/15-tool-registry-and-mandate-tool-loop.md#Required-evidence-before-implementation` | Architecture 15 |
| EVD-030 | Bridge slow-peer non-delay (M5+) | Slow/resync/detached-peer non-blocking and typed-resync fixtures | Planned | `architecture/19-mandate-gateway-rlm-bridge.md#Bridge-detail-DTOs-limits-and-safe-failures` | Architecture 19 |
| EVD-031 | Kernel disposal, diagnostics, and background capture (M5+) | Idle-disposal, no-formatted-footer, diagnostics-content, and background-capture fixtures | Planned | `architecture/20-ipython-kernel-lifecycle.md#Kernel-detail-DTO-family-bounds-and-safe-failures` | Architecture 20 |
| EVD-032 | Child usage no-token-ceiling and MCP name supersession (M5+) | No-token-ceiling aggregation and MCP V1-record supersession fixtures | Planned | `architecture/17-mandate-child-graph-and-delegated-verifier-authority.md`; `architecture/18-mandate-mcp-capability-lifecycle.md` | Architectures 17/18 |

## Interpretation rule

No `Planned` row satisfies implementation authorization, release readiness,
coverage, or `make verify` evidence. An activating specification must replace or
supplement each applicable planned row with an exact artifact and observed result.
