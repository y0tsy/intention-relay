# PR 24 code-review ledger

## Status and review baseline

This ledger records the findings from three read-only review waves over pull
request 24. Seven reviewers covered twenty-one zones. The reviewed range was:

- base: `b5fa71e1b09f823f28669272ed14a79308ed5f83` (`main`);
- head: `8e99251fd2e2fc3681ef8c4673bb60283977762a`;
- comparison: `origin/main...origin/impl/m5plus-slice2-control-plane`.

The ledger is review evidence, not an accepted architecture source. ADR 0038
intentionally removes backward-compatibility and migration behavior; those
removals are not defects. A finding is retained when it is a concrete defect,
a test or quality blind spot, a live documentation contradiction, or a
structural defect in a surface added or activated by the pull request.

## Severity and status vocabulary

- **P0:** catastrophic data loss, security compromise, or universally broken
  core behavior.
- **P1:** a primary supported workflow is deterministically broken.
- **P2:** a material correctness, lifecycle, security, or operability defect.
- **P3:** a bounded correctness defect, latent integration defect, test gap,
  maintainability defect with concrete consequences, or documentation/policy
  contradiction.
- **Confirmed:** independently re-read against the pull-request head.
- **Corroborated:** reported independently by more than one zone.
- **Needs adjudication:** evidence is concrete, but the intended contract must
  be chosen before implementation.
- **Pre-existing:** present at the base revision; retained because the review
  covered the complete affected subsystem, but it is not attributable to this
  pull request.

## Summary

| ID | Sev. | Status | Finding |
| --- | --- | --- | --- |
| PR24-001 | P1 | Confirmed | Provider-selection digest is globally unique, so a second run on the same selection conflicts; queued runs also lose the selection. |
| PR24-002 | P2 | Corroborated | `provider_profiles_v1` is documented but never negotiated or enforced by the real daemon/client. |
| PR24-003 | P2 | Corroborated | Pending-removal state cannot be resumed after restart and expiry has no production driver. |
| PR24-004 | P2 | Corroborated | Removal acceptance spans multiple commits and fallible in-memory steps without roll-forward. |
| PR24-005 | P2 | Corroborated | Configuration reload can rewrite durable `pending_removal` to `active`. |
| PR24-006 | P2 | Corroborated | Rejected or expired removal candidates make the same removal identity collide on retry. |
| PR24-007 | P2 | Corroborated | Held-run admission and unavailable-queue production are not wired in production. |
| PR24-008 | P2 | Confirmed | Reloaded model/config state diverges from catalog profile and durable selection state. |
| PR24-009 | P2 | Confirmed | Generic-chat terminalizes before reading the standard trailing usage chunk. |
| PR24-010 | P2 | Confirmed, pre-existing | A hard-killed Unix daemon leaves a socket that blocks all automatic restarts. |
| PR24-011 | P2 | Confirmed, pre-existing | Execute can wait forever on pipe-reader threads and does not terminate descendants. |
| PR24-012 | P3 | Confirmed | Mid-execution errors can leave a durable `Running` or `Starting` run without an owner. |
| PR24-013 | P3 | Confirmed | A second executor error after StopRun can leave `Cancelling` without a task or terminalizer. |
| PR24-014 | P3 | Confirmed | A transient terminal-publication read failure can leave live subscribers permanently stale. |
| PR24-015 | P3 | Confirmed | Idempotent turn retry can return an outcome that contradicts the accompanying durable projection. |
| PR24-016 | P3 | Confirmed | Catalog material loads descriptors by revision alone, not by `(kind, revision)`. |
| PR24-017 | P3 | Confirmed | Tombstones are never revoked when an identifier is reintroduced. |
| PR24-018 | P3 | Confirmed | Rebinding a run to a different selection is reported as storage unavailable, not Conflict. |
| PR24-019 | P3 | Confirmed | Held-run admission is not recoverable after commit-before-dispatch failure. |
| PR24-020 | P3 | Confirmed | OpenRouter advertises tool calls but cannot translate a tool-result follow-up round. |
| PR24-021 | P3 | Needs adjudication | OpenRouter fails the whole stream for recognized encrypted/non-textual reasoning detail. |
| PR24-022 | P3 | Confirmed, pre-existing | Directory grep/edit and normalized tool outcomes lack aggregate memory/content bounds. |
| PR24-023 | P3 | Confirmed, pre-existing | Derived serde bypasses `BoundedText` constructor limits at the daemon tool boundary. |
| PR24-024 | P3 | Confirmed | Combined 4 MiB per-run reasoning output limit is not enforced. |
| PR24-025 | P3 | Confirmed | Reasoning aggregate addition can overflow. |
| PR24-026 | P3 | Confirmed | Programmatic-caller policy canonical decoder does not reject the wrong record version. |
| PR24-027 | P3 | Confirmed | Canonical decoders truncate several `u64` values to `u32`. |
| PR24-028 | P3 | Confirmed | Envelope metadata is not covered by its digest. |
| PR24-029 | P3 | Confirmed | Error DTO absent-optional behavior no longer has direct current-shape evidence. |
| PR24-030 | P3 | Confirmed | Generic capability helper reports the execution-meaning error for unrelated base capabilities. |
| PR24-031 | P3 | Confirmed | Required protocol fields retain always-`Some` accessors and unreachable consumer branches. |
| PR24-032 | P3 | Confirmed | Protocol module documentation still claims old payloads decode identically. |
| PR24-033 | P3 | Confirmed | Sync client response path omits the DTO schema-version check used by stream clients. |
| PR24-034 | P3 | Confirmed | Exact-version integration coverage does not exercise minor mismatch or async rejection. |
| PR24-035 | P3 | Needs adjudication | Credential-shape policy is duplicated with different verdicts across boundaries. |
| PR24-036 | P3 | Confirmed | Reload candidate validation duplicates startup parsing and can drift. |
| PR24-037 | P3 | Confirmed | Config carries a hand-written SHA-256 implementation despite the workspace implementation. |
| PR24-038 | P3 | Confirmed | Candidate acceptance/digest DTO surfaces have no production consumer. |
| PR24-039 | P3 | Confirmed | Selection canonicalization version has two different values in application producers. |
| PR24-040 | P3 | Confirmed | `migration_result` is a constant, unread wire field under the no-migration policy. |
| PR24-041 | P3 | Confirmed | Duplicate-removal tests accept any error and do not validate the advertised typed conflict. |
| PR24-042 | P3 | Confirmed | `run_exclusive` error-path test name contradicts the mutation behavior it pins. |
| PR24-043 | P3 | Pre-existing | Quality self-test can overwrite the live coverage metadata report with fixture data. |
| PR24-044 | P3 | Confirmed | Coverage performs a redundant daemon collection for an equivalent feature set. |
| PR24-045 | P2 | Confirmed | ADR 0036 remains a contradictory normative ledger after ADR 0038. |
| PR24-046 | P3 | Confirmed | Reconciliation README still advertises the removed schema 3-to-4 migration. |
| PR24-047 | P3 | Corroborated | ADR 0038 is absent from the decisions index and reconciliation owner map. |
| PR24-048 | P2 | Corroborated | Source-of-truth rows still claim V3 execution-meaning records are live/frozen. |
| PR24-049 | P3 | Corroborated | ADR 0037 ownership still assigns schema-4 migrations. |
| PR24-050 | P3 | Confirmed | Accepted ADR 0022 still references removed execution-meaning V3. |
| PR24-051 | P3 | Confirmed | RUN-003 still assigns ownership of migrations. |
| PR24-052 | P3 | Confirmed | CFG-002 through CFG-006 call implemented evidence “planned”. |
| PR24-053 | P3 | Confirmed | Architecture 25 says reload migrates configuration after migration removal. |
| PR24-054 | P3 | Confirmed | Current storage code comments still use schema-4 terminology. |
| PR24-055 | P3 | Needs adjudication | `https-only` catalog policy conflicts with endpoint validation that permits HTTP. |
| PR24-056 | P3 | Confirmed | Client exposes several commands that the real composition can only reject. |
| PR24-057 | P3 | Confirmed | OpenRouter options declared by the catalog are not applied by production composition. |

## Detailed findings

### PR24-001 — Provider selection cannot be reused across runs

**Severity/status:** P1, Confirmed.

**Evidence:** `crates/intention-storage-sqlite/src/control_plane.rs:163-182`
declares `selection_digest TEXT NOT NULL UNIQUE`; `insert_selection` at
`control_plane.rs:1501-1544` rejects an identical digest associated with a
different run. The immediate-start branch inserts a selection at
`crates/intention-storage-sqlite/src/lib.rs:753-757`, while the queued branch
at `lib.rs:790-817` and `promote_oldest_queued_turn` at `lib.rs:461-512` do not
persist one.

**Failure scenario:** Complete run R1, then send another turn using the same
active profile. The resolved selection bytes are identical, so R2 aborts with
`provider_selection_digest_conflict`. If the second turn was queued instead,
its selection is discarded and the promoted run has no durable selection.

**Violated invariant:** Every fresh run has exactly one durable resolved
selection; identical content may be associated with multiple run identities.

**Why tests miss it:** Real-SQLite tests vary model/profile data per run or
exercise one run. Application fakes do not model the global digest uniqueness.

**Fix direction:** Make `run_id` the unique identity; make the digest
non-unique or unique only with `run_id`. Persist the queued turn's selection
and transfer it in the promotion transaction. Add a two-sequential-runs test
and a queue-to-promotion test using the same profile.

### PR24-002 — Provider-profile capability is never negotiated

**Severity/status:** P2, Corroborated by Z04, Z08, and Z18.

**Evidence:** `crates/intention-daemon/src/lib.rs:1155-1166` omits
`ProviderProfilesV1` from `daemon_hello`; `serve_async_connection` at
`:918-930` discards the remote hello; `gated_command_result` at `:999-1030`
checks readiness only. `crates/intention-client/src/lib.rs:52-55,649-658`
requires only baseline capabilities. `require_provider_profiles` in
`crates/intention-protocol/src/negotiation.rs:111-128` has no production
caller. Catalog status hardcodes negotiated state at
`crates/intention-application/src/session_selection.rs:840`.

**Failure scenario:** A peer that omits `provider_profiles_v1` can execute the
entire Slice 2 control-plane surface. Conversely, a compliant client that
checks the real daemon hello must reject every Slice 2 call because the daemon
never advertises the capability.

**Violated invariant:** ADR 0037 requires the hello intersection to gate all
dependent effects before they happen.

**Why tests miss it:** Negotiation helpers use synthetic capability sets and
client fixtures advertise the capability; no test uses the real daemon hello
with an unnegotiated peer.

**Fix direction:** Advertise the capability, retain the remote capability set
per connection, gate every dependent command/query, derive status from the
negotiated set, and add negotiated/unnegotiated real-daemon tests.

### PR24-003 — Pending removal cannot recover after restart

**Severity/status:** P2, Corroborated by Z03, Z09, and Z17.

**Evidence:** Startup reconstructs `PendingRemoval` with `expires_at: 0` and
`gate.prepared = None` in
`crates/intention-application/src/provider_catalog.rs:254-310`.
`accept_pending` and `reject_pending` at `:620-775` require `prepared`; a new
prepare is rejected while pending. `expire_pending` has only test callers, and
the storage trait exposes no pending-candidate load/list operation.

**Failure scenario:** Restart while a candidate is pending. Accept/reject can
no longer identify the candidate, re-prepare conflicts, and no production
timer expires it. Provider-backed work remains blocked indefinitely.

**Violated invariant:** Pending-removal is durable and must preserve its
accept/reject/expiry exits across restart.

