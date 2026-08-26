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
| [Concept supersession index](concept-supersession-index.md) | Provenance coverage and supersession mapping for selected concept headings. |
| [Evidence register](evidence-register.md) | Exact evidence anchors and verified/planned status. |
| [Deferred and excluded register](deferred-excluded-register.md) | Atomic non-authorized deferred and excluded claims. |
| [Decision records](../decisions/README.md) | Accepted Foundation decisions and their rationale. |

## Review baseline

- Closed M4 implementation baseline: `d2a85370a66d63fc759e4987a74d435ecd5d5115`.
- Documentation review baseline: branch `docs/m4plus-authoritative-replan`, reviewed at `f3ecaca` on 2026-08-26.
- The M4 implementation baseline above is immutable; documentation review baselines are moving metadata and must be refreshed when this package changes.
- Primary research provenance: [`m4plus_concept2.md`](../m4plus_concept2.md).

## Transition rule

A later package may become implementation-ready only after its matrix topics,
owner documents, decision record, crate/test/coverage declarations, contract
fixtures, architecture fixtures, and outcome evidence are approved together.
This reconciliation approval authorizes documentation work only.

## Owner map

Detailed semantics live only in the linked owner architecture documents. The
following map is navigation and provenance; it does not restate their rules:

| Domain | Owner | Decision |
| --- | --- | --- |
| Mandate lifecycle/admission | architecture 13 | decision 0006 |
| Execution meaning/compatibility | architecture 14 | decision 0003 |
| Tools/direct admission | architecture 15 | decision 0007 |
| Scheduler/readiness | architecture 16 | decision 0008 |
| Child graph/verifier | architecture 17 | decision 0009 |
| MCP lifecycle | architecture 18 | decision 0010 |
| Gateway/RLM bridge | architecture 19 | decision 0011 |
| Kernel lifecycle | architecture 20 | decision 0012 |
| Goals/Skills/context | architecture 21 | decision 0013 |
| Provider/reasoning | architecture 22 | decision 0014 |
| Session branching | architecture 23 | decision 0015 |
| Activity/UI/adapters | architecture 24 | decision 0016 |

Use the [source-of-truth matrix](source-of-truth-matrix.md) for topic ownership,
the [contradiction register](contradiction-register.md) for conflict resolution,
the [compatibility register](compatibility-register.md) for historical laws,
the [evidence register](evidence-register.md) for evidence status, and the
[deferred/excluded register](deferred-excluded-register.md) for non-scope claims.

### Controlled status vocabulary

Use separate fields and never treat one as another:

- **Disposition:** `Adopt`, `Adapt`, `Supersede`, `PreserveHistorical`, `Defer`, `Exclude`, `Conflict`.
- **Package status:** `Research`, `Planned`, `Documentation-approved`, `Implementation-authorized`, `Implemented`, `Closed`, `Superseded`.
- **Evidence status:** `Verified`, `Planned`, `Deferred`, `Not applicable`, `Blocked`.
- **Baseline status:** `Immutable`, `Moving review`, `Unverified`.

`Adopt` means selected for authoritative replanning only. It never means
implementation-authorized or verified.
