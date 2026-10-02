# ADR 0035: M5+ Complete Foundation Activation

## Status

Accepted 2026-08-30. This decision re-edits
[Milestone 5+](../architecture/11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment)
from a documentation-approved future milestone into the activation home for
the complete post-M5 package stack and the declared hard prerequisite of
Milestones 6-9. It records the activation shape of the milestone: one
pre-approved sequence of activating slices, approved together as one package,
that fixes every contract, version, ownership, quality, and evidence
requirement the later milestones consume. It does not activate implementation:
no crate, schema, migration, protocol, feature profile, or quality-policy
target is activated by this decision.

Amended 2026-09-26 by [ADR 0043](0043-instruction-sources-and-system-context.md):
the pre-approved slice sequence gains a fifth activating slice, instruction
sources and system context, without changing slices 1-4.

Amended 2026-09-30 by [ADR 0044](0044-revert-of-m5plus-slice2-control-plane.md),
[ADR 0045](0045-local-json-rpc-2-0-transport.md),
[ADR 0046](0046-typed-serde-json-contracts.md), and
[ADR 0048](0048-limits-by-precedent-and-no-content-scanning.md): the Slice 1
canonical-codec and capability-family items, the Slice 2 control-plane
reversion, the queue-audit and corridor wording, and the numeric contract
limits are reconciled in the slice list below without changing the five-slice
order.

## Decision

Milestone 5+ is the single activation home for the full post-M5 stack
(architectures 25-29, ADR 0021-0034, and all detail packages) and the hard
prerequisite of Milestones 6-9 in the roadmap dependency graph
(`K[M5+] --> F/G/H/I`). The post-M4 Mandate packages (architectures 13-21) are
delivered by the separate Mandate-track milestones 10-12, which consume this
milestone's foundation and whose contract records the five slices below name;
the Mandate track does not extend the slice sequence or renumber M6-M9. The
milestone is delivered as a pre-approved sequence
of activating slices, all approved together as one package:

1. **Contracts and versions** — the versioned protocol/schema/DTO contract
   ledger for the full post-M5 stack over JSON-RPC 2.0 and typed serde JSON
   ([ADR 0045](0045-local-json-rpc-2-0-transport.md),
   [ADR 0046](0046-typed-serde-json-contracts.md)), the single live
   configuration and storage schemas with M3/M4 byte preservation, and crate
   ownership, feature-profile, and coverage-tier declarations for every
   activated family. The former execution-meaning record, capability families,
   canonical tags, and digests are removed by those records.
2. **Control plane** — the ADR 0020 cluster and provider session selection
   (architectures 25/29/22): controlled live reload, credential rotation,
   provider health checks, model discovery, pricing policy, profile UI and
   raw-TOML/configuration editing, arbitrary authentication headers, session
   defaults and per-turn/fork overrides, pending-removal and degraded
   recovery, and the provider reasoning/catalog surface. The Slice 2
   activation is reverted by
   [ADR 0044](0044-revert-of-m5plus-slice2-control-plane.md) and can return
   only through a new activating specification; the queue promotion,
   reconciliation, and held-run admission wording is removed by
   [ADR 0048](0048-limits-by-precedent-and-no-content-scanning.md). The
   unconsumed-surface audit (2026-09) removed the unconsumed provider-native
   preservation-control and server-side-parser contracts, the Responses
   reasoning-mode projection, the reasoning-usage DTOs, and the
   model-capability envelope.
3. **Harness** — continual harness, programmatic-caller policy, Goal domain,
   and autonomous continuation (architectures 26/27/28, ADR 0021/0022/0023/
   0030/0031/0033): durable harness rules and triggers, dossiers/checkpoints,
   execution classes, the 15 closed `harness_*` safe failures, the two closed
   root origins, the Goal tree and Verification Mandates, and Build-mode
   autonomous continuation.
4. **UI foundation** — session branching, activity/notification, reasoning/
   catalog delivery, and adapter boundaries (architectures 23/24/22, ADR 0026/
   0028/0029/0032/0033/0034): `session_fork_v1`, activity journal and
   notification projections, normalized reasoning delivery, RLM packaging and
   export, and the exact typed client/protocol surface that M6 consumes.