**Why tests miss it:** Controller tests keep the same in-memory controller;
restart tests reopen only active catalogs; storage tests bypass the gate.

**Fix direction:** Add a durable pending-candidate read contract, rebuild
`PreparedCandidate` at startup, preserve the actual deadline, and drive expiry
from startup plus a bounded daemon timer or from every relevant command.

### PR24-004 — Removal acceptance is not atomic or recoverable

**Severity/status:** P2, Corroborated by Z03, Z09, and Z17.

**Evidence:** `accept_pending` at
`crates/intention-application/src/provider_catalog.rs:620-707` commits removal
acceptance, commits catalog acceptance, builds and activates the registry,
records tombstones, and finally mutates the gate. These are separate fallible
operations and transactions.

**Failure scenario:** Crash after removal acceptance but before catalog
acceptance leaves an accepted candidate under durable `pending_removal`; crash
or error after catalog acceptance but before registry/gate activation leaves
durable Active with an in-memory PendingRemoval gate. Same-operation retry is
not idempotent at each intermediate state.

**Violated invariant:** Acceptance is described as atomic and restart-safe.

**Why tests miss it:** Fault injection covers failures within one SQLite
transaction, not between repository calls or after registry activation.

**Fix direction:** Introduce one storage transaction for candidate acceptance,
catalog activation, tombstones, projections, and audit. Build private material
before commit when safe, or persist an explicit recovery-required state and
roll forward on startup. Make every acceptance step operation-idempotent.

### PR24-005 — Reload clobbers pending removal

**Severity/status:** P2, Corroborated by Z03, Z08, and Z10.

**Evidence:** `commit_configuration_reload` at
`crates/intention-storage-sqlite/src/control_plane.rs:2254-2280` sets status to
`active` whenever `active_catalog_revision_id` is non-null. Reload/edit/rotate
commands are omitted from `provider_affecting_command` at
`crates/intention-daemon/src/lib.rs:1010-1019`.

**Failure scenario:** Commit a non-catalog edit during pending removal. The
candidate row remains pending, but durable catalog status becomes Active.
After restart, the candidate is invisible to readiness and blocks future
candidate creation through its unique pending row.

**Violated invariant:** Only accept, reject, or expiry may leave the closed
pending-removal lifecycle.

**Why tests miss it:** Reload and removal are tested independently; the
status-update `ELSE 'active'` arm is not exercised in a pending-removal state.

**Fix direction:** Preserve non-Active catalog statuses during reload, or gate
all provider-affecting reload/rotation operations while degraded. Add
pending-removal + reload + restart coverage.

### PR24-006 — Closed removal candidate identities collide on retry

**Severity/status:** P2, Corroborated by Z09 and Z13.

**Evidence:** `prepare_candidate` derives `catalog-{applied + 1}` at
`crates/intention-application/src/provider_catalog.rs:518-543`. Reject/expiry
do not advance applied revision. Candidate handle and candidate revision are
unique in `crates/intention-storage-sqlite/src/control_plane.rs:215-256`, and
closed rows remain.

**Failure scenario:** Reject or expire catalog-2, then retry the same removal
while revision 1 is still active. The second insert reuses catalog-2 and fails
with a generic SQLite/storage error forever, including after restart.

**Violated invariant:** Rejection and expiry must permit a corrected or
repeated proposal.

**Why tests miss it:** Application fakes retain only pending rows; storage
tests never create the same identity after close.

**Fix direction:** Allocate candidate identity from a durable monotonic
sequence/max revision, or delete/archive closed candidate identity separately.
Map duplicate constraints to a typed Conflict and test reject/expire then
retry.

### PR24-007 — Held admission and unavailable queue lack production producers

**Severity/status:** P2, Corroborated by Z03, Z14, and Z17.

**Evidence:** Repository-wide callers of
`mark_recovered_run_held_for_daemon` and
`enqueue_unavailable_run_for_daemon` are tests only
(`crates/intention/src/lib.rs:1565-1638,4770,4851,4996`). Recovery can promote
a queued turn to Starting, but daemon startup has no hold/schedule sweep.

**Failure scenario:** Recovery interrupts an active run and promotes its queued
successor to Starting. The successor has no task and no held row; admission
cannot find it and startup never schedules it. New turns queue behind it.

**Violated invariant:** Every recovery-promoted run is either durably held for
explicit admission or automatically scheduled exactly once.

**Why tests miss it:** Held and queue state are injected through test-only
facade methods; storage recovery tests stop after asserting Starting.

**Fix direction:** Persist the hold as part of recovery/promotion, then expose
and admit it; or run a startup sweep that schedules all non-held Starting
runs. Wire unavailable enqueue at the actual provider-unavailable decision or
remove the inactive queue surface.

### PR24-008 — Reload model diverges from catalog authority

**Severity/status:** P2, Confirmed; contract choice must be reflected in docs.

**Evidence:** Reload classification permits `model` changes at
`crates/intention-config/src/control_plane.rs:434-495`; commit advances only
the config snapshot at `crates/intention/src/lib.rs:2074-2090`; startup catalog
activation no-ops once active at `:1110-1130`. Catalog profile identity hashes
the model at `crates/intention-application/src/provider_catalog.rs:1260-1302`.

**Failure scenario:** Reload model B while catalog profile A remains active.
Fresh execution uses model B from the snapshot while selection, profile,
rotation binding, and catalog queries continue to assert model A. Restart may
revert the effective model because active catalog activation no-ops.

**Violated invariant:** Executed model and durable selected-profile model must
describe one authority for a run.

**Why tests miss it:** Composition tests assert only the dispatched snapshot
and intentionally seed a differently named catalog profile.

**Fix direction:** Until catalog replacement is implemented, classify model
and endpoint changes as catalog-affecting and reject them; otherwise prepare
and activate a matching catalog revision atomically with reload.

### PR24-009 — Generic-chat drops trailing usage

**Severity/status:** P2, Confirmed.

**Evidence:** `GenericStreamState::next` at
`crates/intention-provider-generic-chat/src/lib.rs:340-362` calls `finish` as
soon as `terminal_reason` is present; `finish` sets terminal at `:416-433`.
Usage is read only from chunks at `:364-374`. The driver requests
`include_usage` at `:562-565`.

**Failure scenario:** Standard stream order is finish-reason chunk, trailing
usage-only chunk, then end. The adapter terminalizes before polling the usage
chunk, so durable accounting silently reports no usage.

**Violated invariant:** Requested provider usage must be emitted exactly once
before Finished.

**Why tests miss it:** Tests manually process usage before finish; no test feeds
the actual trailing-usage order through `next`.

**Fix direction:** Record finish reason but continue consuming until native
end, accepting/deduplicating usage-only chunks, then emit Finished.

### PR24-010 — Stale Unix socket blocks daemon restart

**Severity/status:** P2, Confirmed, pre-existing.

**Evidence:** Listener bind at `crates/intention-transport/src/lib.rs:237-337`
does not reclaim; listener options disable reclaim/overwrite. Socket cleanup is
Drop-only. `crates/intention-daemon/tests/facade_e2e.rs:101-110` manually
removes the socket after hard kill and documents the problem.

**Failure scenario:** Kill -9, panic abort, or power loss leaves the path. Every
subsequent bootstrap launches a daemon that exits with endpoint-in-use until
manual deletion or runtime-directory cleanup.

**Violated invariant:** Client bootstrap must recover from an unclean daemon
exit without deleting a live daemon's endpoint.

**Why tests miss it:** E2E cleanup removes the stale path before restart.

**Fix direction:** On Unix bind conflict, probe-connect. If no live listener
responds and ownership/path checks pass, remove the stale socket and retry once.

### PR24-011 — Execute timeout does not bound pipe-reader joins

**Severity/status:** P2, Confirmed, pre-existing.

**Evidence:** `bounded_output_with_timeout` at
`crates/intention-tools/src/lib.rs:328-385` performs unconditional reader-thread
joins after direct-child exit and after kill. Spawn does not create a process
group.

**Failure scenario:** A child backgrounds a descendant that inherits stdout or
stderr, then exits. The direct child is reaped, but EOF never arrives. Timeout
and cancellation are no longer checked; StopRun cannot terminate the tool and
the descendant remains alive.

**Violated invariant:** Execute must return within timeout/cancellation bounds
and must not leave its process tree running.

**Why tests miss it:** Fixtures use only foreground children.

**Fix direction:** Create a process group/session, terminate the group, and
make pipe collection deadline/cancellation aware. Add a background-descendant
fixture.

### PR24-012 — Runtime errors can strand a non-terminal run

**Severity/status:** P3, Confirmed.

**Evidence:** Reasoning fact construction and appends propagate `?` from
`crates/intention-runtime/src/lib.rs:558-569,662,886-916`. The daemon handles
executor errors specially only when status is Cancelling at
`crates/intention-daemon/src/lib.rs:235-262`; otherwise it drops the task.

**Failure scenario:** An oversized reasoning event or transient durable append
failure occurs after Running. `execute` returns Err, task ownership is removed,
and no Failed transition is committed.

**Why tests miss it:** Constructor limits and runtime append failure are tested
separately; tests pin Err propagation but not daemon terminalization.

**Fix direction:** Convert semantic/bound failures to durable Failed facts, and
have daemon error handling terminalize any still-active run or retain a
recovery owner.

### PR24-013 — Second cancellation-terminalization failure is dropped

**Severity/status:** P3, Confirmed.

**Evidence:** `crates/intention-daemon/src/lib.rs:235-262` performs one retry
when executor returns Err and status is Cancelling, discards that retry result,
then removes task ownership. The persistent terminalizer is spawned only when
StopRun initially found no task.

**Failure scenario:** StopRun cancels an active task; execution completion fails
twice due to storage errors. Durable status remains Cancelling, but both task
and terminalizer are gone. Later StopRun cannot repeat Cancelling→Cancelling.

**Why tests miss it:** No two-consecutive-failure fixture.

**Fix direction:** If retry fails and reread remains Cancelling, transfer
ownership to `spawn_cancellation_terminalizer`; remove task only after a
terminal reread.

### PR24-014 — Terminal publication has no retry

**Severity/status:** P3, Confirmed.

**Evidence:** `publish_current` at
`crates/intention-daemon/src/lib.rs:469-533` silently returns on replay/tail or
batch errors. Publication is driven by commit observation; terminal commits
have no guaranteed successor.

**Failure scenario:** A transient read error during final Completed/Failed
publication leaves connected subscribers on a stale non-terminal snapshot
until they reconnect.

**Why tests miss it:** No fault is injected at terminal publication.

**Fix direction:** Queue a bounded retry whenever `published` remains behind,
or maintain a per-run publication worker that retries until durable cursor is
delivered/resync is sent.

### PR24-015 — Turn idempotency returns stale outcomes

**Severity/status:** P3, Confirmed.

**Evidence:** `accept_user_turn` at
`crates/intention-storage-sqlite/src/lib.rs:660-713` reconstructs outcome from
the immutable `turns` row and hardcodes Starting. `remove_queued_turn` at
`:852-880` removes only `queued_turns`.

**Failure scenario:** Retry acceptance after queued-turn removal returns
Queued although the accompanying projection has no queued turn. Retry after a
run advanced returns Started/Starting regardless of current status.

**Why tests miss it:** Retries occur only before state advances/removal.

**Fix direction:** Recompute outcome from current `runs` and `queued_turns`; if
the original queued turn was removed, return a typed terminal result/conflict
rather than a ghost position.

### PR24-016 — Catalog descriptor lookup ignores kind identity

**Severity/status:** P3, Confirmed.

