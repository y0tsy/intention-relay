# ADR 0025: Post-M5 Base-Tool Contracts and Tool-Loop Bounds

## Status

Accepted 2026-08-30. This decision records the base-tool initial contracts and
the model-tool-loop fragment/terminal/bounds detail as accepted future
directions. It does not activate implementation: the contracts are bound to
[Milestone 5+](../architecture/11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment)
and are implemented only through a separately accepted activating
specification at the start of that milestone.

## Decision

The following detail from
[`m4plus_concept2.md`](../m4plus_concept2.md) is adopted and owned by
[architecture 15](../architecture/15-tool-registry-and-mandate-tool-loop.md):

- the initial effect-profile flag mapping table (read/glob/grep/expand =
  `workspace_read`; write = `workspace_write`; edit = read+write; execute =
  `process_start`; fetch_url = `network_retrieval`; ask_user =
  `user_interaction`; todo/plan_submit = `session_state_mutation`; retrieve =
  `retained_content_read`; sub_agent = `child_agent_start` +
  `child_agent_control`; mcp = `process_start` + `network_retrieval`);
- the base-tool initial contracts: `execute` `ShellCommandTextDto`, `fetch_url`
  GET/HEAD-only, `ask_user` as a normal `user_interaction` tool, and the
  trusted-local (no-OS-sandbox) model;
- the fragment stream contract (`ToolOutputDeltaRecorded` +
  `ToolCallResultRecorded`, per-call positive positions, `tool_result_stream_invalid`);
- the first-scope bounds (512 KiB per canonical fact, 4 MiB combined per group,
  `tool_output_limit_exceeded`);
- the closed terminal outcome taxonomy (`Succeeded`,
  `DeniedBeforeExecution`, `FailedBeforeExternalEffect`, `CancelledBeforeStart`,
  `InterruptedBeforeStart`, `OutputLimitExceeded`, `ExecutionUnavailable`,
  `ExternalEffectUnknown`); and
- tool-history replay negotiation (`RunToolHistoryPageDto` /
  `RunToolHistoryCompletedDto`, 256 facts / 512 KiB per page,
  `model_tool_loop_required`).

Each direction keeps M3/M4 behavior authoritative (including the byte-identical
`tool_execution_unavailable` denial), affects fresh runs only after a later
activating specification, and is bound to Milestone 5+ in the roadmap.

## Rationale

The authoritative package review of 2026-08-30 confirmed the base-tool initial
contracts and the loop bounds/terminal taxonomy are present in
`m4plus_concept2.md` but only at principle level in architecture 15. This
decision adopts the detail so the authoritative documentation fully covers the
feature, while preserving the project rule that no feature is documented as
implemented without code evidence.

## Normative invariants

1. The effect profile states only direct declared capability and never itself
   requires confirmation; `process_start` does not claim a shell program cannot
   read/write/start descendants or access network.
2. `execute` runs with the user's ordinary OS authority and `WorkspaceRoot`
   CWD and is not a sandbox; `fetch_url` permits only GET/HEAD over HTTP(S);
   `ask_user` keeps the post-M4 run `Running` and never rewrites M3/M4
   `WaitingInput` semantics.
3. Every accepted fragment commits as its own durable fact before publication;
   a fragment is never model context by itself.
4. Output is never truncated or partly committed; an over-budget fragment
   terminalizes only its own call with `tool_output_limit_exceeded`.
5. An `ExternalEffectUnknown` result never permits another model step.
6. Unnegotiated clients fail closed with `model_tool_loop_required`; historical
   M4 runs retain byte-identical replay and denial.

## Failure semantics

- Malformed fragment streams fail closed as `tool_result_stream_invalid`.
- A next fragment that cannot fit the 4 MiB group budget is not written; only
  its call receives `tool_output_limit_exceeded`; remaining calls continue.
- Missing/incomplete tool history requires typed resynchronization and never
  causes a live-tool retry.

## Compatibility and supersession

This decision supersedes the absence of the detail in architecture 15's
principle-level text. The closed M4 baseline, M3/M4 bytes, and existing behavior
remain unchanged. Activation remains deferred: no code changes are authorized
by this decision.

## Security and residual risk

The contracts remain trusted-local. Tool output and history are bounded and
credential-free; redaction stays central and every activating specification must
pass the fake-secret regression suite.

## Affected documents

- [`architecture/11-implementation-roadmap.md`](../architecture/11-implementation-roadmap.md)
- [`architecture/15-tool-registry-and-mandate-tool-loop.md`](../architecture/15-tool-registry-and-mandate-tool-loop.md)
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

This decision does not implement the contracts; it does not change M3/M4
behavior; it does not renumber M5--M9; it does not activate a crate, schema,
migration, protocol, or feature. Parallel tool execution, tool-failure-to-model
continuation, and Mandate/architecture-15 loop activation remain outside this
decision.
