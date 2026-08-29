# ADR 0019: Production Model-Tool Loop

## Status

Accepted 2026-08-29. This decision records the production daemon behavior for
executing provider-emitted tool calls through the real typed tool registry. It
does not activate Mandate-specific execution meaning; that remains future
architecture-15 scope.

## Decision

The daemon executes provider-emitted tool calls through the production
model-tool loop:

- The runtime records each provider-emitted tool call as a durable typed
  `ToolCallRecorded` model fact before any local effect.
- The daemon supplies a `ToolExecutionPort` backed by the real typed tool
  registry. The application builds the typed invocation, runs it under
  `WorkspaceRoot` with the typed hook pipeline, and persists the bounded,
  credential-free `ToolResultRecorded` fact before publication.
- The provider exchange continues with assistant-tool-call and tool-role
  `ModelMessageDto` values carrying the recorded results, and the run
  terminalizes `Completed` on provider finish.
- Continuation is bounded to eight tool rounds (`MAX_TOOL_ROUNDS = 8`). A
  round past the bound appends the typed terminal failure
  `tool_round_limit_exceeded` and the run terminalizes `Failed`.
- Tool calls are never re-executed after a daemon restart; recorded facts are
  replayed to clients, and interrupted work is never automatically resumed.
- The M4 no-port denial path remains byte-identical: when no tool executor is
  wired, the runtime records the `ToolCallRecorded` facts and appends the
  `tool_execution_unavailable` failure exactly as M4 did.

## Rationale

The M4 boundary recorded typed tool-call evidence and then denied execution.
The typed tool registry, `WorkspaceRoot`, and hook pipeline from M5 provide
the daemon-owned execution path, so the denial boundary can be superseded for
newly admitted ordinary runs while every historical M4 fact and the closed M4
baseline keep their recorded meaning. Persisting before publishing preserves
the atomic-commit law, and replaying recorded calls without re-executing them
preserves the no-resume rule.

## Normative invariants

1. Only typed DTOs cross the model, runtime, domain, and storage boundaries;
   provider SDKs remain private to their provider crates.
2. Provider adapters never invoke local tools; the daemon-owned runtime and
   application own invocation, execution, and continuation.
3. `ToolCallRecorded` and `ToolResultRecorded` facts commit before the
   correlated result reaches the publication boundary.
4. Tool results are bounded and credential-free: no credentials, absolute
   workspace roots, or OS error strings enter durable facts or model-context
   payloads.
5. Cancellation suppresses continuation and later stream activity after the
   durable cancelling commit.
6. A started tool operation without terminal proof is never retried, resumed,
   or treated as rolled back.
7. The provider exchange continues only with assistant-tool-call and
   tool-role messages built from recorded calls and results.
8. The loop is bounded by the immutable run execution policy; the typed
   `tool_round_limit_exceeded` failure is terminal.
9. M3/M4 historical bytes and the closed M4 baseline remain unchanged; the
   no-port denial path stays as the M4-compatible fallback.

## Failure semantics

- Invalid tool input, workspace denial, or hook denial produces a typed failed
  `ToolResultRecorded` fact and the run terminalizes `Failed` without retry.
- A tool infrastructure error produces a safe normalized failure without
  leaking provider or OS text.
- A provider failure after tool rounds is terminal; it never re-executes or
  replays recorded tool calls.

## Compatibility and supersession

This decision supersedes the M4 denial-only boundary for newly admitted
ordinary runs. The closed M4 baseline, [M4 Closure Evidence](../closeout/m4-closure-evidence.md),
and historical M4 `ToolCallRecorded` facts remain unchanged, and the no-port
denial path continues to behave byte-identically to M4. Mandate-specific tool
loop execution meaning remains future architecture-15 scope.

## Security and residual risk

This decision intentionally accepts:

- `WorkspaceRoot` is a filesystem and CWD boundary, not a sandbox; `execute`
  is trusted-local and may interact with the wider user environment;
- a TOCTOU residual between workspace validation and the filesystem
  operation, narrowed but not eliminated by repeated symlink metadata checks;
- prompt injection and adversarial provider content may influence which
  registered tool is called; the fixed registry and typed invocation remain
  the only execution path.

Durable facts and model-context payloads never contain credentials, absolute
workspace roots, or OS error strings.

## Affected documents

- `architecture/04-sessions-runs-events-and-storage.md`
- `architecture/05-tools-workspace-and-hooks.md`
- `architecture/08-model-protocol-and-providers.md`
- `architecture/10-test-driven-delivery-and-verification.md`
- `architecture/11-implementation-roadmap.md`
- `closeout/m5-closure-evidence.md`
- `reconciliation/README.md`
- `reconciliation/contradiction-register.md`
- `reconciliation/concept-supersession-index.md`

## Required evidence

The activation must run `make quick`, `make verify`, and the required
Linux/Windows CI matrix. The following tests prove the loop:

- runtime `m5_tool_loop` tests: `tool_call_executes_tool_records_result_and_completes`,
  `multiple_tool_calls_execute_sequentially_in_provider_order`,
  `repeated_tool_rounds_continue_until_finished`,
  `tool_round_limit_terminalizes_typed_failure`,
  `tool_failure_records_result_and_terminalizes_without_retry`,
  `port_infrastructure_error_terminalizes_without_leaking_text`,
  `cancellation_during_tool_execution_suppresses_continuation`, and
  `no_port_preserves_m4_denial`;
- wiring `m5_tool_loop_wiring` tests:
  `daemon_tool_executor_executes_real_read_tool_through_loop` and
  `daemon_tool_executor_missing_file_returns_typed_failure`;
- the facade-level daemon-host E2E scenario
  (`crates/intention-daemon/tests/facade_e2e.rs`): real binary over IPC, fake
  provider tool call, real registry execution, durable result, provider
  continuation, `Completed`, restart replay, and no re-execution;
- durable fact and redaction assertions in `m4_durable_facts`,
  `m5_tool_results`, and the SQLite reopen fixtures, proving bounded
  credential-free results and restart durability.

## Non-goals

This decision does not add OpenRouter or OpenAI Responses tool mapping,
parallel tool execution, tool-failure-to-model continuation, configurable
round bounds, or Mandate/architecture-15 loop activation. MCP, kernel, VFR,
Headroom, and Plan features remain outside this decision.