**Evidence:** `load_provider_catalog_material` at
`crates/intention-storage-sqlite/src/control_plane.rs:2093-2114` deduplicates
and queries by `descriptor_revision_id` only, although the schema key is
`(kind_id, descriptor_revision_id)`.

**Failure scenario:** Two kinds legally use revision `1`; loader returns an
arbitrary descriptor and silently produces wrong material.

**Why tests miss it:** Fixtures use globally distinct revision strings.

**Fix direction:** Deduplicate and query by `(kind_id, revision_id)` and fail
typed if the pair is absent.

### PR24-017 — Reintroduced identifiers remain tombstoned

**Severity/status:** P3, Confirmed.

**Evidence:** Tombstone sets only grow at
`crates/intention-application/src/provider_catalog.rs:1029-1041`; lookup at
`:790-822` rejects any member. Durable tombstones use insert-ignore keyed by ID.

**Failure scenario:** Remove profile P, then accept a later catalog that
reintroduces P. Lookup still returns `provider_profile_tombstoned` until
restart; later durable removals retain stale revision information.

**Why tests miss it:** Tests cover one-way removal only.

**Fix direction:** Clear active IDs from in-memory tombstones on acceptance and
define durable tombstones as append-only removal events or update to the latest
removal.

### PR24-018 — Different selection rebind has wrong error category

**Severity/status:** P3, Confirmed.

**Evidence:** `insert_selection` at
`crates/intention-storage-sqlite/src/control_plane.rs:1501-1544` prechecks digest
but not existing `run_id`. A different selection for the same run hits the
primary key and is mapped to `storage_unavailable`; the storage contract
requires Conflict.

**Fix direction:** Query by run ID before insert and return a typed
`provider_selection_conflict`; test identical and different rebinds.

### PR24-019 — Held admission cannot recover dispatch-after-commit failure

**Severity/status:** P3, Confirmed.

**Evidence:** `HeldRunService::admit` at
`crates/intention-application/src/session_selection.rs:1027-1096` commits the
admission before dispatch. Same-operation retry returns acceptance without
dispatch.

**Failure scenario:** Commit succeeds, process dies or dispatch fails, retry is
idempotently accepted but the run remains unscheduled.

**Fix direction:** Make scheduling recoverable from durable Admitted+Starting
state at startup and on retry, while preserving exactly-once registration.

### PR24-020 — OpenRouter tool round cannot complete

**Severity/status:** P3, Confirmed.

**Evidence:** `crates/intention-provider-openrouter/src/lib.rs:540-556` rejects
Tool-role messages and drops assistant tool calls, while capabilities at `:287`
advertise tool calls. Runtime constructs those follow-up messages after tool
execution.

**Failure scenario:** Tool executes locally, then round-two translation fails;
run becomes Failed after side effects.

**Why tests miss it:** Adapter tests use text-only requests; runtime tests use
fake drivers.

**Fix direction:** Implement both assistant-tool-call and Tool-role mapping, or
declare the capability unavailable and fail before local tool execution.

### PR24-021 — Non-textual reasoning detail fails a valid stream

**Severity/status:** P3, Needs adjudication.

**Evidence:** `crates/intention-provider-openrouter/src/lib.rs:360-381` fails
when detail text is absent/empty. The SDK represents encrypted and server-tool
detail blocks without text.

**Failure scenario:** A valid answer includes an encrypted reasoning block; the
adapter fails the whole run instead of suppressing the unpublishable block.

**Why tests miss it:** Tests pin failure for a single encrypted block but do not
cover encrypted detail followed by a valid answer.

**Fix direction:** Decide policy explicitly. Recommended: skip recognized
opaque blocks and fail only malformed/unknown shapes; add mixed-stream tests.

### PR24-022 — Tool output and file processing lack aggregate bounds

**Severity/status:** P3, Confirmed, pre-existing.

**Evidence:** Directory grep uses unbounded `std::fs::read` at
`crates/intention-tools/src/lib.rs:1503-1608`; edit uses full `read_to_string`
at `:1325`; per-match caps permit a very large aggregate. Normalized outcome
content has no hard aggregate cap.

**Failure scenario:** Large files and many long matches create hundreds of MB
of allocations, JSON, durable facts, and model context.

**Fix direction:** Use bounded per-file reads, cap edit targets, and enforce an
aggregate normalized-result limit before persistence/model insertion.

### PR24-023 — Serde bypasses tool text bounds

**Severity/status:** P3, Confirmed, pre-existing.

**Evidence:** `BoundedText` derives transparent Deserialize at
`crates/intention-tools/src/lib.rs:757-773`; validation exists only in `new`.
Daemon parses provider JSON directly at
`crates/intention-daemon/src/lib.rs:791-799`.

**Failure scenario:** Provider emits multi-megabyte or NUL-containing write or
execute arguments; parse succeeds and effects run outside documented bounds.

**Fix direction:** Implement validating Deserialize, cap argument count and
total bytes, and test the daemon parse boundary.

### PR24-024 — Per-run reasoning aggregate limit is not enforced

**Severity/status:** P3, Confirmed.

**Evidence:** Limit helpers exist at
`crates/intention-domain/src/model_facts.rs:90-99` and
`reasoning_history.rs:307-313`, but runtime appends each valid per-fact delta at
`crates/intention-runtime/src/lib.rs:886-923` and explicitly defers aggregate
accounting.

**Failure scenario:** Many sub-512-KiB deltas exceed 4 MiB without
`reasoning_output_limit_exceeded`.

**Fix direction:** Track durable aggregate per run at the append authority and
reject before writing the fragment that crosses the bound.

### PR24-025 — Reasoning aggregate addition overflows

**Severity/status:** P3, Confirmed.

**Evidence:** `crates/intention-domain/src/reasoning_history.rs:311` uses
unchecked addition, unlike the checked sibling helper.

**Failure scenario:** `u64::MAX + 1` panics in debug or wraps and bypasses the
limit in release.

**Fix direction:** Use `checked_add` and return the existing limit error.

### PR24-026 — Programmatic policy decoder accepts wrong version

**Severity/status:** P3, Confirmed.

**Evidence:** `ProgrammaticCallerPolicySelectionV1::decode` at
`crates/intention-domain/src/run_execution_meaning.rs:500-503` checks tag but
not version.

**Failure scenario:** Version 3 bytes decode successfully and re-encode as
version 1, violating canonical decode/re-encode stability.

**Fix direction:** Require version 1 and add a wrong-version negative fixture.

### PR24-027 — Canonical integer fields truncate

**Severity/status:** P3, Confirmed.

**Evidence:** `RunExecutionMeaningEnvelopeV1::decode` at
`crates/intention-domain/src/run_execution_meaning.rs:725,730,735` and
`ReasoningHistoryBound::decode` at
`crates/intention-domain/src/reasoning_history.rs:132` cast `u64 as u32`.

**Failure scenario:** Values above `u32::MAX` are accepted as different values;
re-encoding changes bytes or reports a misleading later digest error.

**Fix direction:** Use `u32::try_from` and return `InvalidField`; add boundary
fixtures for every affected field.

### PR24-028 — Envelope digest does not authenticate metadata

**Severity/status:** P3, Confirmed; impact currently test-only.

**Evidence:** Envelope digest verification at
`crates/intention-domain/src/run_execution_meaning.rs:705-712` covers field 5
bytes only; execution kind, tag, record version, and canonicalization version
are outside it.

**Failure scenario:** Metadata can be altered without DigestMismatch, leaving
the same inner record under different advisory metadata.

**Fix direction:** Include fields 1-4 in digest input, or explicitly specify
them as unauthenticated and validate equality against decoded inner bytes.

### PR24-029 — Optional ErrorDto fields lost direct evidence

**Severity/status:** P3, Confirmed test gap.

**Evidence:** The PR removed the only fixture/test decoding absent
`correlation_id` and `detail`, while architecture 10 still claims this current
behavior. `serde(default)` remains at
`crates/intention-types/src/lib.rs:446-465`.

**Fix direction:** Add a current-shape minimal JSON test asserting both fields
decode as None. Do not restore a legacy-version fixture.

### PR24-030 — Capability helper emits unrelated error

**Severity/status:** P3, Confirmed.

**Evidence:** Fallback in
`crates/intention-protocol/src/negotiation.rs:82-84` maps missing baseline
capabilities to `execution_meaning_capability_required`.

**Fix direction:** Define per-capability errors or restrict the helper to the
families for which it has valid errors.

### PR24-031 — Required fields retain optional accessors

**Severity/status:** P3, Confirmed.

**Evidence:** `ProtocolAcceptedDto::result` and
`SessionSnapshotDto::projection` at
`crates/intention-protocol/src/lib.rs:759-764,1049-1054` always return Some;
consumers retain impossible None branches.

**Fix direction:** Return direct references and remove dead branches under the
no-source-compatibility policy.

### PR24-032 — Protocol documentation contradicts required-field tightening

**Severity/status:** P3, Confirmed.

**Evidence:** `crates/intention-protocol/src/lib.rs:83-90` says old payloads
decode identically, but result/projection became mandatory and old fixtures
were removed under ADR 0038.

**Fix direction:** State that current version equality and required fields
replace prior decoding tolerance.

### PR24-033 — Sync client omits DTO schema check

**Severity/status:** P3, Confirmed hardening gap.

**Evidence:** `request_on` at `crates/intention-client/src/lib.rs:667-686`
checks correlation and protocol version only; stream paths at `:758-768` and
`:818-828` also check message schema.

**Fix direction:** Apply the same current DTO schema check and add a malformed
fixture.

### PR24-034 — Exact-version integration evidence misses minor mismatch

**Severity/status:** P3, Confirmed test gap.

**Evidence:** Exact equality is implemented at
`crates/intention-transport/src/lib.rs:677-693`, but integration tests use 2.0
major mismatches and sync paths.

**Fix direction:** Add 1.2-vs-1.1 tests in sync and async daemon/client
negotiation.

### PR24-035 — Credential-shape policy diverges by boundary

**Severity/status:** P3, Needs adjudication.

**Evidence:** Protocol predicate at
`crates/intention-protocol/src/contract_families.rs:44-80` differs from domain
and config predicates at `crates/intention-domain/src/canonical.rs:904` and
`crates/intention-config/src/control_plane.rs:781-812`.

**Failure scenario:** `password`/`secret` forms pass one boundary and fail the
next, while broad `key` substrings can be rejected only on protocol paths.

**Fix direction:** Define one shared policy or explicitly separate raw-TOML,
identifier, and secret-value predicates, then add cross-boundary verdict tests.

### PR24-036 — Reload validation duplicates startup parsing

**Severity/status:** P3, Confirmed structural risk with observable error drift.

**Evidence:** `collect_validation_issues` at
`crates/intention-config/src/control_plane.rs:539-669` mirrors startup
`parse_resolve` at `crates/intention-config/src/lib.rs:362-616`; candidate
acceptance uses the mirror even when parse falls back to the previous snapshot.

**Failure scenario:** A new validation rule added to only one path may accept a
candidate whose resolved snapshot remains unchanged or report different error
codes for identical input.

**Fix direction:** Return structured issues from one parser/validator and make
candidate acceptance depend on that result; add parser/candidate equivalence
property cases.

### PR24-037 — Config duplicates SHA-256

**Severity/status:** P3, Confirmed maintainability/security defect.

**Evidence:** `crates/intention-config/src/control_plane.rs:829-973` implements
SHA-256 privately while `sha2` is already the domain digest implementation.

**Fix direction:** Use the audited crate with an explicit dependency-policy
update, or put a dependency-light shared digest function in the proper owner.
Retain known-answer and cross-implementation tests during migration.

