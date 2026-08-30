# ADR 0033: Post-M5 Accepted Directions — Control-Plane Editing, Provider-Native Controls, Fork Execution, Harness Autonomy, and RLM Packaging

## Status

Accepted 2026-08-30. This decision records thirteen deliberately deferred
concept items as accepted future directions to be executed in
[Milestone 5+](../architecture/11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment).
It does not activate implementation: each direction is bound to Milestone 5+
and is implemented only through a separately accepted activating specification
at the start of that milestone.

## Decision

The following items, named in the
[`m4plus_concept2.md`](../m4plus_concept2.md) backlog as deferred or excluded,
are adopted as accepted future directions for execution in Milestone 5+:

**Configuration and provider control plane (owner: architecture 25):**
1. **Provider-profile UI and raw-TOML editing** — a provider profile UI and a
   safe, validated raw-TOML editing surface over the shared typed client,
   never adapter authority; profile edits surface through the reload and
   catalog contracts and affect fresh runs only.
2. **Configuration editing** — a validated configuration-editing surface that
   produces a new candidate snapshot through the same atomic reload contract;
   never in-place mutation of an admitted run or recorded snapshot.
3. **Model discovery** — non-authorizing discovery of provider/model
   capabilities as typed records, never model-name routing (extends CFG-005
   of [ADR 0020](../decisions/0020-configuration-provider-control-plane-directions.md)).

**Provider evolution and reasoning (owner: architecture 22):**
4. **Arbitrary authentication headers** — a closed, code-owned, typed
   header-policy surface that may declare additional validated headers beyond
   bearer/one-selected-header, each bound to a descriptor/kind revision and
   never entering durable/public identity.
5. **Provider-native preservation controls** — explicit typed controls for
   provider-native reasoning preservation (`preserve_thinking`,
   `thinking.keep`, and similar) under the local-history-first law, never
   remote continuation.
6. **Server-side parser setup** — explicit typed configuration for
   server-side vLLM/SGLang-style parser setup where a closed descriptor
   declares it, never raw JSON/templates and never unbounded parsing.

**Session branching and regeneration (owner: architecture 23):**
7. **Tool-result execution** — a future fork/regeneration mode that may
   execute a frozen terminal tool result as a separately admitted ordinary
   action, never silent re-execution and never Mandate authority.
8. **Child-agent execution** — a future fork/regeneration mode that may start
   a child-agent execution from frozen fork references, never Mandate child
   edges and never verifier authority.

**Activity, harness, and MCP boundaries (owners: architectures 23/24/26/28/18):**
9. **Export** — a bounded, credential-free export surface for fork lineage,
   activity, and harness records, never raw history rewrite and never
   destructive deletion.
10. **Cross-workspace clone/rebind** — an explicit user-authorized future
    direction for cloning or rebinding a fork tree to another `WorkspaceRoot`,
    never implicit and never transferring live state or authority.
11. **Autonomous harness goal mode** — a future harness mode where a
    goal-directed rule may continue against an active goal, separately
    admitted and never an autonomous free-running agent.
12. **Work/requeue after client disconnection** — a future explicit contract
    for durable work, continuation, or requeue after client disconnection,
    never silent automatic resumption of old external work.
13. **Delivery of all RLM capabilities in one package** — the packaging
    direction that a later M5+ activating specification may consolidate all
    RLM capabilities (child graph, sub-agent classes, direct-pair messaging,
    activity identity) into one implementation package, preserving the
    existing documentation package boundaries.

Each direction:

- keeps M3/M4 behavior authoritative for existing persisted runs and
  snapshots;
- affects fresh runs only after a later activating specification;
- is bound to Milestone 5+ in the roadmap;
- requires its own activating specification (crates, DTO/wire/storage
  versions, quality policy, feature profiles, migration declarations, tests,
  and outcome evidence) accepted at the start of the milestone.

## Rationale

The sixth-wave authoritative package review of 2026-08-30 confirmed these
thirteen items are named in `m4plus_concept2.md` as deferred or excluded but
appear in the authoritative documentation only inside non-goals of ADR
0020/0021/0028/0031 and architectures 22/23/25/26/28/29/18, without rows in
the deferred/excluded register and without a Milestone 5+ delivery home. This
decision adopts them as accepted future directions so they are scheduled for
execution in Milestone 5+ rather than permanently excluded, while preserving
the project rule that no feature is documented as implemented without code
evidence.

## Normative invariants

1. Every direction is non-authorizing until its activating specification:
   it creates no RunId, reason, lifecycle transition, scheduler candidate,
   tool permission, child edge, verifier authority, MCP capability, bridge
   grant, kernel epoch, context projection, branch, or reconciliation result.
