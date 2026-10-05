# ADR 0048: Limits by precedent, no runtime content scanning, and removal of the corridor, period, and queue-audit engines

## Status

Accepted 2026-09-30. It fixes the numeric-limit policy, bans runtime content scanning, and removes the corridor and
reservation model, the Day/Week/Month period engine, and the queue audits from the documentation because they never
existed in code. It authorizes no new limit, scanner, corridor, period engine, or queue audit.

## Scope and supersession

In scope: numeric limits in contracts and runtime, the runtime credential-shaped content scanners, and the
documentation-only corridor, reservation, calendar-period, and queue-audit surfaces.

| Record | Superseded clause | Replaced by |
| --- | --- | --- |
| [ADR 0022](0022-programmatic-caller-policy-directions.md) | `InteractiveLocalReadBaselineV1` (256/16); the bounded `ProgrammaticAuthorizationCorridorDto`; "run and calendar limits with atomic reservations"; the corridor and reservation invariants; and the corridor, counter, reservation, and limit failure wording | The remaining programmatic-caller policy and admission direction stands without limits, corridors, reservations, or calendar counters |
| [ADR 0025](0025-base-tool-contracts-and-tool-loop-bounds.md) | The 16-call tool-group maximum and `provider_tool_group_invalid`; the "first-scope bounds" (512 KiB per canonical fact, 4 MiB per group, `tool_output_limit_exceeded`); the 256-fact and 512-KiB replay page bounds; and the `model_tool_loop_required` gate (also removed with the capability plane by [ADR 0045](0045-local-json-rpc-2-0-transport.md)) | The qualitative fragment, descriptor, and terminal-taxonomy contracts stand; numeric bounds exist only by recorded precedent |
| [ADR 0034](0034-accepted-m5plus-retained-deferral-directions.md) | Item 5, the future classification of activity numeric values as intrinsic bounds, capacity availability, or ordinary product policy | A numeric activity bound is introduced only with a recorded precedent, under the rule below |
| [ADR 0035](0035-m5plus-complete-foundation-activation.md) | The Slice 2 queue-promotion and reconciliation wording and the Slice 3 "corridors and reservations" wording | The Slice 2 revert and the precedent rule below |

The fixed-limit rows of the removed Slice 1 ledger (activity, run, child, and tool-loop limit records) are removed with
the ledger; no limit record remains. The unavailable-queue promotion and reconciliation, held-run admission, and related
queue audits were reverted (Slice 2 revert); this record removes their documentation remnants.

## Decision

### Limits only by real precedent

1. A numeric limit may exist only when a real precedent justifies it: a
concrete failure mode, a resource that must be bounded, or an external constraint. Contract vocabulary alone never
justifies a limit.
2. Speculative contract limits are removed wherever they exist: the 256 and
512 character caps, the contract quantity and aggregate caps, the 16-call tool-group maximum and its
`provider_tool_group_invalid` outcome, the frozen `Fixed*Limits` records, digest-format checks, the tool-result quantity
caps (`MAX_GLOB_MATCHES`, `MAX_GREP_MATCHES`, `MAX_GREP_FILES`) with their truncated-result behavior, the execute
argument caps (`MAX_EXECUTE_ARGUMENTS`, `MAX_EXECUTE_ARGUMENTS_TOTAL_BYTES`), and the activity, run, and child ceiling
families. They are deleted, not renumbered or relocated.
3. The keep-list of liveness safeguards is retained, each with its reason:

   | Safeguard | Reason |
   | --- | --- |
   | Transport message-size cap (`MAX_MESSAGE_BYTES`, 1 MiB) | An unbounded message lets a peer force unbounded allocation or a hang |
   | Connect and synchronous IO timeouts (`CONNECT_TIMEOUT`, `SYNC_IO_TIMEOUT`) | Bound a stalled socket or a peer that stops reading |
   | Stale-socket probe | Allows reclaim after a crashed daemon without manual cleanup |
   | Storage read bounds (`MAX_TAIL_FACTS`, `MAX_*_BYTES`) | Bound replay and page reads to what the storage can serve safely |
   | Tool read and output windows (`MAX_TOOL_OUTPUT_BYTES`, `MAX_EDIT_TARGET_BYTES`, `MAX_GREP_AGGREGATE_BYTES`) | Bound one tool read or one rendered result so a single large file or match set cannot force unbounded allocation or a hang |
   | Durable assistant-content bound (`MAX_ASSISTANT_CONTENT_BYTES`) | Bounds one durable fact and the runtime chunk that emits it; long assistant text is fragmented at this size |
   | Process timeout window (`EXECUTE_TIMEOUT`, 30 s) | Bounds a child process that stops producing progress or never exits |
   | Process drain window (`READER_DRAIN_GRACE`, 5 s) | Bounds draining a child's output after the timeout, so the loop cannot hang on a silent pipe |
   | Model progress and retry timeouts | Bound a live provider request that stops producing progress |

4. A future limit returns only with its precedent recorded in an ADR or an
architecture section: the failure mode it prevents, the unit, the bound, and the behavior at the bound. No limit is
added because a number looks prudent.

### No runtime content scanning

5. The runtime scanners of credential-shaped content are removed with their
files: `credential_shaped_identifier`, `bearer_token_shape`, and every `credentials_forbidden` validator, together with
their tests.
6. The ban is policy: no daemon, client, tool, or provider code scans user,
model, repository, or tool content for credential-like shapes. Content scanning is not a security control: it cannot be
exhaustive, it rejects legitimate content that resembles a credential, and it suggests a guarantee it cannot provide.
Credential safety stays structural: credentials never enter public or durable surfaces, remain non-serde and
non-`Debug`, and are redacted at the boundary.
7. The surviving secret hygiene is the CI documentation secret scan
(`quality/check_docs.py`) and the fake-secret absence tests over public and durable surfaces. They guard the repository
and the recorded state; they are not runtime content scanners.