### PR24-038 — Dead candidate acceptance/digest surfaces

**Severity/status:** P3, Confirmed.

**Evidence:** `CandidateAcceptanceOutcomeDto` and `redacted_safe_digest` in
`crates/intention-config/src/control_plane.rs:257-340,502-522` have only test
callers. Application and protocol use separate acceptance projections.

**Fix direction:** Remove them until a production consumer exists, or make the
digest the actual reload transaction identity and verify it end to end.

### PR24-039 — Selection canonicalization version has two values

**Severity/status:** P3, Confirmed.

**Evidence:** `intention-application/src/session_selection.rs:48,366` writes
`"provider-selection-v1"`; `provider_catalog.rs:49,1222`, domain tests,
protocol fixtures, and the ledger use `"1"`.

**Failure scenario:** Semantically identical selection records from catalog
and run-admission paths have different canonical bytes/digests.

**Fix direction:** Define one domain-owned constant and use it in every
producer; add parity coverage.

### PR24-040 — Constant migration-result wire field

**Severity/status:** P3, Confirmed.

**Evidence:** `ReloadTransactionDto.migration_result` at
`crates/intention-protocol/src/contract_families.rs:2925-2972` is always
`"not-applicable"` and is never consumed.

**Fix direction:** Remove the field and literals under ADR 0038's one-version
policy.

### PR24-041 — Removal conflict tests accept any error

**Severity/status:** P3, Confirmed test gap.

**Evidence:** `m5_control_plane_repos.rs:1202-1216` uses only `expect_err`; the
index-name string mapping at `control_plane.rs:2853-2862` does not match normal
SQLite UNIQUE text, so callers receive `storage_unavailable`.

**Fix direction:** Make the conflict deterministic with precheck or
insert-ignore, then assert the exact typed code for pending and same-handle
duplicates.

### PR24-042 — Gate test name contradicts behavior

**Severity/status:** P3, Confirmed test clarity defect.

**Evidence:** `crates/intention-application/src/provider_gate.rs:192-204` test
name says “without mutation”, but it mutates revision to 9, returns Err, and
asserts revision 9 remains.

**Fix direction:** Rename and document mutation-on-error, or add rollback
semantics and change callers accordingly.

### PR24-043 — Self-test pollutes coverage metadata

**Severity/status:** P3, Pre-existing.

**Evidence:** `quality/self_test.py:1393` executes coverage collection with the
real repository root and synthetic `/tmp/fixture` metadata, overwriting the
gitignored live report. Subsequent direct metadata-backed coverage checks fail.

**Fix direction:** Override ROOT to the fixture copy or save/restore the live
report in `finally`.

### PR24-044 — Equivalent daemon coverage is collected twice

**Severity/status:** P3, Confirmed performance issue.

**Evidence:** `quality/run_coverage.py:88-105` appends `--all-features` for the
daemon in every profile, so default and no-default execute effectively the
same daemon feature set under different flag tuples.

**Fix direction:** Normalize semantic feature sets before deduplication, while
preserving distinct report names when CI requires them.

## Documentation and policy findings

### PR24-045 — ADR 0036 contradicts the live one-version ledger

**Severity/status:** P2, Confirmed.

**Evidence:** `docs/intention-relay/decisions/0036-m5plus-slice1-contract-ledger.md`
still records schema 3, v3/v4 execution meaning, 0x020C, compatible-minor
fixtures, `ReservedForSlice2`, and a normative legacy-binding appendix
(:19, :49, :61, :117-118, :133-146, :349-361).

**Consequence:** This Accepted contract ledger directs later work to recreate
mechanisms ADR 0038 and current code prohibit.

**Fix direction:** Amend every stale ledger/table/evidence row to V4 only,
single schema logical version 1, exact current protocol, current tag statuses,
and no 0x020C appendix.

### PR24-046 — Reconciliation README advertises removed migration

**Severity/status:** P3, Confirmed.

**Evidence:** `docs/intention-relay/reconciliation/README.md:99-100` says schema
advances 3 to 4 additively.

**Fix direction:** Replace with single live schema, logical version 1, created
directly on open under ADR 0038.

### PR24-047 — ADR 0038 is missing from indexes

**Severity/status:** P3, Corroborated by Z01 and Z15.

**Evidence:** `docs/intention-relay/decisions/README.md` ends at ADR 0037;
`reconciliation/README.md:167-169` owner map also ends at 0037.

**Fix direction:** Add ADR 0038 to both indexes and extend the index self-test.

### PR24-048 — Source-of-truth matrix retains execution-meaning V3

**Severity/status:** P2, Corroborated by Z01 and Z15.

**Evidence:** `source-of-truth-matrix.md:62-63` SL1-003/004 still claim v3/v4
records and goldens are frozen; SL1-008 also retains preservation/future-schema
evidence.

**Fix direction:** Make V4 the only live record and remove preservation/future
schema evidence from current rows.

### PR24-049 — ADR 0037 still assigns schema-4 migrations

**Severity/status:** P3, Corroborated by Z01 and Z15.

**Evidence:** `decisions/0037-m5plus-slice2-control-plane.md:128` contradicts
the same file's no-migration ledger.

**Fix direction:** Assign current-schema DDL/projections/rows, not migrations.

### PR24-050 — ADR 0022 references removed V3

**Severity/status:** P3, Confirmed.

**Evidence:** `decisions/0022-programmatic-caller-policy-directions.md:35` names
run-execution-meaning v3/v4.

**Fix direction:** Reference only V4 and note ADR 0038 supersession.

### PR24-051 — RUN-003 assigns migrations

**Severity/status:** P3, Confirmed.

**Evidence:** `source-of-truth-matrix.md:28` says storage owns migrations.

**Fix direction:** Assign the single current schema DDL and projections.

### PR24-052 — CFG evidence remains planned after implementation

**Severity/status:** P3, Confirmed.

**Evidence:** `source-of-truth-matrix.md:88-92` CFG-002 through CFG-006 call
reload, rotation, health, discovery, and pricing fixtures planned, while
EVD-049 through EVD-053 mark them Verified.

**Fix direction:** Point CFG rows to verified evidence or mark them superseded
by the corresponding SL2 rows.

### PR24-053 — Architecture 25 retains migration wording

**Severity/status:** P3, Confirmed.

**Evidence:** `architecture/25-configuration-provider-control-plane.md:60`
says reload reparses, migrates, and validates.

**Fix direction:** Change to reparses and validates.

### PR24-054 — Current storage comments say schema 4

**Severity/status:** P3, Confirmed.

**Evidence:** `crates/intention-storage-sqlite/src/control_plane.rs:7-8,271-273,1107`
and `crates/intention-storage/src/lib.rs:977` use schema-4 terminology.

**Fix direction:** Use current-schema/control-plane terminology.

### PR24-055 — `https-only` descriptor conflicts with HTTP acceptance

**Severity/status:** P3, Needs adjudication.

**Evidence:** `intention-application/src/provider_catalog.rs:1157` declares
`https-only`; `intention-domain/src/provider_catalog.rs:146-158,1308` permits
and tests HTTP, including loopback. Generic-chat sends bearer credentials to
the configured base URL.

**Consequence:** Catalog claims a stronger transport policy than enforcement;
HTTP can expose credentials in plaintext once outbound execution is active.

**Fix direction:** Enforce HTTPS for production kinds with a deliberate
test-only loopback exception, or rename the descriptor to the actual policy.

### PR24-056 — Exposed commands have no success path in real composition

**Severity/status:** P3, Confirmed.

**Evidence:** Typed edit reconstruction cannot retain the credential at
`crates/intention/src/lib.rs:854-930`; rotation always uses the unavailable
credential port at `:703-712,2014-2030`; held admission lacks a producer.

**Consequence:** Client methods exist and application fakes prove success, but
the real daemon always rejects them.

**Fix direction:** Either wire the private source/producer in this slice, or
remove/gate the commands behind capabilities the real daemon does not offer.

### PR24-057 — Catalog driver options are not applied by composition

**Severity/status:** P3, Confirmed integration gap.

**Evidence:** Provider option builders are exercised in adapter tests, but
`SelectedProvider::from_startup_material` in `crates/intention/src/lib.rs:434-445`
constructs default drivers and does not apply declared header/reasoning options.

**Consequence:** Catalog projections can advertise option policy that the live
driver silently ignores.

**Fix direction:** Translate validated catalog options into provider-specific
builders at the composition boundary, or remove/defer the advertised options
until that wiring exists.

## Fix-plan assignment

The detailed implementation plans will be added after seven focused planning
reviews. The planned ownership is:

1. storage/run identity, queue semantics, and canonical codec strictness:
   PR24-001, 015, 016, 018, 026, 027, 028, 041;
2. capability, protocol, client, and wire cleanup: PR24-002, 029-034, 040;
3. catalog removal/recovery lifecycle: PR24-003-006, 017, 019, 042;
4. runtime/daemon lifecycle and publication: PR24-007, 012-014, 056;
5. config/catalog/provider integration: PR24-008-009, 020-021, 035-039,
   055, 057;
6. tools/security/quality: PR24-010-011, 022-025, 043-044;
7. architecture/reconciliation documentation: PR24-045-054.

## Consolidated repair plans

The plans below were produced by seven independent light planning reviews and
then consolidated against the same head. They are implementation plans, not a
record that a finding is resolved. Any item that changes ADR 0037 or ADR 0038
scope requires explicit charter adjudication before code removal.

### Plan A — Storage, run identity, and queue semantics

**Ledger IDs:** PR24-001, PR24-015, PR24-016, PR24-018, PR24-026,
PR24-027, PR24-028, PR24-041.

**Decisions:**

- A resolved selection is identified by `run_id`; its digest is a content
  fingerprint and is not globally unique.
- A queued turn stores its selection on `queued_turns` until its run exists;
  promotion transfers that selection into
  `resolved_run_provider_selections` in the same transaction as run creation.
- Idempotent turn replies are recomputed from current `runs` and
  `queued_turns`, not from the immutable acceptance marker.
- Catalog descriptor lookup is keyed by `(kind_id, descriptor_revision_id)`.
- Removal-candidate conflicts are detected explicitly in the transaction, not
  by matching SQLite error text.

**Implementation surfaces and order:**

1. In `intention-storage-sqlite/src/control_plane.rs`, remove `UNIQUE` from
   `selection_digest`; change `insert_selection` to query by `run_id`, return
   success for the identical digest, and return typed
   `provider_selection_conflict` for different bytes. Remove the obsolete
   global-digest precheck and `provider_selection_digest_conflict` code.
2. Add nullable `resolved_selection_json` to the single live `queued_turns`
   schema in `intention-storage-sqlite/src/lib.rs`. Encode the selection during
   queued acceptance, then decode and call `insert_selection` after promotion
   inserts the run. Expose the existing selection JSON codec as `pub(crate)`.
   Add a `ProviderSelection` fault point after transfer so the whole promotion
   transaction can be proven atomic.
3. In `accept_user_turn`, recompute Started using the actual run status. For a
   queued marker, require a current `queued_turns` row and use its ticket. If
   the membership was removed, return typed Conflict (`accepted_turn_removed`)
   instead of a ghost queue position.
4. In `load_provider_catalog_material`, keep the profile's `kind_id`, dedupe on
   the pair, and query `WHERE kind_id=? AND descriptor_revision_id=?`.
5. In `create_provider_catalog_removal_candidate`, precheck candidate handle,
   pending-row existence, and candidate revision in deterministic order. Map
   same identity to `provider_catalog_removal_candidate_conflict` and a second
   pending candidate to `provider_catalog_removal_pending_exists`.
