# Architecture Decision Records

Decision records capture cross-document architectural decisions that require stable rationale, compatibility treatment,
failure behavior, and later evidence; they supplement, but do not replace, the authoritative architecture documents. A
record is `Proposed`, `Accepted`, `Superseded`, or `Deprecated`; accepted records are normative only through their
linked architecture sections.

## Index

| ID | Status | Topic |
| --- | --- | --- |
| [0001](0001-rust-owned-capability-plane-and-fixed-tool-registry.md) | Accepted | One Rust-owned capability path, fixed tool registry, and the unified registry invariants |
| [0002](0002-build-autopilot-and-plan-focus-continuity.md) | Accepted | Build Autopilot and Plan focus continuity, including its activation scope |
| [0003](0003-production-model-tool-loop.md) | Accepted | Production model-tool loop |
| [0004](0004-m5plus-complete-foundation-activation.md) | Accepted | M5+ complete foundation activation: hard prerequisite for M6-M9, five-slice activation sequence |
| [0005](0005-no-backward-compatibility-and-legacy-removal.md) | Accepted | No backward compatibility and legacy removal (single-version policy) |
| [0006](0006-request-side-tool-advertisement.md) | Accepted | Request-side tool advertisement (ordinary production tool loop) |
| [0007](0007-opt-in-live-provider-e2e.md) | Accepted | Opt-in live-provider end-to-end channel (manual, non-blocking) |
| [0008](0008-same-run-reasoning-round-trip.md) | Accepted | Same-run provider reasoning round-trip (ordinary production tool loop) |
| [0009](0009-project-script-library-for-kernel-cells.md) | Accepted | Project script library for kernel cells (`.ir/scripts` persistence of agent-authored kernel modules) |
| [0010](0010-instruction-sources-and-system-context.md) | Accepted | Instruction sources and system context (deployment instruction profile, workspace `AGENTS.md`, user-editable fragments, `Mode`/`Vfr` contributions, immutable effective projection) |
| [0011](0011-local-json-rpc-2-0-transport.md) | Accepted | Local JSON-RPC 2.0 transport (NDJSON framing, protocol 2.0 exact-match handshake, no capability plane, no connection roles) |
| [0012](0012-typed-serde-json-contracts.md) | Accepted | Typed serde JSON contracts (binary canonical codec, tag registry, digests, and contract families removed; RFC 8785 policy deferred to the first real consumer) |
| [0013](0013-workspace-root-addressing-anchor.md) | Accepted | WorkspaceRoot as an addressing anchor (join, child-process cwd, default glob/grep scope; not a security boundary) |
| [0014](0014-limits-by-precedent-and-no-content-scanning.md) | Accepted | Limits by precedent, no runtime content scanning, and every numeric cap removed from the future child, bridge, activity, fork, kernel, and progress surfaces |
| [0015](0015-ordering-authorities.md) | Accepted | Ordering authorities (session event sequence and container journal sequence, plus the observation cursor as a non-authoritative resume position) |
| [0016](0016-per-crate-coverage-tiers.md) | Accepted | Per-crate coverage tiers (`core` 75%, `standard` 60%, `edge` 20%, `exempt` 0%; replaces the base 80% threshold and designated-files mechanism) |
| [0017](0017-partial-tool-results-for-interrupted-execution.md) | Accepted | Partial tool results for interrupted execution (bounded captured output, interruption notices, and no pause) |
| [0018](0018-dynamic-context-window-and-prompt-caching.md) | Accepted | Dynamic context window and prompt-cache breakpoints (provider context-window policy, calibrated token accounting, largest-first tool-result compression, capacity stub, and ephemeral cache markers) |
| [0019](0019-pending-turns-and-cooperative-interruption.md) | Accepted | Pending turns and cooperative interruption (durable pending input joined to the live run context, `run.interrupt`, no `Cancelling`/`Cancelled` run status, and cooperative tool interruption with partial results) |
