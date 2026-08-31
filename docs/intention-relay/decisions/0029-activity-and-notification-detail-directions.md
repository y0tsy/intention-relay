# ADR 0029: Post-M5 Activity and Notification Detail Directions

## Status

Accepted 2026-08-30. This decision records the activity/notification detail
layer as an accepted future direction. It does not activate implementation: the
layer is bound to
[Milestone 5+](../architecture/11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment)
and is implemented only through a separately accepted activating specification
at the start of that milestone.

## Decision

The agent-communication, activity-observation, and user-notification detail
from [`m4plus_concept.md`](../m4plus_concept.md) is adopted and owned by
[architecture 24](../architecture/24-activity-ui-and-adapters.md):

- `AgentActivitySelectionV1` (Root/Descendant) in `run-execution-meaning-v4`;
- `AgentActivityPairDto`, `AgentMessageDto`, `AgentMessageReferenceDto` (six
  closed variants), `AgentActivityJournalRecordDto`, `DirectChildStatusDto`,
  `DescendantSummaryDto`, and `AgentNotificationLevelDto`;
- the 17 closed journal record kinds;
- the fixed activity bounds (1,024 messages, 4 MiB aggregate, 4,096 journal
  records, 64 KiB record, 256/512-KiB page, 16 references, 60-minute
  clarification; 16/512-KiB per-direction with 1-slot/64-KiB clarification
  reserve);
- urgent-notification conditions and one-`Urgent`-per-(tree, reason) dedup;
- archive terminality precondition;
- the 32-tree/64-KiB notification page bound; and
- the 18 closed `agent_activity_*`/`agent_message_*`/`agent_notification_*`/
  `user_notifications_*` safe failures;
- the child-operations, delivery, and model-exchange detail: the full
  `RlmMessageExchangeDto` field family (activity tree, recipient session/run,
  target model step, captured activity-journal sequence, ordered messages) and
  the provider-translation constraints (distinct from text-only
  `ModelMessageDto` and `ModelToolExchangeDto`; no flattening into ordinary
  history, no inferred current message, no reordering, no remote
  continuation; daemon supplies all undelivered messages in increasing
  `AgentActivityJournalSequenceDto` order with `AgentPairOrderDto` proof), the
  `RlmChildMessageOperation` binding (direct parent link, child `ModelStepId`,
  daemon-assigned `RlmMessageId`, message kind, pair order, canonical payload
  digest; not a `ToolId`/`ToolCallId`/registry call/bridge operation/MCP
  command; cannot create, cancel, configure, or delegate to a child), and the
  terminal-or-cancelled-recipient delivery rejection with the original message
  and reason retained in the activity journal.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after
a later activating specification, and is bound to Milestone 5+ in the roadmap.
The numeric values are now first-scope limits adopted here (superseding the
"not implementation limits" wording in architecture 24's historical-projections
note) and are classified as intrinsic/capacity bounds, never Mandate quotas.

## Rationale

The authoritative package review of 2026-08-30 confirmed the agent
communication/observation/notification detail is present in `m4plus_concept.md`
but only at principle level in architecture 24, with the numeric values
explicitly deferred. This decision adopts the detail so the authoritative
documentation fully covers the feature, while preserving the project rule that
no feature is documented as implemented without code evidence.

## Normative invariants

1. Activity identity is daemon-assigned and distinct from Session, Run, lineage,
   notification, and Mandate-graph identity; a `RunId` is provenance, not
   authority.
2. Only a direct parent/child pair exchanges the closed message kinds; a sibling,
   root, adapter, bridge, kernel, MCP service, provider, or arbitrary caller
   cannot send a pair message.
3. `safe_text` is bounded redacted presentation; `RetainedContent` is disclosed
   only via the separately admitted `retrieve` tool.
4. Messages deliver only at the recipient's next fresh model request, in journal
   order, and never start work.
5. A semantic transition commits projections/journal/indexes/snapshots/
   notification reference atomically or not at all; publication follows commit
   and exact scoped reread.
6. Notifications are read/presentation only; at most one `Urgent` per
   (tree, cancellation reason); archive requires root and descendants terminal.
7. On restart nothing is retried or resumed; a later attempt is separately
   admitted with new identities.
8. `RlmMessageExchangeDto` is distinct from `ModelMessageDto` and
   `ModelToolExchangeDto`; a provider descriptor owns the private compatible
   translation but cannot flatten an RLM message into ordinary history, infer a
   current message, reorder it, or introduce a remote continuation; the daemon
   supplies all undelivered messages in increasing journal order.
9. `RlmChildMessageOperation` is bound to the direct parent link, child
   `ModelStepId`, daemon-assigned `RlmMessageId`, message kind, pair order, and
   canonical payload digest; it is not a `ToolId`, `ToolCallId`, registry
   invocation, bridge operation, MCP command, or independent authority, and it
   cannot create, cancel, configure, or delegate to a child.
10. A terminal or cancelled recipient rejects an undelivered ordinary message
    with a typed durable delivery outcome; the original message and its reason
    remain in the activity journal.

## Failure semantics

- A limit failure is checked before a partial durable record and never
  truncates, evicts, synthesizes, or starts external work.
- Invalid direction, stale link, terminal endpoint, duplicate/skipped order,
  invalid reference, or limit failure rejects before publication.
- An unaccepting or non-negotiating peer fails closed or resynchronizes and
  never blocks durable work or healthy subscribers.

## Compatibility and supersession

This decision supersedes the "numeric values retained in research are not
implementation limits" wording in architecture 24's historical-projections note
for the adopted values, and the absence of the detail in the principle-level
text. The closed M4 baseline, M3/M4 bytes, and existing behavior remain
unchanged. Activation remains deferred: no code changes are authorized by this
decision.

## Security and residual risk

The layer remains trusted-local. Messages, records, and summaries are bounded
and credential-free; redaction stays central and every activating specification
must pass the fake-secret regression suite.

## Affected documents

- [`architecture/11-implementation-roadmap.md`](../architecture/11-implementation-roadmap.md)
- [`architecture/24-activity-ui-and-adapters.md`](../architecture/24-activity-ui-and-adapters.md)
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
protocol, or feature. Native OS notifications, remote push, accounts, inbox
semantics, physical deletion, export, compaction, retention clocks, provider
UI/control planes, and production activation remain outside this decision.