6. Tighten canonical decoding in `intention-domain`: require record version 1
   in `ProgrammaticCallerPolicySelectionV1::decode`; replace every envelope and
   reasoning-bound `u64 as u32` conversion with `u32::try_from` and
   `InvalidField`; then either include envelope metadata fields 1-4 in the
   digest input or validate them as an explicitly unauthenticated duplicate of
   the inner record. The recommended choice is to authenticate fields 1-4 so
   tag/version/kind metadata cannot be changed independently of field 5.

All schema edits modify the single version-1 DDL in place. Existing developer
databases must be recreated; no migration is added.

**Regression evidence:**

- two sequential runs on the same profile both persist the identical
  selection;
- queued selection survives acceptance, promotion, reopen, and a fault rollback;
- retry after queue removal returns Conflict; retry after Running/Failed returns
  Started with the actual status; retry after promotion returns Started;
- two kinds sharing descriptor revision `1` load their own descriptors, and a
  missing exact pair fails typed decode;
- same-run identical rebind is idempotent, different rebind is Conflict;
- duplicate pending and duplicate-handle removal candidates assert exact error
  codes and leave state/audits unchanged;
- wrong programmatic-policy record version rejects; every affected integer
  field rejects `u32::MAX + 1`; metadata tampering produces DigestMismatch;
  canonical decode/re-encode is byte-stable for every accepted boundary value.

Primary targets: `sqlite_contracts`, `m5_control_plane_repos`, SQLite unit
fault tests, `m5_control_plane_rejections`, domain canonical unit tests, and
one real-composition two-run regression.

**Atomic commits:** provider selection identity/rebind; queued selection
transfer; idempotent response recomputation; descriptor pair lookup; typed
removal conflicts; canonical record version/conversion strictness; envelope
digest authentication. Land transfer before idempotent response changes
because both touch turn acceptance. Land integer/version checks before changing
the envelope digest so failures remain attributable.

**Risks/dependencies:** queued-selection transfer is required by Plan D's held
admission. New error codes flow through daemon error mapping. Recreating local
SQLite state is intentional under ADR 0038. Changing envelope digest bytes
requires updating only the current V1 envelope golden and identity evidence,
never retaining the old digest shape.

### Plan B — Capability negotiation, protocol, client, and wire cleanup

**Ledger IDs:** PR24-002, PR24-029, PR24-030, PR24-031, PR24-032,
PR24-033, PR24-034, PR24-040.

**Decisions:**

- The real daemon and client advertise `provider_profiles_v1`; baseline M3
  capabilities remain the minimum for baseline calls.
- Every Slice 2 command/query is capability-gated before effect, except
  `GetProviderCatalogStatus`, which remains readable and reports the actual
  per-connection negotiated value. Plain SendUserTurn is baseline; a turn with
  provider override requires `provider_profiles_v1`.
- Missing capabilities have explicit family-specific errors; no generic
  execution-meaning fallback.
- Protocol/DTO remains the single live 1.1 shape; no version bump or legacy
  fixtures are added.

**Gate inventory:** gate session profile updates, catalog removal accept/reject,
queue reconciliation while retained, held admission, reload/edit/rotation,
catalog/profile/usage/health/discovery/pricing/configuration queries, and
SendUserTurn with an override. Leave daemon health, session snapshot,
CreateSession, RemoveQueuedTurn, StopRun, SubscribeSession, and plain
SendUserTurn on the baseline.

**Implementation surfaces and order:**

1. Remove `ReloadTransactionDto.migration_result` and all constant producers,
   validator entries, and tests. Keep unknown-additive-field tolerance: a wire
   object carrying the removed member may decode by ignoring it, but no current
   producer emits it.
2. Make `ProtocolAcceptedDto::result()` and
   `SessionSnapshotDto::projection()` return direct references. Mechanically
   remove impossible `Some` matching and client `ok_or_else` branches.
3. Replace `require_capability`'s wildcard with explicit baseline and post-M5
   codes. Decide and document names for the four baseline capability errors.
4. Add a current-shape ErrorDto test that omits optional `correlation_id` and
   `detail` and asserts both decode as None; no legacy fixture.
5. Add DTO schema equality to sync `IntentionClient::request_on`, matching the
   stream clients. Add wrong-schema response coverage.
6. Add sync and async 1.2-vs-1.1 negotiation rejection tests on both client and
   daemon sides. Use inline versions, not non-current fixtures.
7. Add `ProviderProfilesV1` to `daemon_hello` and client advertisement. Retain
   the remote hello/capability set in sync and async serve paths. Introduce an
   exhaustive request classifier and reject gated work with
   `provider_profiles_capability_required` before calling the facade. Derive
   catalog status's negotiated bit from that connection.
8. Rewrite protocol module docs to state exact 1.1 equality, required current
   fields, and unknown-additive-field tolerance, not identical decoding of old
   payloads.

**Regression evidence:** real daemon negotiated/unnegotiated E2E; exhaustive
gate classifier; real daemon hello inventory; plain-vs-override SendUserTurn;
catalog status true/false by peer; current minimal ErrorDto; required field
negative decoding; sync wrong-schema response; all sync/async minor mismatch
paths; migration field absent from serialized current output.

**Docs/policy:** ADR 0037 capability semantics, SL2-002 and EVD-048 evidence,
ADR 0036/0037 error-code lists, and ADR 0038 executed-removal wording for
`migration_result`. Coordinate these edits with Plan G.

**Atomic order:** wire-field removal; accessor/current-shape cleanup;
capability error cleanup; ErrorDto/schema evidence; transport tests; then the
behavioral capability gate last so all fixtures are ready.

**Risks/dependencies:** capability and degraded gates must have a defined error
order; capability failure should be observed before readiness failure. Every
fixture that performs gated work must advertise the capability. Wide accessor
changes are compile-driven and must remain one atomic commit.

### Plan C — Catalog removal, recovery, and gate state machine

**Ledger IDs:** PR24-003, PR24-004, PR24-005, PR24-006, PR24-017,
PR24-019, PR24-042.

**Decisions:**

- Pending candidate material and expiry are durable and reconstructible.
- Removal-row acceptance and catalog acceptance are one SQLite transaction.
- Candidate revisions come from a durable monotonic counter, not
  `applied_revision + 1`.
- Reload never changes catalog lifecycle status.
- Tombstones are append-only removal-history events; current active membership
  permits deliberate identifier reintroduction.
- Held admission commit is once-only, but scheduling may be retried from
  durable Admitted+Starting state.
- `run_exclusive` does not roll back in-memory mutations; callers must perform
  fallible durable work before gate mutation.

**Single-schema edits:** add `next_candidate_revision_id` to
`provider_catalog_state`, seeded to 1 and monotonically advanced by every
candidate/catalog writer. Make tombstone primary keys `(id,
removed_catalog_revision_id)` so repeated removal history is representable.
No migration is introduced.

**Implementation surfaces and order:**

1. Extend storage contracts with a pending-removal read and candidate-material
   read by revision. In `ProviderCatalogController::startup`, load the pending
   row/material, recompute removed IDs, restore `PreparedCandidate` and the real
   deadline. Missing/inconsistent pieces fail closed as
   `catalog_state_inconsistent`.
2. Add `expire_if_overdue` under the gate and invoke it during startup,
   prepare, accept, and reject. This supplies a production expiry driver
   without introducing an idle timer; if wall-clock expiry while idle is a
   requirement, that needs a separate host decision.
3. Allocate candidate revision from the durable counter. Reject/expire clear
   candidate markers but retain closed rows as history. Retrying the same
   proposal receives a fresh revision/handle.
4. Add a combined storage operation for removal acceptance plus catalog
   acceptance, profiles/kinds, projections, tombstones, state, and audits in
   one immediate transaction. Build private registry material before commit.
   If post-commit in-memory activation fails, transition gate to
   ActivationRecoveryRequired and roll forward at startup. Same-operation
   retries read durable accepted state and return the existing result.
5. Change reload's status update to preserve PendingRemoval and
   ActivationRecoveryRequired; no unrelated writer may exit those states.
6. On acceptance, append tombstone events for removed IDs and remove every ID
   present in the new active material from in-memory tombstone sets. Remove or
   rewrite the unused “never reintroduced” domain validator and associated
   docs.
7. In HeldRunService, validate schedule identity before commit. After commit,
   dispatch failure still returns Accepted; same-operation retry attempts
   dispatch again. The daemon task registry ensures one registered executor.
   Plan D's startup handling closes crash recovery.
8. Document `run_exclusive` mutation-on-error; rename the contradictory test;
   reorder startup/accept closures so gate mutation is last.

**Regression evidence:** pending restart accept/reject/expiry; overdue startup;
fault at every stage of combined acceptance and reopen; same/different
operation retries; reject/expire then retry with a higher revision; reload
during PendingRemoval then restart; remove-reintroduce-remove tombstone cycle;
dispatch failure then same-operation admission retry; activation failure leaves
recoverable gate state; exact mutation-on-error unit semantics.

**Docs/policy:** architecture 22/29 and ADR 0037 must define durable pending
reconstruction, monotonic revisions, atomic acceptance, reload isolation,
tombstone reintroduction, and admitted-run scheduling recovery. Tombstone
reintroduction changes existing accepted wording and requires explicit
adjudication before implementation.

**Atomic order:** durable counter; pending reconstruction/expiry; atomic
acceptance; reload isolation; tombstone semantics; held scheduling recovery;
gate contract documentation.

**Risks/dependencies:** Plan D owns production creation of held rows and startup
ownership. Plan A owns queued selection persistence required by held admission.
Changing permanent-tombstone doctrine must be decided before code/docs diverge.

### Plan D — Runtime, daemon ownership, publication, and inactive surfaces

**Ledger IDs:** PR24-007, PR24-012, PR24-013, PR24-014, PR24-056.

**Recommended decisions:**

- Recovery promotion writes a durable held row in the same transaction and is
  admitted only through AdmitRecoveredRun. Recovery skips held/admitted
  Starting runs on later restarts.
- Remove the unavailable-queue surface rather than inventing an unplanned
  deferred-run runtime. This is a charter change because ADR 0037/0038 retain
  it; do not implement removal without approval.
- Deterministic model/fact failures become durable Failed outcomes in runtime;
  storage/cursor failures transfer ownership to a daemon terminalizer.
- One terminalizer owns every non-terminal executor-error/cancellation state
  until an independent terminal reread.
- Terminal publication uses a bounded, deduplicated catch-up worker.
- Keep AdmitRecoveredRun after wiring its producer. Remove or capability-gate
  ApplyConfigurationEdit and RotateProviderCredentials until a private
  credential source exists; removal is also a charter decision.

**Implementation surfaces and order:**

1. In SQLite promotion, when terminal status is Interrupted, insert the
   successor run and `held_recovered_runs` row atomically. On recovery, skip
   held/admitted runs and clean terminal stale holds. This depends on Plan A's
   promoted selection transfer.
2. Consolidate daemon terminal side effects (publish, queue handling while
   retained, and scheduling a current Starting successor) into one helper used
   by the observer, terminalizer, unadmitted-start failure, and executor-error
   path.
3. Convert deterministic reasoning/fact constructor failures into non-retryable
   durable Failed outcomes. Add a general `fail_active_run` helper for
   Starting/Running/Completing storage failures.
4. Replace the one discarded Cancelling retry with ownership transfer to a
   unified terminalizer. The registry entry is removed only after a fresh
   terminal reread. Cancelling terminalizes Cancelled; other active states
   terminalize Failed.
5. Add a per-run deduplicated terminal-publication retry worker. Retry boundedly
   when durable cursor/status is ahead of `published`; reconnect remains the
   ultimate fallback.
