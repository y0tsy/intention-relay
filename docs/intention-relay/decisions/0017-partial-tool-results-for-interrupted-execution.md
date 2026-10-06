# ADR 0017: Partial tool results for interrupted execution

## Status

Accepted 2026-10-05. It replaces the retired external-effect-unknown outcome and its blocking and pause semantics with
a bounded `Partial` tool result, and it deletes the removed members. This record authorizes no lifecycle member beyond
`Partial`, no new DTO family, no storage mechanism, and no error code.

## Scope and supersession

In scope: the tool lifecycle, result, and outcome vocabularies; the interruption outcome of a started tool call; the
bounded captured output; the daemon interruption and recovery notices; the interruption codes; and the durable partial
tag.

Out of scope: ordinary run lifecycle, provider retry policy, tool-history replay, tool-group publication, the restart
recovery transition to `Interrupted`, and every behavior of the ordinary runtime.

| Record | Superseded clause | Replaced by |
| --- | --- | --- |
| [ADR 0001](0001-rust-owned-capability-plane-and-fixed-tool-registry.md) 27-28 | The invariant that required a pause and exact reconciliation for an unknown started effect | A started effect without terminal proof commits a bounded partial result |
| [ADR 0002](0002-build-autopilot-and-plan-focus-continuity.md) invariant 13 | A started operation without terminal proof classified as the retired outcome, never automatically retried, resumed, or treated as rolled back | A started operation interrupted or lost before a final result commits a bounded partial result and permits the next model step; it is never automatically retried, resumed, or treated as rolled back |
| [ADR 0002](0002-build-autopilot-and-plan-focus-continuity.md) 71-72 | The rule list that kept unknown-effect rules authoritative | The list drops the retired rule: one-active-run, append-only history, commit-before-effect, and no-resume rules remain authoritative |
| [ADR 0002](0002-build-autopilot-and-plan-focus-continuity.md) 20-21 | The activation-scope clause listing unknown-effect boundaries among the preserved boundaries | The list keeps one-active-run, no-resume, commit-before-effect, redaction, and DTO-only boundaries |

The named clause of each record is amended in place; every other clause of those records stays as written. The
superseded external-attempt-evidence record is deleted from the corpus, and its text stays in git history.

## Decision

The tool lifecycle vocabulary is closed:

| Value | Members |
| --- | --- |
| `ToolLifecycleStatusDto` | `Admitted`, `Rejected`, `Started`, `Completed`, `Failed`, `Cancelled`, `Partial` |
| `ToolResultStatusDto` | `Completed`, `Failed`, `Cancelled`, `Partial` |

`ToolResultStatusDto` maps one to one onto the matching `ToolLifecycleStatusDto` terminal member. The lifecycle
transition set is closed: none to `Admitted`; `Admitted` to `Cancelled`, `Started`, or `Rejected`; `Started` to
`Completed`, `Failed`, or `Partial`.

`ToolResultOutcomeDto::Partial { content }` records one execution that was interrupted before a final result. `Partial`
requires non-blank content, exactly like `Succeeded`. An interrupted execution commits a `Partial` lifecycle fact and a
`Partial` result whose content is the output captured before the interruption. `Partial` never terminalizes the run:
the next model step proceeds, and the interrupted call is never retried.

The bounded captured output uses the existing per-tool output windows with their explicit truncation marker and is
normalized and redacted by the same path as a completed result, so a partial document is bounded exactly like a
completed one. A call that captured no output commits the interruption notice as its complete content.

Two interruption codes are recorded in the durable partial document: `tool_cancelled` when the caller stopped the call,
and `tool_execution_interrupted` when the call lost its process evidence (a deadline, stalled pipes, or a failed wait
probe). The durable tag of a canonical partial document or terminal error is `partial`; the removed tag is not
preserved.

Every interrupted call reaches the model with a bracketed daemon notice. The notice follows the captured output in the
tool-message content that answers the call:

| Case | Notice |
| --- | --- |
| stopped with captured output | `[The tool call was stopped before a final result; the output above is partial.]` |
| lost with captured output | `[The tool call did not receive a final result; the output above is partial.]` |
| stopped without captured output | `[The tool call was stopped before a final result.]` |
| lost without captured output | `[The tool call did not receive a final result.]` |

The model owns the interpretation of the captured output: a `Partial` tool message answers its call exactly like a
completed one and the loop continues. Daemon notices use `ModelRoleDto::Notice`; providers render that role as a
user-role message with the text unchanged.

Restart recovery still transitions an unfinished run to `Interrupted`. For a call whose latest durable lifecycle status
is `Started` with no terminal record, the next run's model context carries one recovery notice per call:

`[The tool call "<tool_id>" did not receive a final result.]`

Recovery writes no new durable record for the call and reports it only through that notice. There is no pause state,
no uncertainty quarantine, and no reconciliation authority for an interrupted effect.

## Rationale

- An interrupted or lost tool call has real captured output. Delivering it bounded to the model, with a notice stating
what is missing, keeps the decision about the output's meaning with the model and keeps the run live instead of turning
a recoverable interruption into a blocking quarantine.
- One closed lifecycle/result vocabulary with a `Partial` terminal member keeps a partial durable document identical in
shape to a completed result and needs no separate uncertainty family, reconciliation command, or pause state.
- Explicit notice texts are required because the model cannot otherwise distinguish "the tool finished and this is
everything" from "the tool was interrupted and this is what was captured".
- Recovery notices are synthesized into the next model context instead of written as new durable records, so restart
recovery stays a read of recorded evidence and never fabricates a terminal outcome.
- Deleting the retired outcome and its tag instead of aliasing it follows the no-backward-compatibility policy and the
single-version rule.