2. M3/M4 startup-only configuration, recorded revisions, persisted snapshots,
   queue tickets, sessions, runs, events, and bytes remain authoritative and
   unchanged.
3. Fresh-run-only: each direction affects only future runs after activation.
4. No direction rewrites historical bytes, assigns new meaning to a closed
   variant, or reconstructs missing meaning from current state.
5. Raw-TOML editing, configuration editing, and arbitrary headers never
   expose credentials, private endpoint material, SDK objects, or raw
   provider payloads on durable/public surfaces.
6. Tool-result and child-agent execution in forks are separately admitted
   ordinary actions and never Mandate child edges, verifier authority, or
   silent re-execution.
7. Autonomous harness goal mode and post-disconnect work are separately
   admitted and never resume, retry, reattach, or rerun old external work.
8. Export and cross-workspace clone/rebind are bounded, credential-free, and
   never destructive; clone/rebind is explicit user-authorized only.
9. RLM packaging consolidates implementation delivery only; it preserves the
   documentation package boundaries and the one-capability-path law.

## Failure semantics

- Each direction fails closed before effect when its future contract is
  unsupported, unnegotiated, or over-limit; no partial projection, partial
  export, or partial rebind is delivered.
- Recovery never resumes, retries, reattaches, or reruns work under any of
  the thirteen directions.
- Configuration/TOML edits that cannot be applied atomically fail closed and
  leave the running daemon on its recorded snapshot.

## Compatibility and supersession

This decision supersedes the "excluded"/"deferred" wording for the thirteen
items in the non-goals of ADR 0020/0021/0028/0031 and architectures
22/23/25/26/28/29/18, and records them as accepted future directions bound to
Milestone 5+. The closed M4 baseline, M3/M4 bytes, and existing behavior
remain unchanged. Activation remains deferred: no code changes are authorized
by this decision.

## Security and residual risk

The directions remain trusted-local. Raw-TOML editing, configuration editing,
arbitrary headers, and export introduce new credential-handling and
content-handling surface; every activating specification must keep
credentials credential-free in durable/public surfaces, keep redaction
central, and pass the fake-secret regression suite. Autonomous harness goal
mode and post-disconnect work must never resume old external work.

## Affected documents

- [`architecture/11-implementation-roadmap.md`](../architecture/11-implementation-roadmap.md)
- [`architecture/18-mandate-mcp-capability-lifecycle.md`](../architecture/18-mandate-mcp-capability-lifecycle.md)
- [`architecture/22-provider-evolution-profiles-and-reasoning.md`](../architecture/22-provider-evolution-profiles-and-reasoning.md)
- [`architecture/23-non-destructive-session-branching-and-regeneration.md`](../architecture/23-non-destructive-session-branching-and-regeneration.md)
- [`architecture/24-activity-ui-and-adapters.md`](../architecture/24-activity-ui-and-adapters.md)
- [`architecture/25-configuration-provider-control-plane.md`](../architecture/25-configuration-provider-control-plane.md)
- [`architecture/26-continual-harness.md`](../architecture/26-continual-harness.md)
- [`architecture/28-goal-domain-and-verification.md`](../architecture/28-goal-domain-and-verification.md)
- [`architecture/29-provider-session-and-profiles-protocol.md`](../architecture/29-provider-session-and-profiles-protocol.md)
- [`reconciliation/README.md`](../reconciliation/README.md)
- [`reconciliation/source-of-truth-matrix.md`](../reconciliation/source-of-truth-matrix.md)
- [`reconciliation/contradiction-register.md`](../reconciliation/contradiction-register.md)
- [`reconciliation/concept-supersession-index.md`](../reconciliation/concept-supersession-index.md)
- [`reconciliation/deferred-excluded-register.md`](../reconciliation/deferred-excluded-register.md)
- [`reconciliation/evidence-register.md`](../reconciliation/evidence-register.md)
- [`reconciliation/compatibility-register.md`](../reconciliation/compatibility-register.md)
- [`reconciliation/ownership-and-dependency-map.md`](../reconciliation/ownership-and-dependency-map.md)
- [`decisions/README.md`](README.md)

## Required evidence

No implementation evidence is claimed. Each direction's activating
specification must declare exact crates, DTO/wire/storage versions, feature
profiles, coverage tiers, fixtures, and outcome evidence, and pass `make
quick`, `make verify`, and Linux/Windows CI before acceptance.

## Non-goals

This decision does not implement the thirteen directions; it does not change
M3/M4 behavior; it does not renumber M5--M9; it does not activate a crate,
schema, migration, protocol, or feature. The directions remain non-authorizing
until their M5+ activating specifications.
