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

It maps, but does not detail or implement, later tool-loop, descriptor,
WorkspaceRoot/direct-admission, child, verifier, MCP, Skill, provider,
reasoning, fork, kernel, activity, notification, and UI packages.

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
