# ADR 0025: Post-M5 Base-Tool Contracts and Tool-Loop Bounds

## Status

Accepted 2026-08-30. This decision records the base-tool initial contracts and
the model-tool-loop fragment/terminal/bounds detail as accepted future
directions. It does not activate implementation: the contracts are bound to
[Milestone 5+](../architecture/11-implementation-roadmap.md#milestone-5-post-m5-retrospective-alignment)
and are implemented only through a separately accepted activating
specification at the start of that milestone.

Amended 2026-09-30: the numeric tool-group, output, and replay-page bounds and
the capability gate are superseded by
[ADR 0048](0048-limits-by-precedent-and-no-content-scanning.md) and
[ADR 0045](0045-local-json-rpc-2-0-transport.md) as recorded below.

## Decision

The following detail from
[`m4plus_concept.md`](../m4plus_concept.md) is adopted and owned by
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
- the descriptor `model_schema_availability` field (whether an active
  descriptor can supply a code-owned function schema to a compatible model
  subset) and the typed-reference alternatives for non-path tools (typed URL,
  question, todo, retained-content, plan, child-agent, or MCP-method reference);
- the explicit statement that the first scope adds no numeric model-step limit;
- the removal of the first-scope numeric bounds (previously 512 KiB per
  canonical fact, 4 MiB combined per group, and `tool_output_limit_exceeded`):
  no per-fact, per-group, or per-page cap is asserted, and only the transport
  and IO liveness safeguards remain
  ([ADR 0048](0048-limits-by-precedent-and-no-content-scanning.md));
- the closed terminal outcome taxonomy (`Succeeded`,
  `DeniedBeforeExecution`, `FailedBeforeExternalEffect`, `CancelledBeforeStart`,
  `InterruptedBeforeStart`, `ExecutionUnavailable`,
  `ExternalEffectUnknown`), with the former `OutputLimitExceeded` member
  removed together with the numeric output budget
  ([ADR 0048](0048-limits-by-precedent-and-no-content-scanning.md)); and
- tool-history replay negotiation (`RunToolHistoryPageDto` /
  `RunToolHistoryCompletedDto`), including the combined publication-gate order
  when the same subscription also delivers the normalized reasoning stream
  (`RunReplayDto` → reasoning pages/completion → tool pages/completion → live
  frames, with omit-if-absent for either history class).

Each direction keeps M3/M4 behavior authoritative (including the byte-identical
`tool_execution_unavailable` denial), affects fresh runs only after a later
activating specification, and is bound to Milestone 5+ in the roadmap.

## Rationale

The authoritative package review of 2026-08-30 confirmed the base-tool initial
contracts and the loop bounds/terminal taxonomy are present in
`m4plus_concept.md` but only at principle level in architecture 15. This
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
4. Output is never truncated, sampled, or partly committed; a fragment is
   either accepted whole as its own durable fact or not published.
5. An `ExternalEffectUnknown` result never permits another model step.
6. Capability negotiation no longer exists
   ([ADR 0045](0045-local-json-rpc-2-0-transport.md)); historical M4 runs
   retain byte-identical replay and denial.

## Failure semantics

- Malformed fragment streams fail closed as `tool_result_stream_invalid`.
- Missing/incomplete tool history requires typed resynchronization and never
  causes a live-tool retry.
- The removed numeric budgets and the `provider_tool_group_invalid` outcome are
  superseded by
  [ADR 0048](0048-limits-by-precedent-and-no-content-scanning.md); an invalid
  fragment stream still fails before effect through `tool_result_stream_invalid`.

## Compatibility and supersession

This decision supersedes the absence of the detail in architecture 15's
principle-level text. The numeric tool-group, output, and replay-page bounds and
the capability gate are superseded by ADRs 0045 and 0048 as recorded above; the
qualitative contracts, the fragment stream, the effect profiles, and the
terminal taxonomy stand. The closed M4 baseline, M3/M4 bytes, and existing
behavior remain unchanged. Activation remains deferred: no code changes are
authorized by this decision.

## Security and residual risk

The contracts remain trusted-local. Tool output and history are credential-free;
redaction stays central and every activating specification must pass the
fake-secret regression suite.

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
