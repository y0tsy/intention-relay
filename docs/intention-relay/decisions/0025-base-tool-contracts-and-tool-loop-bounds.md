# 0025: Post-M5 Base-Tool Contracts and Tool-Loop Bounds

## Status

Accepted 2026-08-30; amended 2026-09-30 by [ADR 0048](0048-limits-by-precedent-and-no-content-scanning.md) and [ADR
0045](0045-local-json-rpc-2-0-transport.md): the numeric tool-group, output, and replay-page bounds and the capability
gate are removed. The qualitative contracts and the terminal taxonomy stand.

## Decision

The following detail from `m4plus_concept.md` is adopted and owned by [architecture
15](../architecture/15-tool-registry-and-mandate-tool-loop.md):

- the initial effect-profile flag mapping for the base tools;
- the base-tool initial contracts: `execute` `ShellCommandTextDto`,
`fetch_url` GET/HEAD-only, `ask_user` as a normal `user_interaction` tool, and the trusted-local, no-OS-sandbox model;
- the fragment stream contract (`ToolOutputDeltaRecorded` +
`ToolCallResultRecorded`, per-call positive positions, `tool_result_stream_invalid`);
- the descriptor `model_schema_availability` field (whether an active
descriptor can supply a code-owned function schema to a compatible model subset) and the typed-reference alternatives
for non-path tools (typed URL, question, todo, retained-content, plan, child-agent, or MCP-method reference);
- the explicit statement that the first scope adds no numeric model-step
limit;
- the removal of the first-scope numeric bounds (previously 512 KiB per
canonical fact, 4 MiB combined per group, and `tool_output_limit_exceeded`): no per-fact, per-group, or per-page cap is
asserted, and only the transport and IO liveness safeguards remain (ADR 0048);
- the closed terminal outcome taxonomy (`Succeeded`,
`DeniedBeforeExecution`, `FailedBeforeExternalEffect`, `CancelledBeforeStart`, `InterruptedBeforeStart`,
`ExecutionUnavailable`, `ExternalEffectUnknown`), with the former `OutputLimitExceeded` member removed together with the
numeric output budget (ADR 0048); and
- tool-history replay negotiation (`RunToolHistoryPageDto` /
`RunToolHistoryCompletedDto`), including the combined publication-gate order when the same subscription also delivers
the normalized reasoning stream (`RunSnapshotDto` → reasoning pages/completion → tool pages/completion → live frames, with
omit-if-absent for either history class).

Each direction keeps M3/M4 behavior authoritative (including the byte-identical `tool_execution_unavailable` denial),
affects fresh runs only after its own activating specification, and remains bound to Milestone 5+.

## Normative invariants

1. The effect profile states only direct declared capability and never itself
requires confirmation; `process_start` does not claim a shell program cannot read/write/start descendants or access
network.
2. `execute` runs with the user's ordinary OS authority and `WorkspaceRoot`
CWD and is not a sandbox; `fetch_url` permits only GET/HEAD over HTTP(S); `ask_user` keeps the post-M4 run `Running` and
never rewrites M3/M4 `WaitingInput` semantics.
3. Every accepted fragment commits as its own durable fact before publication;
a fragment is never model context by itself.
4. Output is never truncated, sampled, or partly committed; a fragment is
either accepted whole as its own durable fact or not published.
5. An `ExternalEffectUnknown` result never permits another model step.
6. Capability negotiation no longer exists (ADR 0045); historical M4 runs
retain byte-identical replay and denial.

## Failure semantics

- Malformed fragment streams fail closed as `tool_result_stream_invalid`.
- Missing or incomplete tool history requires typed resynchronization and
never causes a live-tool retry.
- The removed numeric budgets and the `provider_tool_group_invalid` outcome
are superseded by ADR 0048; an invalid fragment stream still fails before effect through `tool_result_stream_invalid`.

## Rationale

The base-tool initial contracts and the loop bounds and terminal taxonomy were present in `m4plus_concept.md` but only
at principle level in architecture 15. Adopting the detail makes the authoritative documentation cover the feature
without documenting any part of it as implemented.

## Compatibility and non-goals

This decision supersedes the absence of the detail in architecture 15's principle-level text. The closed M4 baseline,
M3/M4 bytes, and existing behavior remain unchanged, and no code changes are authorized by this decision.

Parallel tool execution, tool-failure-to-model continuation, and Mandate/architecture-15 loop activation remain outside
this decision, as do MCP, kernel, VFR, Headroom, and Plan features. M5-M9 are not renumbered.

Owner: architecture 15. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).
