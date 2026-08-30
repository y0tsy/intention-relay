# Architecture Decision Records

## Status

Decision records capture cross-document architectural decisions that require
stable rationale, compatibility treatment, failure behavior, and later evidence.
They supplement, but do not replace, the authoritative architecture documents.

## Lifecycle

A record is `Proposed`, `Accepted`, `Superseded`, or `Deprecated`. Accepted
records are normative only through their linked authoritative architecture
sections. Every record includes scope, decision, invariants, compatibility,
security/failure behavior, non-goals, affected documents, evidence, and research
provenance.

## Index

| ID | Status | Topic |
| --- | --- | --- |
| [0001](0001-mandate-authority-and-fresh-run-lifecycle.md) | Accepted | Mandate authority and fresh-run lifecycle |
| [0002](0002-external-attempt-evidence-and-unknown-effect-reconciliation.md) | Accepted | External attempt evidence and unknown-effect reconciliation |
| [0003](0003-run-execution-meaning-and-historical-compatibility.md) | Accepted | Execution meaning and historical compatibility |
| [0004](0004-rust-owned-capability-plane-and-fixed-tool-registry.md) | Accepted | One Rust-owned capability path and fixed registry boundary |
| [0005](0005-m4plus-authority-reconciliation-and-delivery-boundaries.md) | Accepted | M4+ reconciliation and delivery boundaries |
| [0006](0006-mandate-lifecycle-and-admission-boundary.md) | Accepted | Mandate lifecycle and admission boundary |
| [0007](0007-unified-tool-registry-and-direct-mandate-tool-admission.md) | Accepted | Unified tool registry and direct Mandate tool admission |
| [0008](0008-durable-mandate-scheduler-and-readiness-driven-admission.md) | Accepted | Durable Mandate scheduler and readiness-driven admission |
| [0009](0009-mandate-child-graph-and-delegated-verifier-authority.md) | Accepted | Mandate child graph and delegated verifier authority |
| [0010](0010-mandate-mcp-capability-lifecycle.md) | Accepted | Mandate MCP capability lifecycle |
| [0011](0011-mandate-gateway-rlm-bridge.md) | Accepted | Mandate Gateway/RLM bridge |
| [0012](0012-ipython-kernel-lifecycle.md) | Accepted | Run-scoped IPython kernel lifecycle |
| [0013](0013-goals-skills-context-memory-and-compaction.md) | Accepted | Goals, Skills, context, memory, and compaction |
| [0014](0014-provider-evolution-profiles-and-reasoning.md) | Accepted | Provider evolution, profiles, and reasoning |
| [0015](0015-non-destructive-session-branching-and-regeneration.md) | Accepted | Non-destructive session branching and regeneration |
| [0016](0016-activity-ui-and-adapters.md) | Accepted | Activity, UI, and adapters |
| [0017](0017-build-autopilot-and-plan-focus-continuity.md) | Accepted | Build Autopilot and Plan focus continuity |
| [0018](0018-plan-build-autopilot-activation-scope.md) | Accepted | Plan/Build Autopilot activation scope |
| [0019](0019-production-model-tool-loop.md) | Accepted | Production model-tool loop |
| [0020](0020-configuration-provider-control-plane-directions.md) | Accepted | Post-M5 configuration and provider control-plane directions |
| [0021](0021-continual-harness-directions.md) | Accepted | Post-M5 continual-harness directions |
| [0022](0022-programmatic-caller-policy-directions.md) | Accepted | Post-M5 programmatic-caller policy directions |
| [0023](0023-goal-domain-and-verification-directions.md) | Accepted | Post-M5 Goal domain and verification directions |
| [0024](0024-provider-session-and-profiles-protocol-directions.md) | Accepted | Post-M5 provider session selection and profiles protocol directions |
| [0025](0025-base-tool-contracts-and-tool-loop-bounds.md) | Accepted | Post-M5 base-tool contracts and tool-loop bounds |
| [0026](0026-session-branching-detail-directions.md) | Accepted | Post-M5 session-branching detail directions |
| [0027](0027-child-kernel-bridge-mcp-detail-directions.md) | Accepted | Post-M5 child, kernel, bridge, and MCP detail directions |
| [0028](0028-provider-reasoning-and-catalog-detail-directions.md) | Accepted | Post-M5 provider reasoning and catalog detail directions |
| [0029](0029-activity-and-notification-detail-directions.md) | Accepted | Post-M5 activity and notification detail directions |
| [0030](0030-continual-harness-safe-failures-and-selection-record-detail.md) | Accepted | Post-M5 continual-harness closed safe failures and selection-record detail |
| [0031](0031-autonomous-continuation-direction.md) | Accepted | Post-M5 autonomous continuation direction |
| [0032](0032-accepted-deferred-directions-activity-metadata-content-inspection-per-call-cancellation.md) | Accepted | Post-M5 accepted deferred directions: activity-tree metadata, semantic content inspection, and per-call cancellation |
| [0033](0033-accepted-m5plus-execution-directions.md) | Accepted | Post-M5 accepted execution directions: control-plane editing, provider-native controls, fork execution, harness autonomy, and RLM packaging |
| [0034](0034-accepted-m5plus-retained-deferral-directions.md) | Accepted | Post-M5 accepted retained-deferral directions: kernel output projection, retention policy, supervision topology, calendar semantics, and activity limit classification |
