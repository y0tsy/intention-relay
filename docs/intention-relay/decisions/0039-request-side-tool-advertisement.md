# ADR 0039: Request-side tool advertisement

## Status

Accepted as a Milestone 5+ retrospective completion of [ADR 0019](0019-production-model-tool-loop.md), within the
retrospective scope of [ADR 0035](0035-m5plus-complete-foundation-activation.md). It completes the ordinary production
model-tool loop on the request side: the daemon now advertises the active registered tools in model requests so real
providers can emit structured tool calls. The M5+ Slice 3 reservations and all reserved tags remain untouched and
reserved.

## Decision

In scope is the ordinary production model-request path only: the provider-neutral request carries typed tool definitions
built from the active registered tool set, the model tool-call capability is requested whenever definitions are present,
and the two current provider adapters (`generic-chat` and `openrouter`) translate the definitions into their private SDK
request. This record supersedes, in place and only for ordinary request translation, the [ADR
0019](0019-production-model-tool-loop.md) non-goal clause that excluded "OpenRouter or OpenAI Responses tool mapping";
ADR 0019's body is unchanged, and an OpenAI Responses driver still does not exist and is not added here.

1. `ModelRequestDto` gains a `tools: Vec<ModelToolDefinitionDto>` field.
`ModelToolDefinitionDto` is provider-neutral and carries exactly `name`, `description`, and `parameters_json`. Because
raw `serde_json::Value` cannot cross a provider boundary ([architecture
02](../architecture/02-dto-and-contract-policy.md)), each schema travels as validated JSON-object text. The DTO
validates its input: the name must be an ASCII `[A-Za-z0-9_-]` token of at most 64 characters
(`invalid_tool_definition_name`), the description must not be blank (`invalid_tool_definition_description`), and
`parameters_json` must be non-empty JSON-object text of at most 64 KiB (`invalid_tool_definition_parameters`).
2. The application's scheduled-run request build path obtains definitions from
`intention_tools::model_visible_descriptors()`, which returns the six `Active` registry entries in registry order:
`read`, `write`, `edit`, `execute`, `glob`, and `grep`. `Reserved` slots are never advertised. Each definition's
parameter document is the tool's code-owned `model_parameters_schema` JSON-schema object text; a model-visible
descriptor without that schema fails with `model_tool_schema_unavailable`.
3. `ModelRequestDto::with_tools` replaces the advertised definitions. A
non-empty replacement forces the requested-capabilities `tool_calls` flag to `true`, so preflight accepts only drivers
that declare tool-call support; an empty replacement leaves the flag unchanged.
4. Both current adapters translate `tools` into their private SDK request
(name, description, and JSON parameter object per definition) and omit `tool_choice`. No adapter forces, ranks, or
preselects a tool.
5. Result continuation is unchanged: assistant tool-call messages and
tool-role result messages already flow through the runtime loop activated by [ADR
0019](0019-production-model-tool-loop.md).

The M5+ Slice 3 reservations remain untouched and reserved: `tool-descriptor-revision` (`0x0301`),
`tool-registry-revision` (`0x0302`), and `model-tool-loop-v1` (`0x0303`) stay `ReservedForSlice3`. No frozen
descriptor/registry revision, canonical digest, execution meaning, or tool selection is introduced, and no wire,
protocol, or storage change is made.

## Invariants

1. Only typed DTOs cross the provider boundary. `parameters_json` is the
single permitted textual schema carrier; raw `serde_json::Value` and loosely typed maps remain forbidden by
[architecture 02](../architecture/02-dto-and-contract-policy.md).
2. Provider SDK request types remain private to their provider crates. No SDK
type, native tool handle, or SDK tool-choice value appears in a public signature.
3. The advertised set is exactly the active registry set in registry order.
That order is frozen and wire-visible: `read`, `write`, `edit`, `execute`, `glob`, `grep`.
4. Advertisement is transient request state. It creates no durable fact, no
digest, no snapshot, no storage row, and no canonical record, and it never reconstructs stored selection from the
current registry.
5. Advertising a tool grants no execution authority. The provider only emits
typed tool calls; the daemon-owned typed registry, workspace, and hook pipeline remain the only execution path.

