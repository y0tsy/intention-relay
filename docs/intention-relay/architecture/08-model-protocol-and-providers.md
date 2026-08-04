# Model Protocol and Providers

## Scope

This document defines the provider-neutral model contract and the first provider adapters. It preserves meaningful provider capabilities rather than forcing all models into a lowest-common-denominator Chat Completion abstraction.

## Canonical model contract

`intention-model` owns typed requests, stream events, capabilities, and the
provider-driver trait. `intention-types` owns provider-neutral `UsageDto`,
`FinishReasonDto`, `ToolCallDto`, and `ProviderErrorDto`, which
`intention-model` re-exports for source compatibility. `intention-domain` owns
the durable fact, projection, and replay representation. Domain, storage, and
protocol retain no dependency on `intention-model`.

```text
ModelDriver
  capabilities() -> ModelCapabilitiesDto
  preflight(ModelRequestDto) -> DtoResult<()>
```

M4 defines text-only `ModelMessageDto` context, an optional system context, explicit requested capability flags, and `ModelStreamLifecycleDto`. Providers must emit `Started` first, then zero or more text/reasoning/tool/usage facts, followed by exactly one terminal `Finished` fact. A second start, a fact before start or after finish, duplicate usage, or a second finish fails validation. Runtime-owned execution and delivery of a native stream remain deferred from this foundation. `prepare_request` validates and privately translates a request only; it never performs an outbound provider action in Package 1.

Core DTO families:

| DTO | Responsibility |
| --- | --- |
| `ModelRequestDto` | System context, text messages, requested reasoning/multimodal/tool/vendor-extension capabilities, and run identity. |
| `ModelCapabilitiesDto` | Supported input, output, reasoning, tool, multimodal, vendor-extension, and streaming capability declarations. |
| `ModelEventDto` | Text, reasoning, tool, usage, lifecycle, and provider-normalized stream facts. |
| `ToolCallDto` | Typed tool identity and typed tool input. |
| `UsageDto` | Provider-normalized usage values with explicit unknown/not-reported states. |
| `FinishReasonDto` | Typed terminal reason. |
| `ProviderErrorDto` | Safe normalized failure, retry category, and correlation data. |

Provider SDK types cannot leave their provider crate. The architecture checker permits `openrouter_rs` namespace use only in `intention-provider-openrouter` private implementation and `async_openai` only in `intention-provider-generic-chat`; rustdoc JSON rejects either SDK plus HTTP/runtime resources in every active public API.

## Durable model facts

The model stream remains provider-neutral; durable model evidence is a separate
domain/storage concern. Each atomic M4 batch assigns a dedicated,
monotonically-increasing `RunEventCursorDto` and appends typed domain events for
attempt start/failure, retry, assistant content, tail-only reasoning, usage,
tool call, finish, or safe failure. Attempts are positive; retry next attempt
is exactly failed attempt plus one; assistant batches are non-blank and at most
4 KiB; and a terminal run accepts no new facts. Individual canonical facts are
limited to 512 KiB. The safe run snapshot includes compatible M3 projection
identity/status/revision, accumulated assistant content for the active
assistant turn, usage, finish reason, and safe failure; it never accumulates
reasoning content.

Storage appends a non-empty fact batch only when its expected cursor matches,
optionally changing status in the same transaction with domain-event envelope,
index, run cursor/projection, session snapshot, and M4 run snapshot. Stable
errors are `run_fact_too_large` (validation/never),
`run_event_cursor_conflict` (conflict/immediate), `run_replay_not_found`
(not-found/never), `invalid_run_event_cursor` (validation/never), and
`run_history_unavailable` (unavailable/manual). Internal replay reads are
run-scoped and bounded; M3 public protocol subscription behavior remains
unchanged.


```mermaid
sequenceDiagram
  participant R as Run actor
  participant M as Model contract
  participant P as Provider driver
  participant X as Provider API

  R->>M: ModelRequest DTO
  M->>P: Normalized request
  P->>X: Provider-native request
  X-->>P: Native stream
  P-->>M: ModelEvent DTO stream
  M-->>R: Typed events
```

The provider adapter translates native formats into canonical DTOs. It must not erase a capability merely because another current provider lacks it. Instead, `ModelCapabilitiesDto` describes support and application/runtime decides whether a requested feature is valid.

