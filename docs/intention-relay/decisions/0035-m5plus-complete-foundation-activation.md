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

## Decision

Milestone 5+ is the single activation home for the full post-M5 stack
(architectures 25-30, ADR 0021-0034, and all detail packages) and the hard
prerequisite of Milestones 6-9 in the roadmap dependency graph
(`K[M5+] --> F/G/H/I`). The milestone is delivered as a pre-approved sequence
of activating slices, all approved together as one package:

1. **Contracts and versions** — the versioned protocol/schema/
   execution-meaning/DTO contract ledger for the full post-M5 stack, including
   `run-execution-meaning-v4` (the single live record version; v3 is removed by
   [ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md)), the
   negotiated capability families
   (`provider_profiles_v1`, `session_fork_v1`,
   `normalized_reasoning_stream_v1`, `agent_activity_v1`,
   `user_notifications_v1`, `daemon_tool_gateway_v1`, `model_tool_loop_v1`),
   additive storage migration with M3/M4 byte preservation, canonical tags and
   digests under the existing `typed-tlv-v1`/SHA-256 policy, and crate
   ownership, feature-profile, and coverage-tier declarations for every
   activated family.
2. **Control plane** — the ADR 0020 cluster and provider session selection
   (architectures 25/29/22): controlled live reload, credential rotation,
   provider health checks, model discovery, pricing policy, profile UI and
   raw-TOML/configuration editing, arbitrary authentication headers,
   provider-native preservation controls, server-side parser setup, session
   defaults and per-turn/fork overrides, unavailable-queue promotion and
   reconciliation, `provider_profiles_v1`, pending-removal and degraded
   recovery, and the provider reasoning/catalog surface.
3. **Harness** — continual harness, programmatic-caller policy, Goal domain,
   and autonomous continuation (architectures 26/27/28, ADR 0021/0022/0023/
   0030/0031/0033): durable harness rules and triggers, dossiers/checkpoints,
   execution classes, the 15 closed `harness_*` safe failures, the two closed
   root origins, corridors and reservations, the Goal tree and Verification
   Mandates, and Build-mode autonomous continuation.
4. **UI foundation** — session branching, activity/notification, reasoning/
   catalog delivery, and adapter boundaries (architectures 23/24/22, ADR 0026/
   0028/0029/0032/0033/0034): `session_fork_v1`, activity journal and
   notification projections, normalized reasoning delivery, the legacy M4
   selection bridge, RLM packaging and export, and the exact typed
   client/protocol surface that M6 consumes.

Each direction from ADR 0020-0034 remains bound to its slice; every
retrospective change to M0-M5 code required by these directions is activated
inside its slice with its own contract, transaction, and outcome test.

## Rationale

Every post-M5 package (architectures 25-30 and ADR 0021-0034) closes with
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

- Each slice fails closed before effect when its contract is unsupported,
  unnegotiated, or over-limit; no partial contract, partial projection, or
  partial migration is delivered.
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
- [`reconciliation/README.md`](../reconciliation/README.md)
- [`reconciliation/source-of-truth-matrix.md`](../reconciliation/source-of-truth-matrix.md)
- [`reconciliation/evidence-register.md`](../reconciliation/evidence-register.md)
- [`reconciliation/ownership-and-dependency-map.md`](../reconciliation/ownership-and-dependency-map.md)
- [`reconciliation/contradiction-register.md`](../reconciliation/contradiction-register.md)
- [`reconciliation/deferred-excluded-register.md`](../reconciliation/deferred-excluded-register.md)
- [`reconciliation/compatibility-register.md`](../reconciliation/compatibility-register.md)

## Required evidence

No implementation evidence is claimed. Each slice's activating specification
must declare exact crates, DTO/wire/storage versions, feature profiles,
coverage tiers, fixtures, and outcome evidence, and pass `make quick`,
`make verify`, and Linux/Windows CI before acceptance. The M5+ exit evidence
must prove that M6, M7, M8, and M9 can each begin against stable contracts
without any retroactive contract change.

## Non-goals

This decision does not implement the four slices; it does not change M3/M4
behavior; it does not renumber M5-M9; it does not activate a crate, schema,
migration, protocol, or feature. The slices remain non-authorizing until their
own activating specifications are accepted at the start of Milestone 5+.