## Compatibility

Public DTO schema 1.1 is unchanged; the local protocol version clause of this bullet is superseded by [ADR
0045](0045-local-json-rpc-2-0-transport.md) (JSON-RPC 2.0 over NDJSON at protocol version 2.0, no capability plane), and
the record otherwise stands. The provider-neutral request is not a public wire family, and the SQLite storage schema and
canonical records are untouched: no migration, no version change, and no persisted advertisement state. M3/M4 recorded
history, replay, and evidence remain unchanged; the M4 no-port denial fallback was already superseded by [ADR
0038](0038-no-backward-compatibility-and-legacy-removal.md) and is not affected by this record. Drivers that do not
declare tool-call support remain valid for tool-free requests; they fail closed only when tools are advertised.

## Security and failure behavior

- A driver that does not declare `tool_calls` fails closed at preflight with
`unsupported_model_capability` before any outbound request; there is no silent drop or fallback.
- Tool definitions contain no credentials, absolute workspace roots, or
provider secrets; they are code-owned registry text. Malformed or oversized definitions are rejected as validation
errors under the rules above.
- A model-emitted tool call is still validated, admitted, and executed by the
typed daemon pipeline under `WorkspaceRoot` with typed hooks. The advertisement does not bypass policy, cancellation,
persistence, or recovery, and provider adapters never invoke local tools.

## Non-goals

No frozen tool selection, descriptor/registry revision, or canonical digest; no mode-based or risk-based tool
filtering and no executor-side allowlist; no `tool_choice`, forced or ranked tool selection; no parallel tool execution;
no strict JSON schemas or provider-specific dialects beyond the validated JSON-object parameter text; no OpenAI
Responses driver, remote continuation, or MCP/kernel tool advertisement.

## Affected documents

Touched: architecture 02, 05, 07, 08, 10; README index.

## Evidence

Hermetic tests are the acceptance evidence; the change adds no live provider dependency.
`crates/intention-model/tests/model_contracts.rs`: `model_request_tools_round_trip_and_omit_the_empty_field`,
`model_request_with_tools_forces_tool_call_capability`, `model_request_with_messages_preserves_advertised_tools`,
`model_tool_definitions_validate_names_descriptions_and_parameters`; `crates/intention-tools/tests/tool_contracts.rs`:
`model_visible_descriptors_are_the_six_active_tools_in_registry_order`,
`model_parameter_schemas_agree_with_serialized_inputs`, `reserved_slots_have_no_schemas_or_revision`; generic-chat crate
unit tests `generic_chat_advertises_tool_definitions_without_tool_choice` and
`generic_chat_omits_tools_when_no_definitions_are_advertised`; OpenRouter crate unit tests
`declared_tools_are_translated_without_tool_choice` and `requests_without_declared_tools_carry_no_tools_on_the_wire`;
`crates/intention-runtime/tests/m5_tool_loop.rs`: `tool_call_executes_tool_records_result_and_completes` (the follow-up
provider round preserves the advertised definitions); `crates/intention-daemon/tests/m5_tool_loop_wiring.rs`:
`daemon_tool_executor_executes_real_read_tool_through_loop` (every model-visible definition is observed in registry
order); `crates/intention-daemon/tests/facade_e2e.rs`: `real_daemon_tool_loop_executes_read_and_replays_after_restart`
(the captured first provider request body advertises tools including `read`). Reconciliation evidence: EVD-062
(`reconciliation/evidence-register.md`; `Verified` by the recorded `make verify` and CI results). Gates: `make quick`,
`make verify`, `docs-check`, Linux/Windows CI.

## Research provenance

`m4plus_concept.md` selected base-tool contracts and unified registry, descriptor/registry revisions, and selected
foundational model-tool loop sections, filtered through [ADR 0019](0019-production-model-tool-loop.md) (ordinary loop
activation), [ADR 0025](0025-base-tool-contracts-and-tool-loop-bounds.md), and the M5+ Slice 3 reservation ledger.
