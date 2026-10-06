# ADR 0019: Pending turns and cooperative interruption

## Status

Accepted 2026-10-05. It replaces the durable turn queue and its ticket promotion with pending turns that join the live
run context at the next model boundary, and it replaces run cancellation with cooperative interruption of the in-flight
provider call or tool call. It authorizes no new durable table, event family, storage schema, or wire version.

## Scope and supersession

In scope: turn acceptance and pending input; the `turns` store and its closed `outcome` set; `SendUserTurnOutcomeDto`;
`PendingTurnProjectionDto` and `SessionProjectionDto.pending_turns`; the `UserTurnPending` and `TurnRemoved` events and
the `turn.remove` method; the `run.interrupt` command and its accepted result; the run status set and its transition
table; `InterruptNoticeRecorded`; cooperative tool interruption with partial results; and the removed dead service,
gate, and terminalizer machinery.

Out of scope: provider retry policy; the tool lifecycle and result vocabularies; `Partial` semantics and its notices
(ADR 0017); the restart recovery transition to `Interrupted`; Goal, Skill, context, and compaction semantics; and
recorded M3/M4 bytes.

| Record | Superseded clause | Replaced by |
| --- | --- | --- |
| [ADR 0004](0004-m5plus-complete-foundation-activation.md) | Preservation clauses keeping "M3/M4... queue tickets... authoritative and unchanged" | The lists drop queue tickets; recorded sessions, runs, events, snapshots, replay, and recovery stay authoritative, and pending turns are durable input joined to the live run context |
| [ADR 0011](0011-local-json-rpc-2-0-transport.md) | Method-table rows `turn.remove` (`RemoveQueuedTurnCommandDto`) and `run.stop` (`StopRunCommandDto`) | `turn.remove` carries `RemoveTurnCommandDto` and `run.interrupt` carries `InterruptRunCommandDto`; the method set, framing, and DTO-only payload rule are unchanged |
| [ADR 0014](0014-limits-by-precedent-and-no-content-scanning.md) | Decision item 9, invariant 5, and the compatibility clause that keep the live M3 queue, its tickets, and its atomic promotion as the queue authority | Pending turns are durable input joined to the live run context at the next boundary; the queue, its tickets, and its promotion no longer exist |
| [ADR 0015](0015-ordering-authorities.md) | Invariant 5 and its out-of-scope, compatibility, and non-goals clauses on `sessions.next_queue_ticket` and `QueuePositionDto` | No queue-ordering mechanism exists: the queue tables, columns, tickets, and positions were removed, and pending turns are durable input rather than a queue order |

The named clause of each record is amended in place; every other clause of those records stays as written. The
per-call cancellation direction of
is superseded as a direction and its text stays as written; its other two directions remain accepted.

## Decision

A session is continuous: user input that arrives while a run is active joins that run instead of waiting for a
successor run.

**Turn acceptance.** `SendUserTurnCommandDto` either starts a run or records a pending turn. When the session has no
active run, acceptance starts a run — the oldest pending turn when one exists, otherwise the accepted turn itself; a
turn accepted alongside that successor run is recorded pending and joins it. The acceptance reports
`SendUserTurnOutcomeDto::Started` with that run and its frozen `ConfigRevisionId`; the committed facts are
`UserTurnAccepted` and `RunStarted`, plus `UserTurnPending` for a turn stored pending. When an active run exists, the
accepted turn is stored in `turns` with `outcome = 'pending'`, its own proposed `RunId`, and its
selected configuration revision, and the acceptance reports `SendUserTurnOutcomeDto::Pending` with a committed
`UserTurnPending` fact. `turns` is the only turn store, and its closed `outcome` set is `started`, `pending`, and
`appended`.

**Boundary join.** A pending turn joins the live run context at the next boundary: after the provider response that
ended a model step, or after a tool batch. The runtime commits the join as `UserMessageAppended` facts in durable
insertion order and marks those turns `appended` in the same storage transaction, then rebuilds the continuation
request. The run continues: nothing is promoted, no second run opens, and no ticket, position, or queue order exists.
Pending turns are durable input, so a restart preserves them, and a pending turn whose run ended before the join starts
the successor run through ordinary acceptance once the session has no active run. `turn.remove`
(`RemoveTurnCommandDto`, event `TurnRemoved`) removes one not-yet-seen pending turn; an appended turn can no longer be
removed, and removal renumbers nothing.

**Interruption.** `run.interrupt` (`InterruptRunCommandDto`, protocol `ProtocolCommandDto::InterruptRun`, accepted
result `ProtocolAcceptedResultDto::InterruptRun`) replaces `run.stop`. Acceptance validates that the exact run is
active, changes no durable run state, and reports the session position; the daemon host then signals the registered
execution task through the same provider-neutral cancellation signal. The runtime ends the in-flight provider stream or
tool call, records one durable `InterruptNoticeRecorded` fact, resets the signal, and rebuilds the continuation
request, so the model receives its next step in the same `Running` run. The two fixed notices are:

| Interrupted operation | Notice |
| --- | --- |
| provider stream or another call without its own result | `[The call was stopped before a final result.]` |
| tool call stopped before it started | `[The tool call was stopped before a final result.]` |

An interrupt that arrives during a retry wait records the notice and starts the next attempt immediately. An interrupt
that arrives before a requested tool call starts never begins an effect: the call is answered with the
stopped-tool-call notice as a bounded `Partial` result, so the assistant tool-call message stays fully answered and the
call/reply pairing is intact.

**Run lifecycle.** `RunStatusDto` is closed to `Queued`, `Starting`, `Running`, `WaitingInput`, `Completing`,
`Completed`, `Failed`, and `Interrupted`; `Cancelling` and `Cancelled` are removed with their transition edges.
`Interrupted` is a recovery-only terminal status: no live command produces it, and the daemon recovery pass still
transitions every unfinished run to it before readiness. A run created from accepted input enters `Starting`; `Queued`
remains a declared member whose only successors are `Starting` and `Interrupted`.

**Cooperative tool interruption.** Every workspace tool checks the invocation's cancellation signal before and between
its I/O steps and returns its captured output as a partial/interrupted outcome when stopped; `execute` already streamed
bounded partial output. Each invocation binds a fresh cancellation signal that is removed when no call is in flight. No
tool waits for an acknowledgment, and no interrupted call is retried.

**Dead machinery.** `RuntimeService`/`RuntimeValuesDto`, `ModelRunFirstAppendGate`, and the daemon gate and
cancellation-terminalizer machinery are removed. `queue_promotion_required` and the queue tables, columns, tickets,
positions, and promotion path no longer exist; the single live schema version 1 drops them in place.

## Rationale

- A user who types while the agent is working expects the message to steer the work already in progress, not to wait
  for a successor run. Joining the live context at the next boundary keeps one continuous conversation and removes the
  queue, its tickets, and the promotion transaction that only existed to order that waiting.
- Durable pending turns keep restart honesty: the messages are recorded input, while no provider request, tool call, or
  external effect resumes.
- Interruption is a better fit than cancellation for a cooperative local system: the captured output is bounded and
  delivered with a notice that tells the model what happened, the run stays live, and no two-step terminal state or
  terminalizer race has to exist.
- Removing `Cancelling` and `Cancelled` from the run vocabulary makes the interruption contract honest: interruption is
  a control signal on one in-flight operation, not a run state, and `Interrupted` stays reserved for the one thing it
  accurately describes, restart recovery.
- Deleting the retired members and tables instead of aliasing them follows the no-backward-compatibility policy and the
  single-version rule.

## Invariants

1. A session has at most one active run. A user turn accepted during that run is recorded pending and never starts a
   second run or a parallel context.
2. Pending turns join the live context exactly once, in durable insertion order, at the next provider-response or
   tool-batch boundary, as `UserMessageAppended` facts; a turn marked `appended` never joins again.
3. A pending turn whose run ended before the join starts the successor run through ordinary acceptance when the session
   has no active run; no pending turn is dropped.
4. `turn.remove` removes only a pending turn that has not joined a context; removal commits `TurnRemoved` and the
   removed turn never joins.
5. Interruption changes no durable run status: the run stays active, records one `InterruptNoticeRecorded` notice for
   the stopped call, resets the signal, and proceeds to its next model step.
6. An interruption never fabricates a final result and never claims rollback: a stopped call commits only its captured
   output and the notice, and the interrupted call is never retried.
7. A tool call interrupted before it starts is answered with the stopped-tool-call notice as a `Partial` result, so no
   effect starts and the assistant tool-call message stays answered.
8. `Interrupted` is produced only by restart recovery; no live command transitions a run into it.
9. The run status set is closed to the eight recorded members; `Cancelling` and `Cancelled` have no producer, consumer,
   alias, or compatibility path.
10. The single live schema version 1 contains no queue table, ticket, or position; the removal is in place, with no
    migration and no second version.

## Compatibility

The change evolves in place under the single live schema version 1 ([ADR
0005](0005-no-backward-compatibility-and-legacy-removal.md)): the `turns` table loses its ticket column and gains the
closed `outcome` set, the queue tables and columns are deleted, and no migration, versioned upgrade step, or second
version exists. A local database file created by an earlier revision is not opened, migrated, or repaired; it is
deleted and recreated by the normal development flow, as the single live schema requires. The queue events
(`UserTurnQueued`, `QueuedTurnRemoved`) and the removed protocol members are deleted, not preserved: no compatibility
fixture, decoder, alias, or golden keeps them readable. M4 model facts, run container-journal bytes, snapshots, and
replay keep their recorded meaning within version 1; the `RunStatusChanged` vocabulary no longer carries
`Cancelling`/`Cancelled`. The protocol version stays 2.0 and the method surface stays one-to-one with the current
commands.