## Initial provider drivers

### OpenRouter

`intention-provider-openrouter` uses `openrouter-rs` 0.14.0 privately.

It owns:

- OpenRouter configuration translation;
- private SDK request construction and fixture normalization for text, reasoning, usage, finish, tool-call, and error facts;
- provider-specific model discovery/capability metadata where available; and
- safe diagnostics and correlation identifiers.

Package 1 does not start an SDK stream or make an outbound request; runtime-owned execution and stream delivery are deferred.

### Generic Chat Completion

`intention-provider-generic-chat` supports compatible Chat Completion-style endpoints using `async-openai` 0.29.3 privately with its configured-base-URL streaming support. It does not implement a custom HTTP or SSE parser.

It owns:

- generic endpoint/auth/config translation;
- private SDK request construction and fixture normalization for text, usage, finish, function-style tool-call, and error facts;
- documented capability limitations; and
- normalized failures.

Package 1 does not start an SDK stream or make an outbound request; runtime-owned execution and stream delivery are deferred.

Provider/model selection is explicit configuration. During M4, the only configuration kind strings remain `openrouter` and `generic-chat-completion-api`; the latter preserves any non-blank model ID without model-name classification. `openai` is not an M4 configuration kind and requires a separately declared OpenAI Responses driver crate and contract decision before it is introduced. The generic provider accepts only text context/output, usage, finish reasons, and function-style tool calls; reasoning, multimodal, and vendor extensions fail preflight before any outbound request is prepared. OpenRouter declares text, reasoning, tool-call, and streaming capability while its M4 foundation rejects multimodal context. Execution-time capability behavior belongs to the selected provider driver and runtime policy.

## Provider selection

Resolved TOML configuration selects a provider and model. M1 defines a serializable, credential-free `ConfigSnapshotDto` foundation containing a `ConfigRevisionId`, capture timestamp, and redacted resolved selection. M1 does not persist revisions, apply daemon reload, or attach snapshots to runs; M3/M4 own those workflows. A later run receives the immutable snapshot selected at startup.

Provider/model selection changes do not mutate an already-started run. They apply to a later run, except if a future explicit, tested runtime transition is introduced.

## Streaming and tool calls

The model stream can emit content, reasoning, tool-call, usage, and terminal events. Runtime owns ordering, stable assistant-turn identity, persistence, and conversion of a model tool call into typed tool execution.

Provider drivers do not invoke local tools directly.

## Retry and timeout ownership

- Provider crates classify native failures into `ProviderErrorDto`.
- Runtime/application policy determines whether an error is retryable for the run.
- Config snapshots define timeout/retry limits applied to the run.
- A retry must produce explicit events and preserve causal relation to the originating model turn.
- A daemon restart does not retry an in-flight provider request.

Exact backoff values and retryable status mapping are implementation-required before production use.

## Required tests and outcomes

| Requirement | Test evidence | Observable outcome |
| --- | --- | --- |
| SDK isolation | Compile/dependency test. | OpenRouter SDK types do not escape provider crate public API. |
| Event normalization | Provider fixture stream tests. | Equivalent native sequences map to valid ordered `ModelEventDto` values. |
| Capability check | Application/runtime test. | Unsupported requested feature fails before an invalid provider call. |
| Tool call boundary | Runtime/provider integration test. | Provider emits a tool-call DTO; only runtime invokes the tool registry. |
| Provider selection | Configuration contract test. | A configured provider and model ID are preserved. |
| Retry | Controlled provider failure test. | Retry lifecycle is typed, bounded, and durable. |
| Secret redaction | Provider error fixture. | API key never appears in `ProviderErrorDto`, logs, or events. |

## Quality-gate integration

`intention-model` and provider adapters are Tier C coverage targets. Stream normalization, capability validation, retry, SDK-isolation, and secret-redaction fixtures are blocking `make verify` inputs under every relevant feature profile. Dependency and public-API checks must prevent provider SDK types and secrets from escaping their crate. See [12 Quality Gates and Makefile](12-quality-gates-and-makefile.md).

## Non-goals

- A provider-agnostic interface that discards reasoning, tool, or native capabilities.
- Direct SDK use from application, runtime, transport, or adapters.
- Full provider catalog in v1.
