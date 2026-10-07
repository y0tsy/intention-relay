# 0004: M5+ Complete Foundation Activation

## Status

Accepted 2026-08-30.

Amended 2026-09-26 by [ADR 0010](0010-instruction-sources-and-system-context.md): the pre-approved slice sequence gains
a fifth activating slice, instruction sources and system context, without changing the slice order.

Amended 2026-09-30 by [ADR 0011](0011-local-json-rpc-2-0-transport.md), [ADR 0012](0012-typed-serde-json-contracts.md),
and [ADR 0014](0014-limits-by-precedent-and-no-content-scanning.md): the Slice 1 canonical-codec and capability-family
items, the queue-audit and corridor wording, and the numeric contract limits are
reconciled in the slice list below without changing the slice order (fifth slice: ADR 0010; activation sequence:
ADR 0004).

Amended 2026-10-07: the pre-approved slice sequence gains an inserted slice 1.5, core simplification, between contracts
and versions and the control plane, without changing the order of slices 1-5.

## Decision

Milestone 5+ is the single activation home for the full post-M5 stack (architectures 25 and 28-29, and
all detail packages) and the hard prerequisite of Milestones 6-9 in the roadmap dependency graph (`K[M5+] -->
F/G/H/I`).

The milestone is delivered as a pre-approved sequence of activating slices, all approved together as one package:

- **Slice 1 — Contracts and versions.** The versioned protocol, schema, and DTO
contract ledger for the full post-M5 stack over JSON-RPC 2.0 and typed serde JSON ([ADR
0011](0011-local-json-rpc-2-0-transport.md), [ADR 0012](0012-typed-serde-json-contracts.md)), the single live
configuration and storage schemas with M3/M4 byte preservation, and crate ownership, feature-profile, and coverage-tier
declarations for every activated family. The former execution-meaning record, capability families, canonical tags, and
digests are removed by those records.
- **Slice 1.5 — Core simplification.** The workspace collapses to ten production crates: `intention-proto`,
`intention-domain`, `intention-config`, `intention-engine`, `intention-tools`, `intention-providers`,
`intention-storage`, `intention-transport`, `intention-daemon`, and `intention-client`; the composition facade is
removed and the daemon host calls the engine directly; DTOs remain only at the IPC wire, SQLite, and provider-SDK
boundaries; tool inputs and
outputs are schema-validated JSON and the only JSON payloads in the system; identity newtypes reduce to nine; durable
events use one `EventPayload`; the scoped durable reread proof is removed and publication follows the durable commit;
the eight hook phases stay; the single live protocol version is renumbered to 1.0; and `intention-client` becomes a
fully asynchronous client implementing every protocol command and query, with daemon end-to-end tests driving it
directly. Architecture 01 owns the crate map, architecture 02 the boundary rules, and architecture 03 the daemon,
transport, and client consequences.
- **Slice 2 — Control plane.** The control-plane cluster and provider session selection
(architectures 25/29/22): controlled live reload, credential rotation, provider health checks, model discovery, pricing
policy, profile UI and raw-TOML/configuration editing, arbitrary authentication headers, session defaults and
per-turn/fork overrides, pending-removal and degraded recovery, and the provider reasoning/catalog surface. Slice 2
is not activated and requires an activating specification; the queue-audit, queue-promotion,
reconciliation, and held-run admission wording is not part of the direction ([ADR
0014](0014-limits-by-precedent-and-no-content-scanning.md)). The unconsumed-surface audit (2026-09) removed the
unconsumed provider-native preservation-control and server-side-parser contracts, the Responses reasoning-mode
projection, the reasoning-usage DTOs, and the model-capability envelope.
- **Slice 3 — Goal domain.** Goal domain (architecture 28): the Goal tree.
- **Slice 4 — UI foundation.** Session branching, reasoning and catalog delivery, and adapter boundaries
(architectures 23/22 plus architecture 03's flat activity journal and notification list):
`session_fork_v1`, normalized reasoning delivery, and the exact typed client/protocol surface that M6 consumes.
- **Slice 5 — Instruction sources and system context.** The instruction channel added
by [ADR 0010](0010-instruction-sources-and-system-context.md) and owned by architecture 30 (instruction sources and
system context): the closed instruction source kinds and scopes, the deployment instruction profile adapted from the
legacy Antibusy static prompt set, user-editable fragments, workspace `AGENTS.md` project instructions, the reserved
`Mode` and `Vfr` contributions, deterministic assembly, the immutable effective instruction projection with its revision
identity, closed `instruction_*` safe failures, and identity-only observability. Numeric bounds and digest wording are
settled at activation under ADR 0014 and ADR 0012. Its activating specification declares the instruction contract
families under the Slice 1 ledger policy without changing the slice order.

Each direction remains bound to its slice; every retrospective change to M0-M5 code required by these
directions is activated inside its slice with its own contract, transaction, and outcome test.

## Rationale

Every post-M5 package closes with "activation remains excluded pending a later M5+ specification", and the packages
declare DTO, wire, storage, and quality values that M6-M9 consume. Without a single activation home that fixes the
shared contract ledger first, later milestones would each change contracts retroactively. The hard prerequisite and the
pre-approved slice sequence guarantee that M6-M9 start against stable contracts and need no retroactive protocol, DTO,
schema, crate-boundary, migration, or quality-policy changes.

## Normative invariants

1. M5+ is the hard prerequisite of M6, M7, M8, and M9; the dependency graph
carries `K[M5+] --> F/G/H/I` and no "parallel/non-blocking" wording.
2. M5+ activates only preparatory foundation work; it never implements M6-M9
milestone behavior (Tauri bridge/desktop UI, Plan/Build, VFR/Headroom, or M9 acceptance closure).
3. Contracts and versions precede core simplification, and core simplification precedes the control plane and UI
foundation; later slices consume only contracts activated by earlier slices.
4. No slice ships half-ready: every activated contract ships with its version,
owner, tests, policy mapping, migration behavior, and evidence together.
5. No downstream milestone requires a retroactive contract change after M5+
exit.
6. M3/M4 startup-only configuration, recorded revisions, persisted snapshots,
sessions, runs, events, and bytes remain authoritative and unchanged; no synthetic post-M5 record is
added to historical runs.
7. M5+ introduces no second runtime, registry, scheduler, persistence
authority, or sandbox.
8. No direction is implemented by this decision; each
remains non-authorizing until its slice activation.

## Failure semantics

- Each slice fails closed before effect when its contract is unsupported or
inconsistent; no partial contract or partial projection is delivered.
- Recovery never resumes, retries, reattaches, or reruns work under any slice.
- A slice that cannot be completed atomically (contracts, tests, policy, and
evidence together) is not accepted; the milestone remains on the prior accepted slice.

## Compatibility and non-goals

This decision extends and operationalizes the accepted post-M5 directions owned by the architecture documents: their
domain decisions remain authoritative, and
this decision supersedes only conflicting activation and dependency wording (the "may run in parallel with M6-M9" and
"does not block their activation" clauses, and the per-direction activation wording replaced by the pre-approved slice
sequence). The closed M4 baseline, M3/M4 bytes, and existing behavior remain unchanged.

No slice is implemented by this decision, and no crate, schema, migration, protocol, feature, or quality-policy target
is activated. The slices remain non-authorizing until their own activating specifications are accepted, and M5-M9 are
not renumbered.

Owner: architecture 11. Evidence: each slice's activating specification (coverage tiers, fixtures, and outcome evidence
included) per [architecture 12](../architecture/12-quality-gates-and-makefile.md). The M5+ exit evidence must prove that
M6, M7, M8, and M9 can each begin against stable contracts without any retroactive contract change.