6. If chartered, remove unavailable queue table/traits/services/protocol/client
   methods/tests and all promotion hooks; amend ADR 0037/0038, architecture 29,
   SL2-008/EVD-057 and self-tests in the same change.
7. If chartered, remove always-rejected typed-edit and rotation command/client
   surfaces, retaining raw edit/reference reload. Otherwise gate them behind a
   capability the real daemon does not offer and keep the rejection evidence.

**Regression evidence:** recovery interrupt+promotion+hold in one transaction;
second restart preserves held Starting; real-daemon restart then explicit
admission executes exactly once; oversized reasoning becomes Failed; two
terminalization failures still complete; queued successor after terminalizer
or fail path schedules once; injected terminal publication failure is retried
without duplicate frames; persistent publication failure is bounded.

**Atomic order:** recovery hold; runtime/daemon terminalization; consolidated
terminal side effects; publication retry; then separately approved surface
removals and their docs.

**Risks/dependencies:** unavailable queue and no-success command removal cannot
be silently folded into a fix. Terminalizer and publication workers need
per-key dedup to avoid double ownership/retry storms.

### Plan E — Config, catalog, and provider integration

**Ledger IDs:** PR24-008, PR24-009, PR24-020, PR24-021, PR24-035,
PR24-036, PR24-037, PR24-038, PR24-039, PR24-055, PR24-057.

**Recommended decisions:**

- Treat provider kind, model, and endpoint changes as catalog-affecting and
  reject live reload until catalog replacement can advance both authorities.
  Execution-policy-only changes remain reloadable.
- Generic-chat drains the native stream after finish reason to collect one
  trailing usage chunk, then emits Finished.
- Implement OpenRouter assistant tool-call and Tool-role response mapping using
  the pinned SDK rather than advertising an unusable capability.
- Publish textual reasoning details; suppress recognized opaque
  `reasoning.*` blocks; fail malformed/unknown block families.
- Put credential-shape policy in domain with explicitly different roles for
  identifier, key-name, value, and raw-TOML scanning; config gains the allowed
  domain dependency.
- One config validation core drives startup and candidate validation;
  candidate acceptance requires resolution success.
- Remove dead candidate acceptance/digest DTOs and the private SHA-256 module
  rather than adding a dependency for unused behavior.
- Export one domain canonicalization-version constant with value `"1"`.
- Enforce HTTPS with a deliberate literal-loopback HTTP exception.
- Keep provider option builders but document/default-guard them until catalog
  material can express and composition can apply non-default options.

**Implementation surfaces and order:**

1. Delete `CandidateAcceptanceOutcomeDto`, `redacted_safe_digest`, and config's
   private SHA-256 module/tests. Keep semantic equivalence and field
   classification.
2. Extend `reject_catalog_affecting_edits` to model and endpoint. Update reload
   tests to use same-semantics or execution-policy candidates and add explicit
   model/endpoint rejection.
3. Refactor startup parse and candidate issue collection onto one rule core.
   Missing credential must be an issue; parse failure cannot yield an accepted
   candidate with the old snapshot. Add an acceptance-iff-resolution property
   corpus and align divergent error codes.
4. Define shared credential policy roles in domain; make protocol/config thin
   consumers. Add `intention-config -> intention-domain` to Cargo and
   architecture policy. Use cross-boundary decision-table tests for secret
   words, false-positive identifier substrings, bearer forms, key names,
   controls, and legitimate `provider.credential` placement.
5. Enforce HTTPS or literal loopback HTTP in domain endpoint validation and the
   shared config rule. Keep daemon loopback E2E green; reject non-loopback HTTP.
6. Add and re-export `PROVIDER_SELECTION_CANONICALIZATION_VERSION = "1"` from
   domain; remove both application constants and update all producers/fixtures.
   Build both selection paths and assert equal bytes/digests.
7. Generic-chat: retain terminal reason, keep polling until native end, accept
   one usage-only chunk, reject duplicate finish/usage or post-finish content,
   then emit tool calls and Finished.
8. OpenRouter: translate assistant tool calls with
   `assistant_with_tool_calls`, and Tool messages with `tool_response`. Add exact
   request JSON and full round-two preparation tests.
9. OpenRouter reasoning: use non-empty text/data for Detail; suppress recognized
   opaque blocks; reject missing/unknown block type. Add mixed-stream tests.
10. Add a composition guard test proving currently producible declarations map
    to default provider options. Document the future option-application seam;
    do not silently advertise non-default settings.

**Docs/policy:** architecture 09/14/22/25, ADR 0037, source-of-truth/evidence
rows, Cargo dependency policy. Error-code choices and endpoint exception become
normative. Model/endpoint reload rejection is a behavior decision and must be
reflected in client-facing docs.

**Atomic order:** dead config surfaces; reload classification; shared
validation; credential authority/dependency; endpoint policy; canonicalization
constant; generic usage; OpenRouter tool round; reasoning policy; option seam.

**Risks/dependencies:** changing identifier credential verdicts can reject
legitimate words and needs the decision table. OpenRouter SDK request shape
must be asserted exactly. Model/endpoint live reload is intentionally reduced
until Plan C can atomically replace catalogs.

### Plan F — Tools, platform recovery, reasoning bounds, and quality harness

**Ledger IDs:** PR24-010, PR24-011, PR24-022, PR24-023, PR24-024,
PR24-025, PR24-043, PR24-044.

**Decisions:**

- Unix stale-socket recovery probes before unlinking and retries bind once;
  live sockets and non-socket paths are never removed.
- Execute runs in a Unix process group and all pipe collection is
  deadline/cancellation bounded; Windows guarantees bounded return but needs a
  separate job-object decision for descendant termination.
- Tool processing has per-file, file-count, aggregate-match, edit-target, and
  normalized-result limits.
- `BoundedText` validates on Deserialize; Execute has argument count and total
  byte caps.
- The 4 MiB reasoning aggregate is durable enforcement state in `runs` and is
  checked transactionally at append.
- Quality self-tests write only inside their fixture; daemon coverage is
  semantically deduplicated only if report-artifact expectations permit it.

**Implementation surfaces and order:**

1. Make `reasoning_history::validate_reasoning_output_bound` use checked add.
2. Add `reasoning_aggregate_bytes` to the single live runs DDL. In SQLite
   append, pre-scan every reasoning delta/summary in the batch with the domain
   helper, reject the complete batch before writes on overflow/cap breach, and
   update the counter in the same transaction. Add exact-bound, crossing,
   reopen, per-run, mixed-summary and atomic-batch tests.
3. Implement validating Deserialize for `BoundedText`; add Execute argument
   count/aggregate validation and daemon parse-boundary tests.
4. Replace directory grep's unbounded reads with bounded reads; cap scanned
   files and retained aggregate bytes; reject oversized edit targets; bound
   write expected-content reads; clamp normalized durable/model content with a
   char-safe truncation contract and durable-fit property.
5. Execute: on Unix create a process group using safe APIs. Terminate the group
   on cancel/timeout and when descendants retain pipes. Replace unconditional
   reader joins with deadline-aware joins and a short grace. A direct
   dependency on the already locked `rustix` may be used only after confirming
   API/features and updating dependency policy/notices.
6. Unix listener bind: on first failure, require an existing socket and a
   ConnectionRefused probe before unlink; retry once. Add stale/live/non-socket
   sync/async tests and remove daemon E2E's manual stale-socket cleanup.
7. Redirect coverage metadata self-test output to a temporary reports root and
   restore the live file defensively in `finally`. Validate in-place mode.
8. Normalize semantic feature sets before daemon coverage dedup. Preserve the
   canonical report or explicitly copy it for consumers; do not remove profile
   artifacts without confirming CI expectations.

**Proposed limits requiring implementation confirmation:** 10,000 scanned
files, 128 KiB retained grep fragments, 1 MiB edit target, 64 KiB normalized
tool content, 128 execute arguments and 256 KiB aggregate arguments. The
durable-fit property, not these exact numbers, is the controlling invariant.

**Regression evidence:** stale Unix socket after abrupt listener death; live
endpoint remains owned; background descendant retaining pipes cannot exceed
timeout and is killed on Unix; huge grep/edit/write inputs stay bounded and
unchanged on rejection; serde rejects oversized/NUL text; reasoning cap
survives restart and isolates runs; self-test leaves report byte-identical;
coverage runner invokes only the approved semantic profile set.

**Atomic order:** overflow helper; durable aggregate; serde bounds; aggregate
tool bounds; process group/joins; socket reclaim; coverage dedup; self-test
isolation. Each behavior update carries architecture 03/05/22 or quality docs
where applicable.

**Risks/dependencies:** process-tree APIs must remain safe under `unsafe_code`
deny. JSON truncation must choose valid structured output versus existing text
marker semantics. Coverage artifact changes need CI consumer confirmation.

### Plan G — Architecture, ADR, and reconciliation repair

**Ledger IDs:** PR24-045, PR24-046, PR24-047, PR24-048, PR24-049,
PR24-050, PR24-051, PR24-052, PR24-053, PR24-054.

**Authority rule:** ADR 0036/0037 and active reconciliation rows are living
normative documents and must be amended. Historical closeout evidence,
`m4.md`, provenance, and ADR 0038's own inventory remain untouched.

**Exact repairs:**

1. ADR 0036: protocol 1.1 exact equality; public DTO 1.1 required current
   fields; config schema 1 single shape; SQLite logical version 1, no migration;
   V4-only execution meaning; delete 0x020C registry/appendix; replace
   compatible-minor evidence with current-version evidence; replace
   ReservedForSlice2 with current Slice3/Slice4/Wired statuses; remove V3 rows
   and stale preserved-fixture claims.
2. Reconciliation README: replace additive 3-to-4 migration with single live
   logical schema; add ADR 0038 to scope and owner map.
3. Decisions index: add ADR 0038. Extend `quality/self_test.py` so ADR 0038 file,
   decisions index, and reconciliation owner-map presence are all enforced.
4. Source-of-truth matrix: SL1-003/004 become V4-only; SL1-008 cites current
   schema evidence; RUN-003 owns current DDL/projections, not migrations;
   CFG-002 through CFG-006 point to EVD-049 through EVD-053 as Verified.
5. ADR 0037 ownership: current-schema DDL/projections/rows, not schema-4
   migrations.
6. ADR 0022: programmatic caller policy exists in V4 only with an ADR 0038
   supersession note. Also inspect architecture 27's matching v3/v4 wording and
   amend it in the same logical commit if active.
7. Architecture 25: reload reparses and validates; it does not migrate.
8. Storage comments: use current-schema/control-plane terminology in both
   storage crates.

**Atomic commits:** ADR 0036 reconciliation; ADR 0038 indexing plus self-test;
matrix repairs; ADR 0037/0022 ownership/version repairs; architecture 25; code
comment terminology. The index test must land with both index rows.

**Validation:** `make docs-check`, architecture checker, quality self-test in
place, formatting for comment changes, then `make verify` after all plans.

**Cross-plan coordination:** Plan B changes capability error lists and ADR 0037
gate semantics; Plan C changes removal/tombstone doctrine; Plan D may request
charter removal of queue/edit/rotation surfaces; Plan E changes reload and
endpoint policy. Apply Plan G's baseline repairs first or last, but rerun a
final documentation consistency sweep after every behavioral decision so the
ledger does not immediately become stale.

## Proposed overall implementation order

1. Land Plan G's baseline documentation/index corrections that describe the
   current head and do not depend on adjudication.
2. Fix PR24-001 first. It blocks ordinary repeated conversation runs and is a
   prerequisite for recovered-run selection verification.