### Corridors, periods, and queue audits are removed from the documentation

8. The corridor and reservation model, its calendar counters, and the
Day/Week/Month period engine never existed in code: a repository search finds zero occurrences. They are removed from
the documentation instead of being kept as unimplemented direction, and ADR 0022 above is amended accordingly.
9. The unavailable-queue promotion and reconciliation, held-run admission,
and related queue audits exist only in documentation after the Slice 2 revert (`AdmitRecoveredRun`). They are removed
from the documentation. The live M3 turn input has since been replaced by pending turns joined to the live run context
by [ADR 0055](0055-pending-turns-and-cooperative-interruption.md).
10. This removal is documentation-only: no queue behavior changes, and no new
queue reconciliation, held-run admission, or audit surface is created.

## Invariants

1. Precedent rule. Every numeric limit has a recorded precedent naming the
failure mode it prevents; a limit without one is removed.
2. Closed liveness list. The safeguard classes above — transport, socket,
storage read, tool read and output, durable content, process timeout and drain, and model progress — are the only
numeric safeguards retained; a new safeguard or a changed value requires its recorded reason.
3. No content scanning. No runtime path scans content for credential-like
shapes, and no heuristic replaces the removed scanners.
4. No corridors or periods. No corridor, reservation, calendar counter, or
Day/Week/Month period model exists in code or documentation.
5. No live queue. Pending turns are durable input joined to the live run
context at the next boundary; the M3 queue, its tickets, and its promotion were removed by [ADR
0055](0055-pending-turns-and-cooperative-interruption.md).
6. No replacement bureaucracy. A removed limit is not replaced by a warning,
a soft cap, a counter, or a periodic audit.

## Compatibility

M3/M4 durable behavior, replay, and storage bytes are unchanged; the M3 queue, its tickets, and its promotion were
later removed by [ADR 0055](0055-pending-turns-and-cooperative-interruption.md). The removed limits were
either speculative contract vocabulary never enforced on the live path or clauses of the reverted Slice 2 surface. Where
a real bound exists on a live path and is retained, its value is unchanged. No protocol, DTO, configuration, or storage
schema version changes.

## Security and failure behavior

Removing the runtime content scanners removes a misfiring rejection path: legitimate content is no longer rejected
because it resembles a credential, and a credential-shaped string is no longer treated as proof of anything. The
structural credential rules remain: credentials stay out of public and durable surfaces, remain non-serde and
non-`Debug`, and are redacted at the boundary. The retained liveness caps still bound resource exhaustion: a malformed
or over-size transport message, a stalled socket, and an unbounded storage read remain rejectable.

## Non-goals

No new limits, quotas, or scanners; no heuristics, entropy checks, or allowlists instead of content scanning; no
corridor, reservation, calendar period, or quota engine; no queue reconciliation, held-run admission, or queue audit; no
change to the live M3 queue; no change to a retained liveness safeguard's value without a recorded precedent; no change
to the fake-secret tests or the CI documentation secret scan.

## Affected documents

[ADR 0022](0022-programmatic-caller-policy-directions.md), [ADR 0025](0025-base-tool-contracts-and-tool-loop-bounds.md),
[ADR 0034](0034-accepted-m5plus-retained-deferral-directions.md), and [ADR
0035](0035-m5plus-complete-foundation-activation.md) are amended as recorded above. [Architecture
27](../architecture/27-programmatic-caller-policy-and-admission.md) owns the programmatic-caller policy that loses the
limit, corridor, and period clauses; [architecture 15](../architecture/15-tool-registry-and-mandate-tool-loop.md) owns
the tool-loop contracts that lose the numeric group and output bounds; [architecture
24](../architecture/24-activity-ui-and-adapters.md) owns the activity surface whose numeric classification is
superseded; [architecture 04](../architecture/04-sessions-runs-events-and-storage.md) owns the live M3 queue that stays
unchanged; [architecture 09](../architecture/09-configuration-security-and-observability.md) owns redaction and the
surviving secret hygiene; [architecture 10](../architecture/10-test-driven-delivery-and-verification.md) and
[architecture 12](../architecture/12-quality-gates-and-makefile.md) own the removal of the scanner and limit tests from
the gate surface; and [`AGENTS.md`](../../../AGENTS.md) records the content-scanning ban.

## Evidence

The policy change is accepted only together with: a repository search receipt showing zero occurrences of
`credential_shaped_identifier`, `bearer_token_shape`, `credentials_forbidden`, `MAX_GLOB_MATCHES`,
`provider_tool_group_invalid`, and the `Fixed*Limits` records, plus a named list of the surviving keep-list sites with
their reasons; a documentation search receipt showing no corridor, reservation, calendar-period, queue-reconciliation,
or held-run admission direction remains; the surviving fake-secret absence tests and the `quality/check_docs.py` secret
scan passing unchanged; re-verified coverage tiers after the deletions; and an unchanged live `make e2e-real-api` path,
where the retained transport and storage safeguards still apply. Gates: `make quick`, `make verify`, `docs-check`,
Linux/Windows CI.

## Research provenance

The cleanup-branch audit that inventoried every numeric bound and found most of them speculative; the project policy
against unbounded bureaucracy in [ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md); the finding that the
corridor, reservation, calendar-period, and queue-audit engines existed only in documentation; and the two real guards
that remain, namely transport and IO liveness and repository secret hygiene.
