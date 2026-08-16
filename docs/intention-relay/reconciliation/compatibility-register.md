# Compatibility Register

## Purpose

This register preserves implemented history while describing the compatibility
boundary for future M4+ design. It does not approve a migration, a new record,
or a new decoder.

| Contract family | Implemented baseline to preserve | Future additive boundary | Forbidden reinterpretation | Evidence anchor |
| --- | --- | --- | --- | --- |
| M1/M3 public DTOs | Existing IDs, closed enums, JSON compatibility, `ErrorDto`, protocol requests, snapshots, and config DTOs remain readable under documented semantics. | New public families require explicit schema/version/capability policy. | Do not infer Mandate, Skill, MCP, verifier, child, activity, or execution-kind state into old DTOs. | [DTO policy](../architecture/02-dto-and-contract-policy.md) |
| M3 SQLite history | Schema-v1 projections, events, snapshots, queue tickets, IDs, and config revisions remain unchanged. | A later migration may add records or indexes without rewriting historical payload bytes, IDs, digests, cursors, or events. | Do not reconstruct history from current configuration, registry, model name, ancestry, or policy. | [Sessions and storage](../architecture/04-sessions-runs-events-and-storage.md) |
| M4 durable model history | Schema-v2 run cursors, model facts, snapshots, replay, and run-stream behavior remain M4 behavior. | Future run kinds require additive durable families and negotiated delivery. | Do not attach synthetic facts or reinterpret `tool_execution_unavailable` as tool execution. | [M4 closure evidence](../closeout/m4-closure-evidence.md) |
| M4 selection | `ConfigSnapshotDto`, legacy UUID `ConfigRevisionId`, provider/model/endpoint/policy selection, and queue provenance remain the persisted M4 selection. | A later explicit bridge may reference, but never replace, legacy selection bytes and UUIDs. | Do not reroute old work to a current default, profile, model, or credential state. | [Model providers](../architecture/08-model-protocol-and-providers.md) |
| M4 providers | Only `openrouter` and `generic-chat-completion-api` are M4 kinds. | Future `responses`/profile work needs its own decision and compatibility fixtures. | Do not route by model name or persist `openai` as a new M4 kind. | [M4 charter](../m4.md) |
| Recovery | Unfinished M3/M4 work becomes interrupted before readiness; no provider/tool/process work resumes. | A future Mandate may admit a new run from a retained trigger only after its explicit lifecycle contract exists. | Do not call a fresh run a resumed old run or reattach an external operation. | [Sessions and storage](../architecture/04-sessions-runs-events-and-storage.md) |
| Protocol/replay | M3 session replay and M4 negotiated run streaming remain separate observable contracts. | Future capability families must fail closed for unnegotiated peers. | Do not partially deliver unknown facts or derive state from filtered snapshots. | [Daemon transport](../architecture/03-daemon-transport-and-adapters.md) |
| Credential boundary | Credentials remain absent from storage, events, snapshots, protocol DTOs, errors, logs, diagnostics, and revision identities. | Future selections and attempt evidence remain credential-free. | Do not put endpoints, SDK objects, handles, grants, raw payloads, or secrets in public/durable identity records. | [Configuration security](../architecture/09-configuration-security-and-observability.md) |

## Compatibility law

Future semantics are additive and explicitly versioned. Unknown or corrupt
future execution data preserves unrelated readable history where possible and
blocks only dependent external work before an effect. A future design may not
rewrite historical bytes, assign a new meaning to a closed variant, or use
current mutable state to fill missing historical meaning.

| Execution-kind decoding | M3/M4 records have only their recorded ordinary semantics. | Future envelope is additive, closed, canonical and credential-free. | Do not infer kind/payload from model name, current state, policy or ancestry. | [Execution meaning](../architecture/14-run-execution-meaning-and-historical-compatibility.md) |
| Canonical meaning digest | Historical UUIDs/digests retain original form. | New SHA-256 canonical records bind exact bytes and nested immutable references. | Do not replace a UUID or recompute/overwrite historical identity. | [Execution meaning](../architecture/14-run-execution-meaning-and-historical-compatibility.md) |
| Future bridge | No M3/M4 meaning bridge exists. | A later explicit bridge may reference legacy bytes and UUIDs. | Never synthesize/rebuild a bridge from current TOML or registry. | [Execution meaning](../architecture/14-run-execution-meaning-and-historical-compatibility.md) |
| Provider/profile boundary | M4 provider kinds/selection remain recorded. | Future meaning may freeze credential-free provider selection and driver contract. | Do not infer Responses/profile/kind from a model name or current profile. | [Execution meaning](../architecture/14-run-execution-meaning-and-historical-compatibility.md) |
| External attachment | M3/M4 recovery interrupts unfinished work. | Future decode failure and recovery block dependent work pre-effect. | Do not reattach, rediscover, retry or resume provider/tool/process/kernel/child/MCP work. | [Execution meaning](../architecture/14-run-execution-meaning-and-historical-compatibility.md) |
| UI projection | M3/M4 adapters use their existing DTOs. | Future negotiated projections expose typed supported meaning outcomes only. | Do not synthesize meaning from partial snapshots or current configuration. | [Execution meaning](../architecture/14-run-execution-meaning-and-historical-compatibility.md) |
| M4 tool-call evidence | M4 `ToolCallRecorded` remains evidence followed by `tool_execution_unavailable`. | Future tool-loop facts are additive and separately negotiated. | Do not execute or reinterpret a historical M4 tool call. | [Tool registry and loop](../architecture/15-tool-registry-and-mandate-tool-loop.md) |
| Registry and descriptor revisions | No current M3/M4 registry revision is persisted as execution meaning. | Future canonical revisions bind frozen selected active descriptors. | Do not rebuild a stored selection from the current registry, descriptor, hook, or readiness state. | [Tool registry and loop](../architecture/15-tool-registry-and-mandate-tool-loop.md) |
| WorkspaceRoot split | Ordinary records retain containment semantics. | Future Mandate selection records default-base/CWD semantics and safe observation. | Do not weaken ordinary history or infer Mandate path rules into it. | [Tool registry and loop](../architecture/15-tool-registry-and-mandate-tool-loop.md) |
| Mandate direct admission | Ordinary confirmation/risk policy remains recorded ordinary behavior. | Future Mandate calls use direct typed admission. | Do not infer confirmation, corridor, quota, or root-origin state into Mandate meaning. | [Tool registry and loop](../architecture/15-tool-registry-and-mandate-tool-loop.md) |
| Tool-loop replay | M3/M4 replay contracts remain unchanged. | Future `model_tool_loop_v1` delivery is negotiated and cursor-owned. | Do not partially replay unknown loop facts or retry a tool from history. | [Tool registry and loop](../architecture/15-tool-registry-and-mandate-tool-loop.md) |