3. Land Plan A's remaining storage fixes.
4. Implement Plan C's durable catalog state machine.
5. Implement Plan B's real capability gate after the command inventory is
   final.
6. Implement Plans D and E, resolving charter decisions before deleting any
   activated surface.
7. Implement Plan F's boundedness/platform/quality items in independent commits.
8. Run a final Plan G consistency pass, update this ledger statuses, and run the
   full verification matrix.

## Validation matrix for the complete repair series

- targeted tests for every changed crate and every named regression;
- `cargo test --workspace`;
- `cargo nextest run --workspace --all-targets --locked --no-fail-fast`;
- `cargo clippy --workspace --all-targets --locked -- -D warnings`;
- `cargo fmt --all -- --check`;
- `make quick` during each atomic commit;
- `make verify` after each cross-cutting plan and at final head;
- `make docs-check`;
- `python3 quality/check_architecture.py`;
- `python3 quality/self_test.py` and the in-place self-test mode;
- Linux and Windows CI, including Unix-only stale socket/process-group tests
  and Windows bounded-return behavior;
- repository-wide searches proving removed symbols, error codes, migration
  wording, V3 claims, 0x020C, and obsolete test fixtures have no unintended
  live references.

## Repair-run status

Status recorded by the PR24 repair run on branch
`impl/m5plus-slice2-control-plane` (working tree at `ff0bdfb` + uncommitted
repairs; no commits were created by the repair run).

### Fixed

- **PR24-001** (storage/SQLite): `selection_digest` is no longer globally
  UNIQUE; `insert_selection` is keyed by `run_id` with idempotent identical
  rebind and typed `provider_selection_conflict` for different bytes. Queued
  turns now persist `resolved_selection_json` on `queued_turns`, and promotion
  transfers the selection into `resolved_run_provider_selections` in the same
  transaction as run creation, with a `ProviderSelection` fault point proving
  atomic rollback. Regression tests: same-profile two-run persistence,
  queued-selection transfer + reopen, and promotion fault rollback.
- **PR24-015**: idempotent turn replies recompute from current `runs` status
  and current `queued_turns` membership; retry after queued-turn removal
  returns the typed `accepted_turn_removed` conflict instead of a ghost queue
  position. Regression tests added.
- **PR24-016**: catalog material descriptor resolution is (kind,
  descriptor-revision)-aware: a revision shared by several kinds resolves to
  the profile's own kind; a revision with rows of other kinds but none of the
  profile's kind fails typed decode; single-row revisions stay unambiguous.
- **PR24-018**: same-run identical rebind is idempotent; a different selection
  for the same run returns `provider_selection_conflict` (run-scoped precheck;
  the digest-global conflict is removed).
- **PR24-025**: `validate_reasoning_output_bound` uses checked addition and
  rejects overflow with `reasoning_output_limit_exceeded`.
- **PR24-026**: `ProgrammaticCallerPolicySelectionV1::decode` requires record
  version 1; wrong-version negative fixture added.
- **PR24-027**: envelope metadata fields and reasoning-history `max_entries`
  decode via `u32::try_from` with `InvalidField`; boundary fixtures reject
  `u32::MAX + 1` and accept `u32::MAX`.
- **PR24-028**: envelope digest now authenticates fields 1-4 (execution kind,
  meaning tag, meaning record version, canonicalization version) plus field 5;
  metadata tampering produces `DigestMismatch`; the three V1 envelope goldens
  were regenerated to the new digest shape (no old digest shape retained).
- **PR24-029**: current-shape minimal ErrorDto JSON test asserts absent
  `correlation_id`/`detail` decode as None (no legacy fixture).
- **PR24-030**: `require_capability` maps every capability to an explicit
  family code; the `execution_meaning_capability_required` wildcard fallback
  is removed.
- **PR24-032**: protocol module documentation rewritten: exact 1.1 equality,
  required current fields, unknown-additive tolerance; the identical-decoding
  claim for old payloads is removed.
- **PR24-033**: synchronous `IntentionClient::request_on` now applies the same
  DTO schema-version equality as the stream clients.
- **PR24-039**: `intention-domain::provider_selection` exports
  `PROVIDER_SELECTION_CANONICALIZATION_VERSION = "1"`; both application
  producers (catalog and session-selection) consume it; the private
  `"provider-selection-v1"` producer constant is removed.
- **PR24-040**: `ReloadTransactionDto.migration_result` field, validator
  entries, and all producers are removed; ADR 0038 documents the executed
  removal.
- **PR24-041**: removal-candidate conflicts are detected explicitly in the
  transaction in deterministic order (handle, then pending existence, then
  revision) and mapped to `provider_catalog_removal_candidate_conflict` /
  `provider_catalog_removal_pending_exists`; the repository test now asserts
  the exact typed codes.
- **PR24-042**: `provider_gate` test renamed to
  `run_exclusive_propagates_typed_errors_after_in_memory_mutation` and
  documents mutation-on-error semantics.
- **PR24-043**: the coverage self-test saves and restores the live
  `quality/reports/coverage-metadata.json` (or removes its synthetic
  snapshot) in `finally`, and asserts byte-identical restoration.
- **PR24-044**: `run_coverage.py` normalizes the daemon's semantic feature set
  (`--all-features` subsumes default/no-default selection) before
  deduplication; equivalent profiles no longer re-execute the identical
  instrumented daemon set under different flag tuples.
- **PR24-045**: ADR 0036 amended: version ledger, numeric registry (0x020C
  removed), evidence wording, runtime-version resolution, registry activation
  statuses (Slice 2 Wired, ReservedForSlice3/4), V3 appendix rows and the
  0x020C appendix removed, envelope digest note updated.
- **PR24-046**: reconciliation README describes the single live logical
  schema instead of the additive 3-to-4 migration and adds ADR 0038 to scope.
- **PR24-047**: ADR 0038 added to the decisions index and the reconciliation
  owner map; `quality/self_test.py` enforces ADR 0038 file/index/owner-map
  presence.
- **PR24-048**: SL1-003/004 are V4-only with ADR 0038 constraints; SL1-008
  retires preservation/future-schema evidence wording and updates the test
  count.
- **PR24-049**: ADR 0037 ownership assigns current-schema DDL, projections,
  and durable control-plane rows (no schema-4 migrations).
- **PR24-050**: ADR 0022 references `run-execution-meaning-v4` only, with the
  ADR 0038 supersession note; architecture 27 v3 wording and fixtures rows
  updated in the same pass.
- **PR24-051**: RUN-003 assigns the single current-schema DDL and projections;
  matching wording in ADR 0003 updated.
- **PR24-052**: CFG-002 through CFG-006 point to the verified EVD-049..053
  anchors.
- **PR24-053**: architecture 25 states reload re-parses and validates; no
  migration wording remains.
- **PR24-054**: current-schema/control-plane terminology replaces schema-4
  wording in both storage crates' comments.

### Partially fixed

- **PR24-005**: the SQLite reload commit now preserves `pending_removal` and
  `activation_recovery_required` statuses (no unrelated writer exits those
  states); daemon-side gating of provider-affecting reload/edit/rotation
  commands and pending-removal + reload + restart coverage remain open.
- **PR24-033**: client code fixed; a dedicated wrong-schema response fixture
  test was not added in this run.

### Not implemented in this run (blocked or deferred with the safe recommendation)

- **PR24-002** — real daemon `provider_profiles_v1` advertisement, per-peer
  remote-capability retention, and the exhaustive capability gate: deferred;
  requires daemon connection-state design plus E2E negotiation fixtures
  (Plan B items 7 and its evidence). Recommendation: implement Plan B gate
  inventory after the command surface is final.
- **PR24-003** — pending-removal restart reconstruction and production expiry
  driver: deferred; requires new storage read surfaces and controller startup
  rebuild of `PreparedCandidate` plus a real deadline (Plan C steps 1-2).
- **PR24-004** — atomic combined removal+catalog acceptance: deferred; needs a
  combined storage operation and operation-idempotent acceptance roll-forward
  (Plan C step 4).
- **PR24-006** — durable monotonic candidate revision counter: deferred; needs
  `provider_catalog_state.next_candidate_revision_id` plus controller
  allocation; reject/expire-then-retry currently stays revision-bound.
- **PR24-007** — recovery-promotion held-row production and startup admission
  sweep (held part), and the unavailable-queue removal decision: deferred;
  queue-surface removal is a charter change (ADR 0037/0038 retain it).
- **PR24-008** — reload classification of model/endpoint as catalog-affecting:
  deferred; behavior decision with client-facing documentation impact
  (Plan E step 2).
- **PR24-009** — generic-chat trailing-usage drain: deferred.
- **PR24-010**, **PR24-011** — stale Unix socket reclaim and process-group /
  deadline-aware pipe joins: deferred; pre-existing platform fixes with
  Unix-specific test requirements (Plan F steps 5-6).
- **PR24-012**, **PR24-013**, **PR24-014** — runtime durable Failed conversion,
  terminalizer ownership transfer, and bounded terminal-publication retry:
  deferred; daemon ownership redesign (Plan D steps 2-5).
- **PR24-017** — tombstone reintroduction: BLOCKED pending adjudication;
  ADR 0037 records tombstones as permanent and Plan C requires explicit
  charter decision before code/docs diverge. Recommendation: on catalog
  acceptance, clear in-memory tombstones for IDs present in the new active
  material and treat durable tombstones as append-only removal history; amend
  ADR 0037 Appendix B wording in the same change.
- **PR24-019** — held-admission dispatch-after-commit recovery: deferred;
  needs startup scheduling recovery from durable Admitted+Starting state
  (Plan D coordination).
- **PR24-020**, **PR24-021** — OpenRouter tool-round mapping and encrypted
  reasoning-detail policy: deferred; SDK request-shape work with exact JSON
  tests (Plan E steps 8-9). Recommended policy for PR24-021: suppress
  recognized opaque blocks, fail malformed/unknown shapes.
- **PR24-022**, **PR24-023** — aggregate tool/file bounds and validating
  `BoundedText` Deserialize: deferred; serde boundary work plus limit
  decisions (Plan F steps 3-4).
- **PR24-024** — durable per-run reasoning aggregate enforcement: deferred;
  requires `reasoning_aggregate_bytes` on `runs` and transactional append
  pre-scan (Plan F step 2).
- **PR24-031** — required-field accessor cleanup (`result`/`projection`):
  deferred; compile-driven wide accessor change (Plan B step 2).
- **PR24-034** — minor-mismatch sync/async negotiation tests: deferred.
- **PR24-035** — shared credential-shape policy roles in domain: deferred;
  needs cross-boundary decision table and config->domain dependency policy
  update (Plan E step 4).
- **PR24-036** — one config validation core for startup and candidates:
  deferred (Plan E step 3).
- **PR24-037**, **PR24-038** — config private SHA-256 and dead candidate
  acceptance/digest surfaces removal: deferred; recommended: delete the dead
  DTOs and digest module rather than add a dependency (Plan E step 1).
- **PR24-055** — `https-only` enforcement with literal-loopback exception:
  deferred (adjudicated recommendation recorded in Plan E step 5).
- **PR24-056** — typed-edit credential retention / rotation source wiring /
  held-admission producer: deferred; removing surfaces is a charter change;
  gating behind a non-offered capability would contradict ADR 0037's
  activation of the control-plane surface. Recommendation: implement the
  private credential source in a later slice; keep fail-closed evidence.
- **PR24-057** — catalog provider options applied at the composition
  boundary: deferred; recommendation: keep option builders but document and
  default-guard them until catalog material can express and composition can
  apply non-default options (Plan E step 10).

