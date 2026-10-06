# 0026: Post-M5 Session-Branching Detail Directions

## Status

Accepted 2026-08-30. Not implemented; activation requires an activating specification.

Amended 2026-10-05 by [ADR 0053](0053-sub-agent-and-fork-limits-by-precedent.md): the depth, descendant-count,
source-boundary rate, and base-snapshot size limits are removed; the tree page and session title bounds stay.

## Decision

The session-branching detail from `m4plus_concept.md` is adopted and owned by architecture 23 (non-destructive session
branching and regeneration):

- the `session_fork_v1` public DTO families (`ForkSessionCommandDto`,
`ForkSessionResultDto`, `GetForkPreviewQueryDto`, `ForkPreviewDto`, `StartForkRunCommandDto`,
`GetConversationTreeQueryDto`, `ConversationTreePageDto`, `ConversationBranchSummaryDto`, `RenameSessionCommandDto`,
`ArchiveSessionCommandDto`, `RestoreSessionCommandDto`);
- the `fork-base-snapshot-v1`/`fork-preview-v1`/`fork-command-v1` field tables
and the `fork-base-snapshot-v2`/`fork-preview-v2` tables; the former `typed-tlv-v2` framing is removed with the
canonical codec by [ADR 0046](0046-typed-serde-json-contracts.md);
- `SessionTitleDto` (128 NFC Unicode scalar values) and the presentation
operation commands;
- the 64-summary tree page bound and no depth, descendant-count,
source-boundary rate, or base-snapshot size limit;
- the audit taxonomy (`SessionForked`, `ForkAnchorMaterialized`,
`SessionRenamed`, `SessionArchived`, `SessionRestored`, `ConversationTreeCreated`, `ConversationBranchLinked`);
- inherited-usage deduplication by original `RunId`;
- the 13 closed `fork_*`/`session_*`/`invalid_conversation_tree_page` safe
failures; and
- the `fork-model-context-v1` trilemma: a later implementation uses the stored
compatible schema unchanged, defines a separately versioned compatible projection, or blocks the dependent operation.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after its own activating specification, and
remains bound to Milestone 5+.

## Normative invariants

1. History remains append-only; a fork creates a new independent child
`SessionId` and never rewrites the source.
2. Exactly two closed `ForkBoundaryDto` variants exist; queued, partial,
failed, cancelled, interrupted, incomplete, waiting, and unfinished work are ineligible.
3. One transaction creates the child, lineage, base snapshot, anchor, events,
and idempotency result or none; no external work occurs inside it.
4. `ForkOperationId` is a domain idempotency identity; changed reuse fails
`fork_operation_conflict`.
5. Session lineage is ordinary-session structure only; a fork carries no
depth, descendant-count, source-boundary rate, or base-snapshot size limit.
6. Inherited usage is never charged twice; tree aggregates deduplicate by
original `RunId`.
7. M3/M4 historical sessions remain linear ordinary records until an additive
byte-preserving migration.

## Failure semantics

- Rejections are typed pre-effect failures enforced inside the fork transaction.
- A source/head mismatch is `fork_source_changed`; a preview mismatch is
`fork_preview_mismatch`; an ineligible or unavailable boundary is `fork_boundary_ineligible` or
`fork_history_unavailable`.
- A failed transaction leaves no partial child, lineage, event, snapshot, or
operation binding.

## Rationale

The branching detail (DTO families, field tables, error codes) was present in `m4plus_concept.md` but only at
principle level in architecture 23. Adopting the detail makes the authoritative documentation cover the feature without
documenting any part of it as implemented.

## Compatibility and non-goals

This decision supersedes the absence of the detail in architecture 23's principle-level text. The closed M4 baseline,
M3/M4 bytes, and existing behavior remain unchanged, and no code changes are authorized by this decision.

Activity/UI implementation, workspace cloning/rebinding, autonomous model or IPython forking, provider implementation,
destructive deletion/GC/export, and production activation remain outside this decision. M5-M9 are not renumbered.

Owner: architecture 23. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).
