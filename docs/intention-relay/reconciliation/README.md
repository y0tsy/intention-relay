# Post-M4 Authority Reconciliation

## Status

The accepted transition direction is recorded in [ADR 0017](../decisions/0017-build-autopilot-and-plan-focus-continuity.md)
and [ADR 0018](../decisions/0018-plan-build-autopilot-activation-scope.md).
These records supersede only the identified future Plan/Build clauses: Plan is
planning focus with available advisory-guided `execute`, and Build Autopilot is
the single unrestricted trusted-local execution policy. Existing M3/M4 history,
typed Plan `write`/`edit` denial, fresh-run recovery, and no-resume semantics
remain preserved.

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
neither changes M4 provider kinds, M4 replay behavior, nor
M4 startup-only configuration behavior. ADR 0019 supersedes the M4 denial-only
tool boundary for newly admitted ordinary runs while the closed M4 baseline
remains historical evidence.

## Scope

This package establishes the smallest future foundation needed before a later
implementation specification can be prepared:

- execution-kind separation;
- Mandate authority and fresh-run lifecycle boundaries;
- transaction/effect/publication law;
- external-effect uncertainty and no-resume recovery;
- historical non-reinterpretation;
- ownership and dependency direction;
- traceability, compatibility, and delivery sequencing;
- post-M5 configuration and provider control-plane directions
  ([ADR 0020](../decisions/0020-configuration-provider-control-plane-directions.md),
  [Milestone 5+](../architecture/11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment));
- post-M5 continual-harness directions
  ([ADR 0021](../decisions/0021-continual-harness-directions.md)); and
- post-M5 programmatic-caller policy directions
  ([ADR 0022](../decisions/0022-programmatic-caller-policy-directions.md));
- post-M5 Goal domain and verification directions
  ([ADR 0023](../decisions/0023-goal-domain-and-verification-directions.md));
- post-M5 provider session-selection and profiles protocol directions
  ([ADR 0024](../decisions/0024-provider-session-and-profiles-protocol-directions.md));
- post-M5 base-tool contracts and tool-loop bounds
  ([ADR 0025](../decisions/0025-base-tool-contracts-and-tool-loop-bounds.md));
- post-M5 session-branching detail
  ([ADR 0026](../decisions/0026-session-branching-detail-directions.md));
- post-M5 child, kernel, bridge, and MCP detail
  ([ADR 0027](../decisions/0027-child-kernel-bridge-mcp-detail-directions.md));
- post-M5 provider reasoning and catalog detail
  ([ADR 0028](../decisions/0028-provider-reasoning-and-catalog-detail-directions.md));
- post-M5 activity and notification detail
  ([ADR 0029](../decisions/0029-activity-and-notification-detail-directions.md));
- post-M5 continual-harness closed safe failures and selection-record detail
  ([ADR 0030](../decisions/0030-continual-harness-safe-failures-and-selection-record-detail.md));
- post-M5 autonomous continuation direction
  ([ADR 0031](../decisions/0031-autonomous-continuation-direction.md));
- post-M5 accepted deferred directions: activity-tree metadata, semantic
  content inspection, and per-call cancellation
  ([ADR 0032](../decisions/0032-accepted-deferred-directions-activity-metadata-content-inspection-per-call-cancellation.md));
- post-M5 accepted execution directions: raw-TOML/configuration editing, model
  discovery, arbitrary headers, provider-native preservation, server-side
  parser, fork tool-result/child-agent execution, export, cross-workspace
  clone/rebind, autonomous harness goal mode, post-disconnect work, and RLM
  packaging
  ([ADR 0033](../decisions/0033-accepted-m5plus-execution-directions.md));
- post-M5 accepted retained-deferral directions: rich MIME/raw kernel output
  projection, physical deletion/GC of historical work, worker/process
  supervision topology, calendar/interval/time-zone/DST semantics, and
  activity numeric limit classification
  ([ADR 0034](../decisions/0034-accepted-m5plus-retained-deferral-directions.md));
- post-M5 complete foundation activation: Milestone 5+ as the hard
  prerequisite of M6-M9 and the single activation home for the complete
  post-M5 stack, delivered as one pre-approved four-slice sequence
  (contracts/versions, control plane, harness, UI foundation)
  ([ADR 0035](../decisions/0035-m5plus-complete-foundation-activation.md)).

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
| Configuration/provider control plane | architecture 25 | decision 0020 |
| Continual harness | architecture 26 | decision 0021 |
| Programmatic-caller policy | architecture 27 | decision 0022 |
| Goal domain and verification | architecture 28 | decision 0023 |
| Provider session selection and profiles protocol | architecture 29 | decision 0024 |
| Base-tool contracts and tool-loop bounds | architecture 15 | decision 0025 |
| Session-branching detail | architecture 23 | decision 0026 |
| Child, kernel, bridge, and MCP detail | architectures 17/20/19/18 | decision 0027 |
| Provider reasoning and catalog detail | architecture 22 | decision 0028 |
| Activity and notification detail | architecture 24 | decision 0029 |
| Continual-harness safe failures and selection record | architectures 26/28 | decision 0030 |
| Autonomous continuation | architecture 13 | decision 0031 |
| Accepted deferred directions (activity metadata, content inspection, per-call cancellation) | architectures 24/22/19 | decision 0032 |
| Accepted execution directions (control-plane editing, provider-native controls, fork execution, harness autonomy, RLM packaging) | architectures 25/22/23/26/28/18/24/29 | decision 0033 |
| Accepted retained-deferral directions (kernel output projection, retention policy, supervision topology, calendar semantics, activity limit classification) | architectures 20/04/03/16/24 | decision 0034 |
| M5+ complete foundation activation | architectures 25-30, 23, 22, 24, roadmap | decision 0035 |

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