## Security and failure behavior

Interruption and pending input stay typed and fail closed:

- Interruption is a control signal, not effect evidence: it cannot fabricate a result, prove absence, or claim
  rollback, and it never repeats an external action.
- `run.interrupt` is accepted only for an exact active run (`active_run_not_found` otherwise); it is never dispatched
  through the synchronous command path, which rejects it with `invalid_interrupt_dispatch`, and the daemon host owns
  reaching the in-flight operation.
- A stopped tool call commits a bounded `Partial` result normalized and redacted by the same path as a completed
  result; no new bound, quota, or budget is introduced.
- Blank pending content is rejected by typed validation, and `turn.remove` fails with `pending_turn_not_found` when no
  matching pending turn exists; an appended turn is never silently removed.
- Late fragments, late results, and late provider data after an interruption remain non-authoritative and cannot repair
  or reinterpret a committed result.
- No runtime content scanning is introduced ([ADR 0014](0014-limits-by-precedent-and-no-content-scanning.md)).

## Non-goals

No second run or parallel context in one session; no queue, ticket, position, promotion, or queue-ordering authority;
no per-`ToolCallId` interruption command; no `Cancelling` or `Cancelled` run status; no automatic continuation,
resume, or retry after restart; no change to provider retry policy or the tool lifecycle vocabulary; no new bound or
limit; no migration, version bump, or compatibility layer; no edit to closed milestone records, whose queue and
cancellation wording stays history.

## Affected documents

[Decisions index](README.md) lists this record, which supersedes the deleted per-call cancellation direction. [ADR
0004](0004-m5plus-complete-foundation-activation.md), [ADR
0011](0011-local-json-rpc-2-0-transport.md), [ADR
0014](0014-limits-by-precedent-and-no-content-scanning.md), and [ADR
0015](0015-ordering-authorities.md) carry the amended clauses. [Architecture
04](../architecture/04-sessions-runs-events-and-storage.md) owns the run state machine, pending turns, boundary joins,
the event taxonomy, and the required outcome tests; [architecture
README](../architecture/README.md) owns the glossary and the ordering invariants; [architecture
00](../architecture/00-principles-and-scope.md) states the one-active-run and pending-input rule; [architecture
03](../architecture/03-daemon-transport-and-adapters.md) owns the method table and restart semantics; [architecture
20](../architecture/20-ipython-kernel-lifecycle.md) owns kernel interruption and recovery. Secondary cleanup lands in
architectures [01](../architecture/01-workspace-and-crate-map.md),
[02](../architecture/02-dto-and-contract-policy.md),
[08](../architecture/08-model-protocol-and-providers.md),
[09](../architecture/09-configuration-security-and-observability.md),
[10](../architecture/10-test-driven-delivery-and-verification.md),
[11](../architecture/11-implementation-roadmap.md),
[14](../architecture/14-run-execution-meaning-and-historical-compatibility.md),
[18](../architecture/18-mcp-capability-lifecycle.md),
[19](../architecture/19-gateway-rlm-bridge.md),
[21](../architecture/21-goals-skills-context-memory-and-compaction.md),
[22](../architecture/22-provider-evolution-profiles-and-reasoning.md),
[23](../architecture/23-non-destructive-session-branching-and-regeneration.md),
[28](../architecture/28-goal-domain-and-verification.md), and
[29](../architecture/29-provider-session-and-profiles-protocol.md).

## Evidence

The change is accepted only together with: a repository symbol-search receipt showing the removed members
(`queued_turns`, `queue_ticket`, `next_queue_ticket`, `queue_promotion_required`, `QueuePositionDto`, `StopRunCommandDto`,
`RuntimeService`, `RuntimeValuesDto`, and `ModelRunFirstAppendGate`) have no remaining producer or consumer; a
documentation search receipt showing no current-policy statement still requires a queue, ticket, promotion, or
`StopRun` path, naming any closed record deliberately left as history; the domain, storage, runtime, protocol, and
daemon tests that prove started-or-pending acceptance, FIFO boundary joins, pending-turn removal, notice recording and
signal reset, pre-invocation partial answers, cooperative tool interruption, and recovery to `Interrupted`; and the gate
suite passing. Gates: `make quick`, `make verify`, `docs-check`, `make architecture`, Linux/Windows CI.

The retired names survive only as historical prose: the queue-and-cancellation wording of the closed M3/M4/M5
milestone records ([M3](../closeout/m3-closure-evidence.md), [M4](../closeout/m4-closure-evidence.md),
[M5](../closeout/m5-closure-evidence.md)); the removal ledgers of [ADR
0005](0005-no-backward-compatibility-and-legacy-removal.md); the reverted Slice 2 descriptions in ADR 0004;
the superseded text of ; the historical
audit [`complexity-audit.md`](../complexity-audit.md); and the preserved research tree under
[`../../reference/`](../../reference/README.md). Editing a historical record to hide a name it reported would destroy
the record.
