# 0029: Post-M5 Activity and Notification Detail Directions

## Status

Accepted 2026-08-30. Not implemented; activation requires an activating specification.

## Decision

The agent-communication, activity-observation, and user-notification detail from `m4plus_concept.md` is adopted and
owned by [architecture 24](../architecture/24-activity-ui-and-adapters.md):

- `AgentActivitySelectionV1` (Root/Descendant);
- `AgentActivityPairDto`, `AgentMessageDto`, `AgentMessageReferenceDto` (six
closed variants), `AgentActivityJournalRecordDto`, `DirectChildStatusDto`, `DescendantSummaryDto`, and
`AgentNotificationLevelDto`;
- the 16 closed journal record kinds;
- the fixed activity bounds (1,024 messages, 4 MiB aggregate, 4,096 journal
records, 64 KiB record, 256/512-KiB page, 16 references, 60-minute clarification; 16/512-KiB per-direction with a
1-slot/64-KiB clarification reserve);
- urgent-notification conditions and one-`Urgent`-per-(tree, reason) dedup;
- the archive terminality precondition and the 32-tree/64-KiB notification
page bound;
- the 18 closed `agent_activity_*`/`agent_message_*`/`agent_notification_*`/
`user_notifications_*` safe failures; and
- the child-operations, delivery, and model-exchange detail: the full
`RlmMessageExchangeDto` field family (activity tree, recipient session/run, target model step, captured activity-journal
sequence, ordered messages) and the provider-translation constraints (distinct from text-only `ModelMessageDto` and
`ModelToolExchangeDto`; no flattening into ordinary history, no inferred current message, no reordering, no remote
continuation; the daemon supplies all undelivered messages in increasing `AgentActivityJournalSequenceDto` order with
`AgentPairOrderDto` proof), the `RlmChildMessageOperation` binding (direct parent link, child `ModelStepId`,
daemon-assigned `RlmMessageId`, message kind, pair order, canonical payload digest; not a `ToolId`/`ToolCallId`/registry
call/bridge operation/MCP command; cannot create, cancel, configure, or delegate to a child), and the
terminal-or-cancelled-recipient delivery rejection with the original message and reason retained in the activity
journal.

The former `run-execution-meaning-v4` carrier is removed with the canonical codec by [ADR
0046](0046-typed-serde-json-contracts.md); the selection records above are typed serde JSON records.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after its own activating specification, and
remains bound to Milestone 5+. The numeric values are first-scope limits adopted here and classified as
intrinsic/capacity bounds, never Mandate quotas.

## Normative invariants

1. Activity identity is daemon-assigned and distinct from Session, Run,
lineage, notification, and Mandate-graph identity; a `RunId` is provenance, not authority.
2. Only a direct parent/child pair exchanges the closed message kinds; a
sibling, root, adapter, bridge, kernel, MCP service, provider, or arbitrary caller cannot send a pair message.
3. `safe_text` is bounded redacted presentation; `RetainedContent` is
disclosed only via the separately admitted `retrieve` tool.
4. Messages deliver only at the recipient's next fresh model request, in
journal order, and never start work.
5. A semantic transition commits
projections/journal/indexes/snapshots/notification reference atomically or not at all; publication follows commit and
exact scoped reread.
6. Notifications are read/presentation only; at most one `Urgent` per
(tree, cancellation reason); archive requires root and descendants terminal.
7. On restart nothing is retried or resumed; a later attempt is separately
admitted with new identities.
8. `RlmMessageExchangeDto` is distinct from `ModelMessageDto` and
`ModelToolExchangeDto`; a provider descriptor owns the private compatible translation but cannot flatten an RLM message
into ordinary history, infer a current message, reorder it, or introduce a remote continuation.
9. `RlmChildMessageOperation` is bound to the direct parent link, child
`ModelStepId`, daemon-assigned `RlmMessageId`, message kind, pair order, and canonical payload digest; it is not a
`ToolId`, `ToolCallId`, registry invocation, bridge operation, or MCP command, and it cannot create, cancel, configure,
or delegate to a child.
10. A terminal or cancelled recipient rejects an undelivered ordinary message
with a typed durable delivery outcome; the original message and its reason remain in the activity journal.

## Failure semantics

- A limit failure is checked before a partial durable record and never
truncates, evicts, synthesizes, or starts external work.
- Invalid direction, stale link, terminal endpoint, duplicate or skipped
order, invalid reference, or limit failure rejects before publication.
- An unaccepting or non-negotiating peer fails closed or resynchronizes and
never blocks durable work or healthy subscribers.

## Rationale

The agent communication, observation, and notification detail was present in `m4plus_concept.md` but only at principle
level in architecture 24, with the numeric values explicitly deferred. Adopting the detail makes the authoritative
documentation cover the feature without documenting any part of it as implemented.

## Compatibility and non-goals

This decision supersedes the "numeric values retained in research are not implementation limits" wording in architecture
24's historical-projections note for the adopted values, and the absence of the detail in the principle-level text. The
closed M4 baseline, M3/M4 bytes, and existing behavior remain unchanged, and no code changes are authorized by this
decision.

Native OS notifications, remote push, accounts, inbox semantics, physical deletion, export, compaction, retention
clocks, provider UI or control planes, and production activation remain outside this decision. M5-M9 are not renumbered.

Owner: architecture 24. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).
