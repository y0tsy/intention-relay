# 0022: Post-M5 Programmatic-Caller Policy Directions

## Status

Accepted 2026-08-30; amended 2026-09-30 by [ADR 0048](0048-limits-by-precedent-and-no-content-scanning.md): the
corridor, reservation, and fixed run/calendar-limit clauses are removed. The remaining programmatic-caller policy and
admission direction stands, bound to Milestone 5+ as before.

## Decision

The programmatic-caller policy and admission model from `m4plus_concept.md` is adopted as an accepted future direction
owned by architecture 27 (programmatic-caller policy and admission):

- one closed root origin (`InteractiveUser`) with no
second root, and immutable `ProgrammaticCallerProvenanceDto` audit records created before `ToolCallStarted`;
- durable policy identity/scope/narrowing (`Project`/`Goal`/`Session`) with
intersection of effect selectors and exact tool/MCP selectors, most-restrictive-wins, and child-narrowing-only;
- closed admission decisions (`Prohibited`, `DirectLocalRead`,
`ExactConfirmationRequired`, `BoundedConfirmationRequired`) with exact confirmation bound to one `ToolCallId`; the
numeric baseline and the bounded corridor are removed by [ADR
0048](0048-limits-by-precedent-and-no-content-scanning.md);
- policy lifecycle (`Active`/`Suspended`/`Revoked`/`Archived`) with live
tightening, drafts, and no reactivation after revoke;
- `InterruptedBeforeStart`/`Partial` recovery for admitted effects: an admitted
effect interrupted or lost before a final result commits a bounded partial result and permits the next model step; the
run, calendar, and reservation limit machinery is removed by ADR 0048;
- `ProgrammaticCallerPolicySelectionV1` with `Disabled` only for historical
M4, and closed `ErrorDto` safe failures for the remaining directions.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only after its own activating specification, and
remains bound to Milestone 5+.

## Normative invariants

1. Every programmatic action has one daemon-assigned root origin; no second
root exists in this first scope.
2. The policy is logical product control and audit evidence, not an OS
security boundary; it creates no durable autonomous actor, second daemon, second registry, remote identity, or authority
surviving an active run.
3. Effective policy is the most-restrictive intersection of effect selectors
and exact tool/MCP selectors; children may only narrow.
4. `execute` never receives `DirectLocalRead` admission in this first scope;
`fetch_url` and `mcp` never receive direct admission.
5. Historical M4 and earlier selections retain `Disabled` policy selection
and are never rewritten or given synthetic policy state.

## Failure semantics

- Snapshot, revision, origin, and draft failures are known typed pre-effect
rejections.
- A started effect interrupted or lost before a final result commits a bounded
`Partial` result with its notice and is never retried; nothing pauses.
- Live suspension or revocation imposes stricter present-time denial but never
rewrites historical semantics or resumes external work.

## Rationale

The programmatic-caller policy was present in `m4plus_concept.md` but absent from the authoritative documentation,
including the retired deferred/excluded, supersession, and contradiction registers. Adopting the model records it as an
accepted future direction for ordinary future work without documenting any feature as implemented.

## Compatibility and non-goals

This decision supersedes the absence of a disposition for the programmatic-caller policy in the retired reconciliation
registers. For new Mandate work, retained RLM run-rooted activity identity, root-origin, direct-pair queue, and fixed
observation limits remain historical-only where they conflict, consistent with architectures 17 and 24. The closed M4
baseline, M3/M4 bytes, and existing behavior remain unchanged, and no code changes are authorized by this decision.

No typed command-template direction, new policy decoder, or OS security boundary is added. M5-M9 are not renumbered.

Owner: architecture 27. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).