## Invariants

1. `Partial` is a terminal tool lifecycle and result status: nothing follows a `Partial` lifecycle fact for that call,
and no further result is recorded for it.
2. A `Partial` result always carries non-blank content: the bounded captured output, the notice, or the output followed
by the notice.
3. `Partial` never pauses, blocks, or terminalizes anything above the call. The run reaches its next model step when no
cancellation is in force, and no lifecycle transition results from an interrupted effect.
4. An interrupted call is never automatically retried, resumed, reattached, or treated as rolled back.
5. Captured output is bounded by the existing per-tool window and truncation marker; no new bound, quota, or budget is
introduced.
6. Recovery reports an unfinished call only through the recovery notice. It records no synthetic terminal outcome and
does not change the call's recorded lifecycle.
7. The retired outcome, its member name, and its durable tag have no producer, consumer, alias, or compatibility path.

## Compatibility

The tool vocabulary evolves in place under the single live schema version 1: no migration, no versioned upgrade step,
and no second version ([AGENTS.md](../../../AGENTS.md), single-version rule). The retired lifecycle member, its result
member, and its durable tag are deleted, not preserved: no compatibility fixture, decoder, alias, or golden keeps them
readable. A local database file created by an earlier revision is not opened, migrated, or repaired; it is deleted and
recreated by the normal development flow, as the single live schema requires. M3/M4 recorded bytes and meanings are
unchanged; interruption codes, notice content, and the `Partial` statuses are new data within version 1.

## Security and failure behavior

Interruption handling stays typed and fails closed:

- A caller stop is recorded as `tool_cancelled` and a lost call as `tool_execution_interrupted`; both produce the same
bounded `Partial` outcome and differ only in the notice text and the run decision that follows the cancellation.
- Cancellation remains a control signal: it prevents later admissions and model steps and is the reason a stopped call
may have no following model step. The `Partial` result itself neither stops nor continues the run.
- Blank partial content fails construction (`invalid_tool_result_content`), so a partial document can never be empty.
- No new error code, retry class, or failure category is introduced beyond the two interruption codes.
- Recovery never repeats an external action and never asserts rollback; it reads recorded evidence and synthesizes a
notice.
- Late fragments, late results, and late provider data remain non-authoritative and cannot repair or reinterpret a
committed `Partial` result.

## Non-goals

No lifecycle member beyond `Partial`; no new result, outcome, notice, or storage family; no change to provider retry
policy, tool-group publication, tool-history replay, or the run `Interrupted` transition; no change to M3/M4 recorded
behavior; no migration, version bump, or compatibility layer; no pause state, reconciliation command, or uncertainty
quarantine for an interrupted effect; no edit to closed milestone records, whose interruption wording stays history.

## Affected documents

[Decisions index](README.md) lists this record, which supersedes the deleted external-attempt-evidence record. [ADR
0001](0001-rust-owned-capability-plane-and-fixed-tool-registry.md) and [ADR
0002](0002-build-autopilot-and-plan-focus-continuity.md) carry the amended clauses. [Architecture
README](../architecture/README.md) defines **Partial**; [architecture
00](../architecture/00-principles-and-scope.md) states the no-pause rule; [architecture
15](../architecture/15-tool-registry-and-model-tool-loop.md) owns the terminal taxonomy and the
effect boundary. Secondary cleanup lands in architectures
[02](../architecture/02-dto-and-contract-policy.md), [04](../architecture/04-sessions-runs-events-and-storage.md),
[10](../architecture/10-test-driven-delivery-and-verification.md),
[11](../architecture/11-implementation-roadmap.md),
[18](../architecture/18-mcp-capability-lifecycle.md),
[19](../architecture/19-gateway-rlm-bridge.md), [20](../architecture/20-ipython-kernel-lifecycle.md),
[21](../architecture/21-goals-skills-context-memory-and-compaction.md),
[22](../architecture/22-provider-evolution-profiles-and-reasoning.md),
[23](../architecture/23-non-destructive-session-branching-and-regeneration.md),
[28](../architecture/28-goal-domain-and-verification.md).

## Evidence

The change is accepted only together with: a repository symbol-search receipt showing the retired lifecycle member,
result member, and durable tag have no remaining producer or consumer; a documentation search receipt showing the
retired identifier has no remaining hit in `docs/intention-relay/` outside the
closed M5 closure evidence; the notice texts asserted by tests on the partial-result path and restart recovery; and
the gate suite passing. Gates: `make quick`, `make verify`, `docs-check`, `make architecture`, Linux/Windows CI. The
retired name survives as historical prose in the root `architecture-fitness-audit.md`; that record is exempt because
editing a historical audit to hide a name it reported would destroy the record.

The uncertainty and reconciliation machinery was removed together with the retired outcome: [architecture
28](../architecture/28-goal-domain-and-verification.md) no longer lists `ResolveUnknownEffect` as a target operation or
audit-contract standard, and no `VerificationUnknownEffectReconciled` activity record remains. No current document
declares a pause, a pause state, or a reconciliation command for an interrupted effect. Outside this record, only closed
milestone evidence keeps the retired state and member names.
