# ADR 0015: Ordering authorities

## Status

Accepted 2026-10-05. It collapses the declared sequence/cursor family to two durable ordering authorities plus one
observation position, and it deletes the dead members of that family. This record authorizes no new sequence, cursor,
DTO family, storage mechanism, or error code.

## Scope and supersession

In scope: the normative sequence/cursor table of [architecture README](../architecture/README.md), every
sequence-independence clause of the records below, and the dead pagination, session-tail, replay, and snapshot-sequence
members of `intention-types`, `intention-protocol`, and `intention-domain`.

Out of scope: the run replay and run tail publication bounds, and every behavior of
the ordinary runtime.

| Record | Superseded clause | Replaced by |
| --- | --- | --- |
| [ADR 0003](0003-production-model-tool-loop.md) 65-67 | "a new fact type does not create a new sequence merely for convenience; a separate sequence is permitted only for an independent aggregate with dedicated bounded queries" | Invariant 1: no record family may introduce a further ordering sequence; it orders by the session event sequence or by its container's journal, or it has no durable order |
| [ADR 0004](0004-m5plus-complete-foundation-activation.md) 85-86 | "M5+ introduces no second runtime, registry, scheduler, persistence authority, or sandbox" | The container journal is a storage mechanism inside the existing single persistence authority, not a second one |
| [ADR 0011](0011-local-json-rpc-2-0-transport.md) 85 | "`session.subscribe` and `session.snapshot` return their snapshot-and-tail result" | They return their snapshot result, or a typed resync |
| [ADR 0011](0011-local-json-rpc-2-0-transport.md) 122 | "M3/M4/M5 durable runs, sessions, events, snapshots, cursors, and storage bytes are untouched by the wire change" | "Cursors" now means exactly the ordering model of this record; the wire change still touches none of it |
| [ADR 0012](0012-typed-serde-json-contracts.md) 84-85 | "M3/M4 bytes, runs, events, snapshots, and cursors keep their recorded meaning and are never re-encoded, rewritten, or synthesized" | Recorded bytes and meanings are preserved; the removed write-only snapshot `sequence` columns carried no recorded meaning, and "cursors" means the two-authority model |
| [ADR 0014](0014-limits-by-precedent-and-no-content-scanning.md) 93-94 | "Live queue unchanged. M3 queue tickets and atomic promotion stay exactly as they are" | Invariant 5: queue tickets are a queue-ordering mechanism outside the ordering model; that mechanism was later removed by [ADR 0019](0019-pending-turns-and-cooperative-interruption.md) |

Each record's text stays as written; this record supersedes only the named clause.

## Decision

Two durable ordering authorities exist, plus one observation position:

| Authority | Scope | Orders | Never used for |
| --- | --- | --- | --- |
| Session event sequence (`SessionEventSequenceDto`) | one session | every record committed in the session: events, run lifecycle, tool lifecycle, model and tool facts | container records, observation positions |
| Container journal sequence (storage mechanism `container_journals`, first and today only kind `run`; the run's position type is `RunEventCursorDto`) | one container: a run today; a conversation tree when activated | records that belong to the container and are not a record of one session | session records, observation positions, identity |
| Observation cursor (reserved name; no DTO until a producer exists) | one reader (an adapter or the local user) | nothing: it is a resume position | never an authority, never durable order, never a dedup key, never a conflict token |

Mapping of the declared rows:

| Today | Becomes |
| --- | --- |
| Session event sequence (arch 04) | authority 1, unchanged |
| Run event cursor (arch 04) | authority 2, container kind `run` |
| lineage sequence (arch 23) | authority 2, container "conversation tree", when activated |
| notification cursor (arch 03) | row 3, observation position |
| Skill sequence (arch 21:243, not in the table) | authority 1: session records, no separate sequence |

The dead members of the family are deleted in this change: `PageCursorDto`, `PageRequestDto`, `SessionEventTailBatchDto`
with `next_after_sequence`, `RunReplayDto::tail`, and the write-only `sequence` columns of `session_snapshots`,
`run_snapshots`, and `model_run_snapshots`. `RunEventTailPageDto` and `load_run_tail` stay because the daemon's live
publication uses them.

## Rationale

- Only two ordering mechanisms exist in code today: the session event sequence and the per-run journal cursor. Five of
  the seven declared rows belong to unbuilt systems, a ninth Skill sequence was never in the table, and the run cursor
  is the first instance of a general container mechanism rather than a peer of the session sequence.
- One storage mechanism with per-container journals keeps the model honest for the systems that will need it: a
  conversation tree orders its own records and nothing else, with no fourth option invented later.
- The observation cursor is named and reserved before any producer exists so a reader position can never drift into
  being treated as durable order, dedup key, conflict token, or identity.
- Deleting the unowned sequences and the dead members keeps the normative set identical to what code and planned work
  actually own, per the no-backward-compatibility policy.

## Invariants

1. No record family may introduce a further ordering sequence. A record family either orders by the session event
sequence, or it belongs to exactly one container and orders by that container's journal, or it has no durable order.
There is no fourth option.
2. A container journal is dense within its container and is the only gap-detection token for that container's records.
The adjacency (`+1`) contracts stay in force where they exist today.
3. The container-scoped optimistic append token (`expected_cursor`) stays container-local: a write to another container
or to the session sequence must never invalidate it.
4. Cross-authority correlation uses typed identity only: no arithmetic, offsets, conversions, or authority
substitution (extends [architecture 02](../architecture/02-dto-and-contract-policy.md), lines 41-42).
5. No queue-ordering mechanism exists: the M3 queue, its tickets, and their promotion were removed by [ADR
0019](0019-pending-turns-and-cooperative-interruption.md), and pending turns are durable input joined to the live run
context rather than an order.

## Compatibility

Ordering evolves in place under the single live schema version 1: no migration, no versioned upgrade step, and no
second version ([AGENTS.md](../../../AGENTS.md), single-version rule). Recorded M3/M4 bytes and meanings are preserved;
this change removes a representation and dead members, not a version, mirroring [ADR
0012](0012-typed-serde-json-contracts.md). The replay and page bounds and the run tail publication are unchanged; the
queue and its tickets were later removed by [ADR
0019](0019-pending-turns-and-cooperative-interruption.md). No compatibility fixture, decoder, alias, or golden is kept
for a removed member.
A local database file created by an earlier revision is not opened, migrated, or repaired: it is deleted and recreated
by the normal development flow, as the single live schema requires.

## Security and failure behavior

Cursor and ordering failures stay typed and fail closed before any effect:

- Surviving: `invalid_event_tail_position` (a session event tail position beyond the durable session sequence),
  `invalid_run_event_cursor` (an unusable run container journal position), `run_event_cursor_conflict` (the
  container-scoped optimistic append token no longer matches), `run_history_unavailable`, `run_replay_not_found`, and
  `run_fact_too_large` (durable run history, replay identity, and fact bounds).
- Deleted with their producers: `invalid_page_cursor` and `invalid_page_limit` (the pagination members),
  `invalid_event_tail` and `invalid_subscription_response` (the session tail batch and its snapshot-and-tail response),
  and `invalid_run_replay` (the removed replay tail/snapshot coherence check).
- No new error code, retry class, or failure category is introduced. No removed sequence can fail at runtime because
  no producer or consumer remains, and nothing falls back to arithmetic, offsets, or authority substitution.

## Non-goals

No new sequence, cursor, DTO, wire field, configuration, storage mechanism, or error code beyond the recorded container
journal; no activation of any container kind beyond `run`; no queue, ticket, or promotion; no
change to replay, page, or fact bounds; no
migration, version bump, or compatibility layer; no reuse, renumbering, or arithmetic conversion of any ordering token;
no edit to closed milestone records, whose sequence wording stays history.

## Affected documents

[Architecture README](../architecture/README.md) owns the normative table and now carries the two authorities and the
observation position; [architecture 02](../architecture/02-dto-and-contract-policy.md) restates the cross-authority
correlation rule; [architecture 04](../architecture/04-sessions-runs-events-and-storage.md) owns the session event
sequence and the run container journal (kind `run`); [architecture
21](../architecture/21-goals-skills-context-memory-and-compaction.md) removes the standalone Skill sequence;
[architecture 23](../architecture/23-non-destructive-session-branching-and-regeneration.md) orders lineage in the
conversation-tree journal; and [architecture 03](../architecture/03-daemon-transport-and-adapters.md) records the
notification cursor as an observation position and projects the flat activity journal over the run container journal.
Secondary mention
cleanup lands in architectures [01](../architecture/01-workspace-and-crate-map.md),
[03](../architecture/03-daemon-transport-and-adapters.md),
[08](../architecture/08-model-protocol-and-providers.md), [11](../architecture/11-implementation-roadmap.md),
[15](../architecture/15-tool-registry-and-model-tool-loop.md),
[19](../architecture/19-gateway-rlm-bridge.md),
[22](../architecture/22-provider-evolution-profiles-and-reasoning.md), and
[30](../architecture/30-instruction-sources-and-system-context.md), and in
[`quality/architecture.toml`](../../../quality/architecture.toml).

## Evidence

The change is accepted only together with: a repository symbol-search receipt showing the removed members
(`PageCursorDto`, `PageRequestDto`, `SessionEventTailBatchDto`, `next_after_sequence`, `RunReplayDto::tail`, and the
write-only snapshot `sequence` columns) have no remaining producer or consumer; a documentation search receipt showing
no current-policy statement still requires a removed sequence, naming any closed record deliberately left as history;
unchanged replay and page bounds (strict-after contiguity, 256 facts, 512 KiB canonical fact data, `has_more`); and the
gate suite passing. Gates: `make quick`, `make verify`, `docs-check`, `make architecture`, Linux/Windows CI.

The symbol receipt is exact for `crates/`, `docs/intention-relay/`, `quality/`, the `Makefile`, and `.github/`. The
removed names survive only in the preserved research tree `docs/reference/`, which is exempt because editing a
historical record to hide a name it reported would destroy the record. The root `architecture-fitness-audit.md` of the
earlier audit also named them; that orphaned artifact was deleted, so nothing else in the tree does.

The closed-set guard is a name-level check: `quality/check_architecture.py` rejects any production type declaration of
any visibility, including aliases, whose name ends in a declared ordering suffix and is not in the declared set, and it
rejects a declared authority that no production source defines. It carries a self-test proving it can fail. It does not
inspect fields, SQL columns, or documentation, so an ordering field or column added without a new type remains a review
responsibility stated here rather than a checked invariant.

## Research provenance

The decision came from a seven-zone complexity audit plus four focused read-only investigations: live counter
machinery, the six unbuilt sequences, the normative framework, and the replay/ordering requirements. No session
internals are named.
