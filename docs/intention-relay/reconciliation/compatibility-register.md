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