Charter/authority notes: no ADR 0037/0038 scope change was made without
authority; items requiring adjudication (PR24-017, parts of PR24-007/056,
and the tombstone doctrine) are recorded as blocked rather than guessed.

## Repair-run status: second pass (working tree after the first repair run)

Recorded by the follow-up repair session on the same branch (working tree at
`ff0bdfb` + the first-pass uncommitted repairs + this pass; still no commits
created by the repair runs). This pass supersedes the per-item statuses above
for the items it touched.

### Fixed additionally in this pass

- **PR24-008**: `reject_catalog_affecting_edits` now classifies provider
  model and endpoint changes as catalog-affecting together with provider
  kind, so live reload rejects them with `catalog_change_requires_restart`;
  execution-policy-only candidates remain reloadable. The catalog runtime
  controller keeps its own kind-only classification (model/endpoint changes
  remain catalog-replacement material for that path). Config tests,
  `ConfigurationReloadService` unit tests, the M5 control-plane runtime
  tests, and the composition reload tests were updated to policy-only
  candidates with explicit model/endpoint rejection assertions.
- **PR24-017**: on every acceptance the controller clears in-memory tombstones
  for identifiers present in the accepted material (reintroduction admits
  again) while durable tombstone tables are append-only removal history keyed
  by `(id, removed_catalog_revision_id)`. The unused never-reintroduced
  domain validator `validate_profile_id_not_tombstoned` and its tests were
  removed; ADR 0037 table rows, architecture 22 lifecycle wording, domain
  tombstone docs, and the PRV-008 source-of-truth row were amended in the
  same change. Regression: remove/reintroduce/remove cycle at the controller
  level and removal-history identity evidence at the domain level.
- **PR24-031**: `ProtocolAcceptedDto::result()` and
  `SessionSnapshotDto::projection()` return direct references (still
  `const fn`); the impossible `Some` matching and the client
  `ok_or_else(invalid_response)` branch were removed across production and
  tests.
- **PR24-033**: the wrong-schema response fixture was added: the sync client
  fixture daemon gains a `SchemaMismatch` response carrying DTO schema 1.2,
  and the scenario matrix asserts `invalid_local_protocol_response`.
- **PR24-034**: sync and async 1.2-vs-1.1 rejection coverage added on both
  sides: daemon-side and client-side sync minor mismatch tests, an async
  daemon-side typed rejection test, and an async fail-closed client test, all
  in `intention-transport/tests/transport_integration.rs`; the sync client
  fixture also covers the client side via `MinorProtocolMismatch`.
- **PR24-037/038**: config's private SHA-256 module,
  `redacted_safe_digest`, and the dead `CandidateAcceptanceOutcomeDto`
  projection were deleted together with their fixture tests; ADR 0038's
  executed-removal inventory was amended.

### Still not implemented after this pass (unchanged from the first pass)

The following findings remain open with the same precise statuses recorded in
the first-pass sections above: PR24-002 (real daemon capability advertisement
and exhaustive gate), PR24-003 (durable pending-removal reconstruction and
production expiry driver), PR24-004 (single-transaction combined removal plus
catalog acceptance), PR24-005 remainder (daemon-side gating of
reload/edit/rotation during pending removal plus the pending-removal +
reload + restart coverage), PR24-006 (durable monotonic candidate revision
counter), PR24-007 (recovery-promotion held-row production and unavailable
queue producers), PR24-009 (generic-chat trailing-usage drain), PR24-010 and
PR24-011 (stale Unix socket reclaim and process-group/deadline-aware pipe
joins), PR24-012, PR24-013, PR24-014 (durable Failed conversion,
terminalizer ownership transfer, terminal-publication retry), PR24-019
(held-admission dispatch recovery), PR24-020 and PR24-021 (OpenRouter tool
round and opaque reasoning-detail policy), PR24-022 and PR24-023 (aggregate
tool/file bounds and validating `BoundedText` deserialize), PR24-024 (durable
per-run reasoning aggregate enforcement), PR24-035 (role-aware shared
credential-shape policy), PR24-036 (single config validation core),
PR24-055 (`https-only` enforcement with a literal-loopback HTTP exception),
PR24-056 (typed-edit/rotation/hold producers in real composition), and
PR24-057 (catalog option application at the composition boundary).

Precise blockers for the still-open items are unchanged from the first-pass
notes: PR24-002/005/007/056 need daemon connection-state and composition
producer work whose fixtures and facade surfaces were not completed in this
session; PR24-003/004/006/019 need the durable storage read/transaction
surfaces plus controller reconstruction and their SQLite and fake
implementations; PR24-035/055 need the domain credential-role and endpoint
policy decisions applied across boundaries; PR24-036 needs one validation
core refactor; the remaining provider/tool/platform items are bounded by the
same fixture requirements recorded above. No ADR 0037/0038 scope change was
made without authority.

## Repair-run status: fourth pass (final; working tree after this pass)

This pass continued from the same branch with the third-pass tree and no
commits. It implemented the remaining catalog-lifecycle durability, the
recovery-hold producer, and fixed every compile/test/lint/architecture
failure the uncommitted tree still had. Final per-finding status:

- **PR24-001, 008, 009, 010, 011, 020-034, 035-054, 055**: Fixed (same
  evidence as the second-pass entries plus the third-pass implementations now
  verified green).
- **PR24-002**: Fixed. Real daemon advertises `provider_profiles_v1`, retains
  the remote capability set per connection, and gates every Slice 2
  command/query before effect; a daemon-level table test covers the exhaustive
  classifier (both rejection branches and the negotiated dispatch), and the
  client/protocol negotiation fixtures cover the client side.
- **PR24-003**: Fixed. The prepared removal candidate's material is durable
  (candidate projection rows), startup rebuilds `PreparedCandidate` from
  durable rows with the real deadline, and the production expiry driver runs
  at startup and on prepare/accept/reject (gate plus durable-state authority).
- **PR24-004**: Fixed. A crash between the removal-acceptance commit and the
  catalog acceptance is rolled forward at startup from the durable prepared
  material; regression coverage exists at controller (fake), facade, and
  repository levels.
- **PR24-005**: Fixed. Reload commits preserve `pending_removal` /
  activation-recovery states durably; pending-removal + execution-policy
  reload + restart coverage added at the facade level. The daemon-side
  blocking of reload/edit/rotate while degraded was deliberately not added:
  the durable fix makes reload safe and blocking it would remove the repair
  path; the OR-clause of the original fix direction is fulfilled by the
  durable preservation arm plus coverage.
- **PR24-006**: Fixed. Candidate revisions are allocated from the durable
  monotonic removal maximum (SQLite override; safe default for fakes);
  reject/expire-then-retry gets a fresh revision/handle (facade + application
  tests).
- **PR24-007**: Fixed (held part). Recovery-interrupted promotion writes the
  durable held row in the same transaction; later restarts skip held/admitted
  runs; stale holds are cleaned once their run is terminal; the daemon skips
  held runs and schedules on explicit admission. The unavailable-queue
  enqueue producer remains dormant: no production decision in this slice
  produces a provider-unavailable run to enqueue, and the ledger's charter
  note on queue-surface removal does not apply (the surface is retained and
  its promote/reconcile entry points are wired).
- **PR24-012, 013, 014, 019**: Fixed. Unified terminalization owns every
  non-terminal executor-error state until a fresh terminal reread; the
  terminalizer also reports execution completion for Cancelling runs; the
  host reports completion through a registry-stored watch sender; bounded
  terminal publication retry is in place; held/admitted Starting successors
  survive restarts and are re-driven by the idempotent admission retry plus
  the daemon scheduling-on-acceptance path.
- **PR24-021**: Fixed with the adjudicated policy (suppress recognized
  opaque/encrypted and server-tool blocks, fail malformed/unknown shapes) and
  mixed-stream tests.
- **PR24-022, 023**: Fixed with bounded file/grep/edit/write/execute limits
  and validating `BoundedText`/execute-argument Deserialize; regression tests
  landed (bounded_contracts, tool_contracts, daemon parse boundary).
- **PR24-024**: Fixed with a durable per-run reasoning aggregate enforced
  transactionally at append.
- **PR24-035**: Fixed with the single role-aware credential-shape policy in
  `intention-domain` consumed by config and protocol, with cross-boundary
  decision-table tests.
- **PR24-036**: Fixed with one rule core: candidate acceptance is exactly
  startup resolution success; candidate/startup error codes cannot diverge.
- **PR24-055**: Fixed with HTTPS-or-literal-loopback enforcement (including
  bracketed `[::1]` handling) in domain endpoint validation and the config
  rule, with tests.
- **PR24-056**: Partially fixed. The held-admission producer is wired
  (PR24-007; `AdmitRecoveredRun` has a real success path through recovery
  holds). Typed-edit and rotation producers remain fail-closed by design: the
  daemon is credential-free and no private credential source exists in this
  slice (architecture 25 assigns one to a later slice); the approved
  "keep and fully wire" decision is recorded here as wiring the producers
  that exist (`AdmitRecoveredRun` and the unavailable-queue promote/reconcile
  entry points) while typed-edit/rotation stay fail-closed with durable
  evidence. Any change to that posture requires a charter decision.
- **PR24-057**: Not implemented; the composition still builds default
  provider drivers and the catalog option-application seam is documented as a
  later-slice item (Plan E step 10 guard remains a follow-up).

### Validation results

- `cargo fmt --all -- --check`: clean.
- `cargo clippy --workspace --all-targets --locked -- -Dwarnings`: clean.
- `cargo test --workspace`: green (all suites).
- `make quick`: green.
- `make verify`: green. The final gate run (`make verify` with `pipefail`,
  log `target/pr24-make-verify-19.log`) exits 0 across fmt, features, check,
  lint, test, docs, architecture, coverage, deps, and quality self-test.
  The last coverage deficit was closed by extending the `bounded_contracts`
  regression suite with focused tests for the reachable uncovered branches:
  write expected-content preflight for missing and non-UTF-8 targets, edit
  fail-closed on non-UTF-8 targets, grep match-cap truncation in both the
  pattern-only file path and the scoped directory path, scoped grep
  rejection of a special-file (Unix socket) target, and the
  spawn-observation wait overflow guard. `intention-tools` line coverage
  measured through the same `llvm-cov nextest --package intention-tools`
  command the pipeline runs is now 93.00-93.13% across profiles, above the
  90.00% tier B requirement. Two intermediate reruns were recorded before
  green: `target/pr24-make-verify-17.log` failed a timing-sensitive,
  unmodified `intention-storage-sqlite` concurrency test once under full
  parallel load (passes in isolation and in every later run), and
  `target/pr24-make-verify-18.log` exposed a stale quality-self-test
  fixture: `quality/self_test.py` still mutated the pre-`bounded_contracts`
  `intention-tools` test-target list, so the fixture string was synced to the
  machine-readable policy in the same change. Earlier full logs:
  `target/pr24-make-verify-15.log`, `target/pr24-make-verify-16.log`.
- Architecture and docs gates: clean after the machine-readable policy
  updates (`intention-config -> intention-domain`, `intention-tools`
  external dependency `rustix`, `bounded_contracts` test target, and the
  `serde_json::Value` avoidance in `intention-storage-sqlite`).

### Known remaining items (honest, not deferred silently)

- **PR24-056 typed-edit/rotation producers** and **PR24-057 option
  application**: not implemented; both require a private credential source /
  provider-option seam that architecture 25 assigns to a later slice (charter
  decision required before wiring).
- **Unavailable-queue enqueue producer**: dormant by design, no production
  produce decision exists in this slice; recorded in PR24-007.
- No commits were created; all work remains in the working tree on
  `impl/m5plus-slice2-control-plane`.
