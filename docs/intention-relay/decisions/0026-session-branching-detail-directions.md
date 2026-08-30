# ADR 0026: Post-M5 Session-Branching Detail Directions

## Status

Accepted 2026-08-30. This decision records the session-branching detail layer
as an accepted future direction. It does not activate implementation: the layer
is bound to
[Milestone 5+](../architecture/11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment)
and is implemented only through a separately accepted activating specification
at the start of that milestone.

## Decision

The session-branching detail from
[`m4plus_concept2.md`](../m4plus_concept2.md) is adopted and owned by
[architecture 23](../architecture/23-non-destructive-session-branching-and-regeneration.md):

- the `session_fork_v1` public DTO families (`ForkSessionCommandDto`,
  `ForkSessionResultDto`, `GetForkPreviewQueryDto`, `ForkPreviewDto`,
  `StartForkRunCommandDto`, `GetConversationTreeQueryDto`,
  `ConversationTreePageDto`, `ConversationBranchSummaryDto`,
  `RenameSessionCommandDto`, `ArchiveSessionCommandDto`,
  `RestoreSessionCommandDto`);
- the canonical `fork-base-snapshot-v1`/`fork-preview-v1`/`fork-command-v1`
  field tables and the `typed-tlv-v2` `fork-base-snapshot-v2`/
  `fork-preview-v2` tables;
- `SessionTitleDto` (128 NFC Unicode scalar values) and the presentation
  operation commands;
- the fixed limits (depth 4,096, descendants 16,384, 16 forks per source
  boundary per rolling hour, 1 MiB base snapshot, 64-summary tree page);
- the audit taxonomy (`SessionForked`, `ForkAnchorMaterialized`,
  `SessionRenamed`, `SessionArchived`, `SessionRestored`,
  `ConversationTreeCreated`, `ConversationBranchLinked`);
- inherited-usage deduplication by original `RunId`; and
- the 17 closed `fork_*`/`session_*`/`invalid_conversation_tree_page` safe
  failures.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after
a later activating specification, and is bound to Milestone 5+ in the roadmap.

## Rationale

The authoritative package review of 2026-08-30 confirmed the branching detail
(limits, DTO families, field tables, error codes) is present in
`m4plus_concept2.md` but only at principle level in architecture 23. This
decision adopts the detail so the authoritative documentation fully covers the
feature, while preserving the project rule that no feature is documented as
implemented without code evidence.

## Normative invariants

1. History remains append-only; a fork creates a new independent child
   `SessionId` and never rewrites the source.
2. Exactly two closed `ForkBoundaryDto` variants exist; queued, partial, failed,
   cancelled, interrupted, incomplete, waiting, and unfinished work are
   ineligible.
3. One transaction creates the child, lineage, base snapshot, anchor, events,
   and idempotency result or none; no external work occurs inside it.
4. `ForkOperationId` is a domain idempotency identity; changed reuse fails
   `fork_operation_conflict`.
5. Tree depth, descendant count, and source-boundary rate limits are
   ordinary-session fork policy only and never constrain Mandate admission,
   scheduler behavior, or Mandate-child creation.
6. Inherited usage is never charged twice; tree aggregates deduplicate by
   original `RunId`.
7. M3/M4 historical sessions remain linear ordinary records until an additive
   byte-preserving migration.

## Failure semantics

- Limit failures are typed pre-effect rejections enforced inside the fork
  transaction.
- A source/head mismatch is `fork_source_changed`; a preview mismatch is
  `fork_preview_mismatch`; an ineligible or unavailable boundary is
  `fork_boundary_ineligible` or `fork_history_unavailable`.
- A failed transaction leaves no partial child, lineage, event, snapshot,
  operation binding, or rate consumption.

## Compatibility and supersession

This decision supersedes the absence of the detail in architecture 23's
principle-level text. The closed M4 baseline, M3/M4 bytes, and existing behavior
remain unchanged. Activation remains deferred: no code changes are authorized
by this decision.

## Security and residual risk

The layer remains trusted-local. Snapshots, previews, and failures are bounded
and credential-free; redaction stays central and every activating specification
must pass the fake-secret regression suite.

## Affected documents

- [`architecture/11-implementation-roadmap.md`](../architecture/11-implementation-roadmap.md)
- [`architecture/23-non-destructive-session-branching-and-regeneration.md`](../architecture/23-non-destructive-session-branching-and-regeneration.md)
- [`reconciliation/README.md`](../reconciliation/README.md)
- [`reconciliation/source-of-truth-matrix.md`](../reconciliation/source-of-truth-matrix.md)
- [`reconciliation/contradiction-register.md`](../reconciliation/contradiction-register.md)
- [`reconciliation/concept-supersession-index.md`](../reconciliation/concept-supersession-index.md)
- [`reconciliation/deferred-excluded-register.md`](../reconciliation/deferred-excluded-register.md)
- [`reconciliation/evidence-register.md`](../reconciliation/evidence-register.md)
- [`reconciliation/compatibility-register.md`](../reconciliation/compatibility-register.md)
- [`decisions/README.md`](README.md)

## Required evidence

No implementation evidence is claimed. The activating specification must declare
exact crates, DTO/wire/storage versions, feature profiles, coverage tiers,
fixtures, and outcome evidence, and pass `make quick`, `make verify`, and
Linux/Windows CI before acceptance.

## Non-goals

This decision does not implement the layer; it does not change M3/M4 behavior;
it does not renumber M5--M9; it does not activate a crate, schema, migration,
protocol, or feature. Mandate association, activity/UI implementation, workspace
cloning/rebinding, autonomous model/IPython forking, provider implementation,
destructive deletion/GC/export, and production activation remain outside this
decision.
