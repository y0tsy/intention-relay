# Post-M4 Authority Reconciliation

## Status

**Approved documentation-only planning package.** This directory coordinates the
post-M4 reconciliation of selected research into future authoritative
architecture. It does not authorize runtime, storage, protocol, crate, Cargo,
or quality-gate implementation.

## Authority and precedence

The governing order is:

1. repository instructions and approved architecture, roadmap, quality policy,
   and accepted decision records;
2. the closed M4 baseline and its closure evidence for historical behavior;
3. this reconciliation package as a coordination and traceability index;
4. [`m4plus_concept2.md`](../m4plus_concept2.md) as research provenance;
5. broader preserved research.

A reconciliation row points to authority. It is never a second normative
architecture source. Closed M4 remains unchanged. In particular, this package
neither changes M4 provider kinds, M4 tool-call denial, M4 replay behavior, nor
M4 startup-only configuration behavior.

## Scope

This package establishes the smallest future foundation needed before a later
implementation specification can be prepared:

- execution-kind separation;
- Mandate authority and fresh-run lifecycle boundaries;
- transaction/effect/publication law;
- external-effect uncertainty and no-resume recovery;
- historical non-reinterpretation;
- ownership and dependency direction;
- traceability, compatibility, and delivery sequencing.

It maps, but does not detail or implement, later child, verifier, MCP, Skill,
provider, reasoning, fork, kernel, activity, notification, and UI packages.

## Artifacts

| Artifact | Purpose |
| --- | --- |
| [Source-of-truth matrix](source-of-truth-matrix.md) | Atomic topic dispositions, owners, prerequisites, and evidence. |
| [Compatibility register](compatibility-register.md) | Closed M3/M4 behavior and permitted future additive boundaries. |
| [Contradiction register](contradiction-register.md) | Conflicts that must be resolved by a later authoritative package. |
| [Ownership and dependency map](ownership-and-dependency-map.md) | Layer ownership and future package ordering. |
| [Concept supersession index](concept-supersession-index.md) | Provenance coverage for selected concept headings. |
| [Decision records](../decisions/README.md) | Accepted Foundation decisions and their rationale. |

## Review baseline

- Closed M4 implementation baseline: `d2a85370a66d63fc759e4987a74d435ecd5d5115`.
- Current review branch baseline: `ced6e0e`.
- Primary research provenance: [`m4plus_concept2.md`](../m4plus_concept2.md).

## Transition rule

A later package may become implementation-ready only after its matrix topics,
owner documents, decision record, crate/test/coverage declarations, contract
fixtures, architecture fixtures, and outcome evidence are approved together.
This reconciliation approval authorizes documentation work only.

## Mandate lifecycle authority

[Mandate domain and durable lifecycle](../architecture/13-mandate-domain-and-durable-lifecycle.md)
now owns detailed Mandate lifecycle and admission rules. This reconciliation
package continues to coordinate provenance and dependencies only; it does not
become a duplicate runtime specification.

## Execution-meaning authority

[Run execution meaning and historical compatibility](../architecture/14-run-execution-meaning-and-historical-compatibility.md)
now owns envelope, canonical identity, decoder and historical compatibility
rules. Reconciliation remains provenance/index material and does not duplicate
that authority.

## Tool registry and Mandate-loop authority

[Tool registry and direct Mandate tool loop](../architecture/15-tool-registry-and-mandate-tool-loop.md)
now owns the fixed registry, immutable tool selection, Mandate direct admission,
Mandate WorkspaceRoot policy, tool-loop facts, and effect recovery. It resolves
CON-001 and CON-002 only for future Mandate execution; ordinary M3/M4 behavior
remains unchanged. [Decision 0007](../decisions/0007-unified-tool-registry-and-direct-mandate-tool-admission.md)
records the cross-document decision.

## Scheduler and readiness authority

[Mandate scheduler and readiness-driven admission](../architecture/16-mandate-scheduler-and-readiness-driven-admission.md)
now owns durable candidate reevaluation, typed readiness/capacity evidence, and
handoff to lifecycle-owned fresh admission. It does not replace the Mandate
lifecycle, execution-meaning, or tool-loop owners. [Decision 0008](../decisions/0008-durable-mandate-scheduler-and-readiness-driven-admission.md)
records the cross-document decision.

## Child graph and delegated verifier authority

[Mandate child graph and delegated verifier authority](../architecture/17-mandate-child-graph-and-delegated-verifier-authority.md)
now owns future immutable child edges, direct-parent controls, graph
terminalization, and separately issued target-scoped verifier authority.
[Decision 0009](../decisions/0009-mandate-child-graph-and-delegated-verifier-authority.md)
records the cross-document decision. Reconciliation remains an index and does
not duplicate those semantics or amend M3/M4 or retained RLM history.

## Mandate MCP capability authority

[Mandate MCP capability lifecycle](../architecture/18-mandate-mcp-capability-lifecycle.md)
now owns future typed MCP source acquisition, discovery normalization, immutable
run-local capability selections, invocation, disposal, and recovery.
[Decision 0010](../decisions/0010-mandate-mcp-capability-lifecycle.md) records
the cross-document decision. Reconciliation remains an index and does not amend
M3/M4 or retained bounded-MCP history.

## Mandate Gateway/RLM bridge authority

[Mandate Gateway/RLM bridge](../architecture/19-mandate-gateway-rlm-bridge.md)
now owns future bridge attachment, ephemeral grants, operation correlation,
safe replay, cancellation propagation, and bridge recovery. [Decision
0011](../decisions/0011-mandate-gateway-rlm-bridge.md) records the
cross-document decision. Reconciliation remains an index: retained RLM bridge
material is provenance, and the bridge is not a second gateway or authority.

## Run-scoped IPython kernel authority

[Run-scoped IPython kernel lifecycle](../architecture/20-ipython-kernel-lifecycle.md)
now owns future private kernel epochs, cells, checkpoints, safe projections, and
kernel recovery. [Decision 0012](../decisions/0012-ipython-kernel-lifecycle.md)
records the cross-document decision. Reconciliation remains an index; the kernel
consumes the bridge and one capability path and never becomes another authority.