5. **Instruction sources and system context** — the instruction channel added
   by [ADR 0043](0043-instruction-sources-and-system-context.md) and owned by
   [architecture 30](../architecture/30-instruction-sources-and-system-context.md):
   the closed instruction source kinds and scopes, the deployment instruction
   profile adapted from the legacy Antibusy static prompt set, user-editable
   fragments, workspace `AGENTS.md` project instructions, the reserved `Mode`
   and `Vfr` contributions, deterministic assembly, the immutable effective
   instruction projection with its revision identity, closed `instruction_*`
   safe failures, and identity-only observability. Numeric bounds and digest
   wording are settled at activation under
   [ADR 0048](0048-limits-by-precedent-and-no-content-scanning.md) and
   [ADR 0046](0046-typed-serde-json-contracts.md). Its activating specification
   declares the instruction contract families under the Slice 1 ledger policy
   without changing slices 1-4.

Each direction from ADR 0020-0034 remains bound to its slice; every
retrospective change to M0-M5 code required by these directions is activated
inside its slice with its own contract, transaction, and outcome test.

## Rationale

Every post-M5 package (architectures 25-29 and ADR 0021-0034) closes with
"activation remains excluded pending a later M5+ specification", and the
post-M5 packages declare DTO, wire, storage, and quality values that M6-M9
consume. Without a single activation home that fixes the shared contract
ledger first, later milestones would each change contracts retroactively,
reproducing the piecemeal delivery the new edition exists to avoid. Making
M5+ the hard prerequisite of M6-M9 and delivering it as one pre-approved
slice sequence guarantees that M6-M9 start against stable contracts and never
need retroactive protocol, DTO, schema, crate-boundary, migration, or
quality-policy changes.

## Normative invariants

1. M5+ is the hard prerequisite of M6, M7, M8, and M9; the dependency graph
   carries `K[M5+] --> F/G/H/I` and no "parallel/non-blocking" wording.
2. M5+ activates only preparatory foundation work; it never implements
   M6-M9 milestone behavior (Tauri bridge/desktop UI, Plan/Build, VFR/
   Headroom, or M9 acceptance closure).
3. Contracts and versions precede control plane, harness, and UI foundation;
   later slices consume only contracts activated by earlier slices.
4. No slice ships half-ready: every activated contract ships with its version,
   owner, tests, policy mapping, migration behavior, and evidence together.
5. No downstream milestone requires a retroactive contract change after M5+
   exit.
6. M3/M4 startup-only configuration, recorded revisions, persisted snapshots,
   queue tickets, sessions, runs, events, and bytes remain authoritative and
   unchanged; no synthetic post-M5 record is added to historical runs.
7. M5+ introduces no second runtime, registry, scheduler, persistence
   authority, or sandbox.
8. No direction from ADR 0020-0034 is implemented by this decision; each
   remains non-authorizing until its slice activation.

## Failure semantics

- Each slice fails closed before effect when its contract is unsupported or
  inconsistent; no partial contract or partial projection is delivered.
- Recovery never resumes, retries, reattaches, or reruns work under any
  slice.
- A slice that cannot be completed atomically (contracts, tests, policy,
  evidence together) is not accepted; the milestone remains on the prior
  accepted slice.

## Compatibility and supersession

This decision extends and operationalizes ADR 0020 and ADR 0030-0034: their
domain decisions remain authoritative, and this decision supersedes only
conflicting activation and dependency wording (the "may run in parallel with
M6-M9" and "does not block their activation" clauses, and the per-direction
activation wording replaced by the pre-approved slice sequence). The closed
M4 baseline, M3/M4 bytes, and existing behavior remain unchanged. Activation
remains deferred: no code changes are authorized by this decision.

## Security and residual risk

The complete-foundation package introduces the shared contract, control-plane,
harness, and UI-foundation surface that later milestones consume. Every
activating slice must keep credentials and raw content out of durable/public
surfaces, keep redaction central, preserve M3/M4 byte meaning, and pass the
fake-secret regression suite. The primary compatibility risk is historical
contamination: no slice may synthesize harness, goal, activity, provider-
profile, or current-configuration meaning into M3/M4 runs.

## Affected documents

- [`architecture/11-implementation-roadmap.md`](../architecture/11-implementation-roadmap.md)
- [`architecture/README.md`](../architecture/README.md)
- [`decisions/README.md`](README.md)

## Required evidence

No implementation evidence is claimed. Each slice's activating specification
must declare exact crates, DTO/wire/storage versions, feature profiles,
coverage tiers, fixtures, and outcome evidence, and pass `make quick`,
`make verify`, and Linux/Windows CI before acceptance. The M5+ exit evidence
must prove that M6, M7, M8, and M9 can each begin against stable contracts
without any retroactive contract change.

## Non-goals

This decision does not implement the five slices; it does not change M3/M4
behavior; it does not renumber M5-M9; it does not activate a crate, schema,
migration, protocol, or feature. The slices remain non-authorizing until their
own activating specifications are accepted at the start of Milestone 5+.
