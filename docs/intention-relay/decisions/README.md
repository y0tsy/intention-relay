# Architecture Decision Records

Decision records capture cross-document architectural decisions that require stable rationale, compatibility treatment,
failure behavior, and later evidence; they supplement, but do not replace, the authoritative architecture documents. A
record is `Proposed`, `Accepted`, `Superseded`, or `Deprecated`; accepted records are normative only through their
linked architecture sections.

## Index

| ID | Status | Topic |
| --- | --- | --- |
| [0001](0001-mandate-authority-and-fresh-run-lifecycle.md) | Accepted | Mandate authority and fresh-run lifecycle |
| [0002](0002-external-attempt-evidence-and-unknown-effect-reconciliation.md) | Superseded | External attempt evidence and unknown-effect reconciliation |
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
| [0022](0022-programmatic-caller-policy-directions.md) | Accepted | Post-M5 programmatic-caller policy directions |
| [0023](0023-goal-domain-and-verification-directions.md) | Accepted | Post-M5 Goal domain and verification directions |
| [0024](0024-provider-session-and-profiles-protocol-directions.md) | Accepted | Post-M5 provider session selection and profiles protocol directions |
| [0025](0025-base-tool-contracts-and-tool-loop-bounds.md) | Accepted | Post-M5 base-tool contracts and tool-loop bounds |
| [0026](0026-session-branching-detail-directions.md) | Accepted | Post-M5 session-branching detail directions |
| [0027](0027-child-kernel-bridge-mcp-detail-directions.md) | Accepted | Post-M5 child, kernel, bridge, and MCP detail directions |
| [0028](0028-provider-reasoning-and-catalog-detail-directions.md) | Accepted | Post-M5 provider reasoning and catalog detail directions |
| [0029](0029-activity-and-notification-detail-directions.md) | Accepted | Post-M5 activity and notification detail directions |
| [0031](0031-autonomous-continuation-direction.md) | Accepted | Post-M5 autonomous continuation direction |
| [0032](0032-accepted-deferred-directions-activity-metadata-content-inspection-per-call-cancellation.md) | Accepted | Post-M5 accepted deferred directions: activity-tree metadata, semantic content inspection, and per-call cancellation |
| [0033](0033-accepted-m5plus-execution-directions.md) | Accepted | Post-M5 accepted execution directions: control-plane editing, provider-native controls, fork execution, and RLM packaging |
| [0034](0034-accepted-m5plus-retained-deferral-directions.md) | Accepted | Post-M5 accepted retained-deferral directions: kernel output projection, retention policy, supervision topology, calendar semantics, and activity limit classification |
| [0035](0035-m5plus-complete-foundation-activation.md) | Accepted | M5+ complete foundation activation: hard prerequisite for M6-M9, pre-approved five-slice activation sequence (the fifth slice is added by ADR 0043) |
| [0038](0038-no-backward-compatibility-and-legacy-removal.md) | Accepted | No backward compatibility and legacy removal (single-version policy) |
| [0039](0039-request-side-tool-advertisement.md) | Accepted | Request-side tool advertisement (ordinary production tool loop) |
| [0040](0040-opt-in-live-provider-e2e.md) | Accepted | Opt-in live-provider end-to-end channel (manual, non-blocking) |
| [0041](0041-same-run-reasoning-round-trip.md) | Accepted | Same-run provider reasoning round-trip (ordinary production tool loop) |
| [0042](0042-project-script-library-for-kernel-cells.md) | Accepted | Project script library for kernel cells (`.ir/scripts` persistence of agent-authored kernel modules) |
| [0043](0043-instruction-sources-and-system-context.md) | Accepted | Instruction sources and system context (deployment instruction profile, workspace `AGENTS.md`, user-editable fragments, `Mode`/`Vfr` contributions, immutable effective instruction projection) |
| [0045](0045-local-json-rpc-2-0-transport.md) | Accepted | Local JSON-RPC 2.0 transport (NDJSON framing, protocol 2.0 exact-match handshake, no capability plane, no connection roles) |
| [0046](0046-typed-serde-json-contracts.md) | Accepted | Typed serde JSON contracts (binary canonical codec, tag registry, digests, and contract families removed; RFC 8785 policy deferred to the first real consumer) |
| [0047](0047-workspace-root-addressing-anchor.md) | Accepted | WorkspaceRoot as an addressing anchor (join, child-process cwd, default glob/grep scope; not a security boundary) |
| [0048](0048-limits-by-precedent-and-no-content-scanning.md) | Accepted | Limits by precedent, no runtime content scanning, and removal of the corridor, calendar-period, and queue-audit engines |
| [0049](0049-base-coverage-threshold.md) | Accepted | Base 80% line-coverage threshold and designated-files mechanism (replaces the A/B/C coverage tiers) |
| [0050](0050-ordering-authorities.md) | Accepted | Ordering authorities (session event sequence and container journal sequence, plus the observation cursor as a non-authoritative resume position) |
| [0052](0052-partial-tool-results-for-interrupted-execution.md) | Accepted | Partial tool results for interrupted execution (bounded captured output, interruption notices, and no Mandate pause) |
| [0054](0054-dynamic-context-window-and-prompt-caching.md) | Accepted | Dynamic context window and prompt-cache breakpoints (provider context-window policy, calibrated token accounting, largest-first tool-result compression, capacity stub, and ephemeral cache markers) |
