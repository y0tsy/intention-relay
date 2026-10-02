# 0033: Post-M5 Accepted Directions — Control-Plane Editing, Provider-Native Controls, Fork Execution, Harness Autonomy, and RLM Packaging

## Status

Accepted 2026-08-30. Not implemented; activation requires an activating specification.

Amended 2026-09-30: the capability families, the binary canonical codec, the corridor and reservation model, and the
fixed numeric contract limits named by the owning packages are removed by [ADR
0045](0045-local-json-rpc-2-0-transport.md), [ADR 0046](0046-typed-serde-json-contracts.md), and [ADR
0048](0048-limits-by-precedent-and-no-content-scanning.md), and by the Slice 2 revert. The thirteen directions otherwise
stand, non-authorizing as before.

## Decision

The following items, named in the `m4plus_concept.md` backlog as deferred or excluded, are adopted as accepted future
directions for execution in Milestone 5+, each owned by the package named below:

**Configuration and provider control plane (owner: architecture 25):**
1. **Provider-profile UI and raw-TOML editing** — a provider profile UI and a
safe, validated raw-TOML editing surface over the shared typed client, never adapter authority; profile edits surface
through the reload and catalog contracts and affect fresh runs only.
2. **Configuration editing** — a validated configuration-editing surface that
produces a new candidate snapshot through the same atomic reload contract; never in-place mutation of an admitted run or
recorded snapshot.
3. **Model discovery** — non-authorizing discovery of provider/model
capabilities as typed records, never model-name routing (extends CFG-005 of [ADR
0020](0020-configuration-provider-control-plane-directions.md)).

**Provider evolution and reasoning (owner: architecture 22):**
4. **Arbitrary authentication headers** — a closed, code-owned, typed
header-policy surface that may declare additional validated headers beyond bearer/one-selected-header, each bound to a
descriptor/kind revision and never entering durable or public identity.
5. **Provider-native preservation controls** — explicit typed controls for
provider-native reasoning preservation (`preserve_thinking`, `thinking.keep`, and similar) under the local-history-first
law, never remote continuation.
6. **Server-side parser setup** — explicit typed configuration for
server-side vLLM/SGLang-style parser setup where a closed descriptor declares it, never raw JSON or templates and never
unbounded parsing.

**Session branching and regeneration (owner: architecture 23):**
7. **Tool-result execution** — a future fork/regeneration mode that may
execute a frozen terminal tool result as a separately admitted ordinary action, never silent re-execution and never
Mandate authority.
8. **Child-agent execution** — a future fork/regeneration mode that may start
a child-agent execution from frozen fork references, never Mandate child edges and never verifier authority.

**Activity, harness, and MCP boundaries (owners: architectures 23/24/26/28/18):**
9. **Export** — a bounded, credential-free export surface for fork lineage,
activity, and harness records, never raw history rewrite and never destructive deletion.
10. **Cross-workspace clone/rebind** — an explicit user-authorized future
direction for cloning or rebinding a fork tree to another `WorkspaceRoot`, never implicit and never transferring live
state or authority.
11. **Autonomous harness goal mode** — a future harness mode where a
goal-directed rule may continue against an active goal, separately admitted and never an autonomous free-running agent.
12. **Work/requeue after client disconnection** — a future explicit contract
for durable work, continuation, or requeue after client disconnection, never silent automatic resumption of old external
work.
13. **Delivery of all RLM capabilities in one package** — the packaging
direction that a later M5+ activating specification may consolidate all RLM capabilities (child graph, sub-agent
classes, direct-pair messaging, activity identity) into one implementation package, preserving the existing
documentation package boundaries.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after its own activating specification, and
remains bound to Milestone 5+.

## Normative invariants

1. Every direction is non-authorizing until its activating specification: it
creates no `RunId`, reason, lifecycle transition, scheduler candidate, tool permission, child edge, verifier authority,
MCP capability, bridge grant, kernel epoch, context projection, branch, or reconciliation result.
2. M3/M4 startup-only configuration, recorded revisions, persisted snapshots,
queue tickets, sessions, runs, events, and bytes remain authoritative and unchanged.
3. Fresh-run-only: each direction affects only future runs after activation.
4. No direction rewrites historical bytes, assigns new meaning to a closed
variant, or reconstructs missing meaning from current state.
5. Raw-TOML editing, configuration editing, and arbitrary headers never expose
credentials, private endpoint material, SDK objects, or raw provider payloads on durable or public surfaces.
6. Tool-result and child-agent execution in forks are separately admitted
ordinary actions and never Mandate child edges, verifier authority, or silent re-execution.
7. Autonomous harness goal mode and post-disconnect work are separately
admitted and never resume, retry, reattach, or rerun old external work.
8. Export and cross-workspace clone/rebind are bounded, credential-free, and
never destructive; clone/rebind is explicit user-authorized only.
9. RLM packaging consolidates implementation delivery only; it preserves the
documentation package boundaries and the one-capability-path law.

## Failure semantics

- Each direction fails closed before effect when its future contract is
unsupported or inconsistent; no partial projection, export, or rebind is delivered.
- Recovery never resumes, retries, reattaches, or reruns work under any of the
thirteen directions.
- Configuration or TOML edits that cannot be applied atomically fail closed
and leave the running daemon on its recorded snapshot.

## Rationale

The thirteen items were named in `m4plus_concept.md` as deferred or excluded but appeared in the authoritative
documentation only inside non-goals, without a Milestone 5+ delivery home. Adopting them schedules the directions for
execution in Milestone 5+ rather than permanent exclusion, without documenting any feature as implemented.

## Compatibility and non-goals

This decision supersedes the "excluded"/"deferred" wording for the thirteen items in the non-goals of ADR
0020/0021/0028/0031 and architectures 22/23/25/26/28/29/18. The closed M4 baseline, M3/M4 bytes, and existing behavior
remain unchanged, and no code changes are authorized by this decision. The directions remain non-authorizing until their
M5+ activating specifications; M5-M9 are not renumbered.

Owner: architectures 25, 22, 23, 18, 24, 26, and 28. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).
