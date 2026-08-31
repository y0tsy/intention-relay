# ADR 0036: M5+ Slice 1 contract ledger

## Status

Accepted as the Slice 1 activating specification required by [ADR 0035](0035-m5plus-complete-foundation-activation.md).

## Scope and supersession

This record freezes and activates only M5+ Slice 1, Contracts and versions. It
extends ADR 0035 Slice 1 and does not activate slices 2–4, M6, M7, M8, or M9.

## Version ledger

| Contract | Version/status |
| --- | --- |
| Local protocol | 1.1 |
| Public DTO schema | 1.1, additive |
| TOML configuration schema | 1, unchanged |
| SQLite storage schema | 3, unchanged; no Slice 1 migration |

## Negotiated capabilities and failure semantics

The additive capabilities are `provider_profiles_v1`, `session_fork_v1`,
`normalized_reasoning_stream_v1`, `agent_activity_v1`,
`user_notifications_v1`, `daemon_tool_gateway_v1`, and `model_tool_loop_v1`.
The effective set is the intersection of both peers’ hello capabilities.
Duplicate entries reject with `duplicate_protocol_capability`. Dependent work
fails closed before effect when unsupported, using the family errors
`provider_profiles_capability_required`, `session_fork_capability_required`,
`normalized_reasoning_stream_required`, `agent_activity_capability_required`,
`user_notifications_capability_required`, `daemon_tool_gateway_capability_required`,
`model_tool_loop_required`, and `execution_meaning_capability_required`. There
is no partial contract or partial effect.

## Canonical codec and identity

`intention-domain` owns the canonical codec and semantic canonical records.
The format is `IRCR` / `typed-tlv-v1` / SHA-256. Digest text is
`<namespace>:sha256:<64 lowercase hex>`; new identities are
`sha256-v1:<64 hex>`. A digest excludes its own field. Digest inputs exclude
credentials, paths, display data, readiness, and current state.

## Numeric tag registry

`intention-domain` owns this registry:

| Tag | Value |
| --- | --- |
| `run-execution-meaning` (records v3, v4) | `0x0101` |
| `programmatic-caller-policy-selection-v1` | `0x0201` |
| `agent-activity-selection-v1` | `0x0202` |
| `goal-run-selection-v1` | `0x0203` |
| `continual-harness-selection-v1` | `0x0204` |
| `mcp-method-catalog-selection-v1` | `0x0205` |
| `model-capability-taxonomy-v1` | `0x0206` |
| `provider-profile-revision-v1` | `0x0207` |
| `provider-selection-v1` | `0x0208` |
| `reasoning-history-manifest-v1` | `0x0209` |
| `context-source-manifest-v1` | `0x020A` |
| `model-context-projection-v1` | `0x020B` |
| `legacy-m4-selection-binding` | `0x020C` |
| `tool-descriptor-revision` | `0x0301` |
| `tool-registry-revision` | `0x0302` |
| `model-tool-loop-v1` | `0x0303` |
| `bridge-invocation-v1` | `0x0304` |
| `fork-base-snapshot-v1/v2` | `0x0401` |
| `fork-preview-v1/v2` | `0x0402` |
| `fork-command-v1` | `0x0403` |
| `agent-activity-tree-v1` | `0x0501` |
| `agent-activity-pair-v1` | `0x0502` |
| `agent-message-v1` | `0x0503` |
| `agent-activity-journal-record-v1` | `0x0504` |
| `agent-notification-record-v1` | `0x0505` |

## Execution-meaning records

`run-execution-meaning-v3` has field tags 1–10; v4 has tags 1–11 and adds
`agent_activity_selection` at tag 11. Envelope tags 1–6 are, in order,
`execution_kind`, `meaning_record_tag`, `meaning_record_version`,
`canonicalization_version`, `canonical_meaning_bytes`, and
`canonical_meaning_digest`. Execution kinds are closed:
`Ordinary`, `Mandate`, and `VerifierMandate`.

`ProgrammaticCallerPolicySelectionV1` uses tags 1–5. `AgentActivitySelectionV1`
has Root and Descendant six-field variants. Its fixed limits are 1024 messages,
4 MiB aggregate, 4096 journal records, 64 KiB per record, 256 records/512 KiB
per page, 16 references, and a 60-minute clarification wait.

## Ownership and preservation invariants

Semantic canonical records/tags belong to `intention-domain`; public wire and
frames to `intention-protocol`; storage contracts/migrations to
`intention-storage` and `intention-storage-sqlite`; registry/typed tool
contracts to `intention-tools`; provider-private translation to provider
crates; process/publication to `intention-daemon`; concrete assembly to
`intention`; and adapters to `intention-client`, then TUI/Tauri. No new crate,
dependency, feature, coverage tier, or exclusion is introduced. Skeleton
`intention-headroom`, `intention-plans`, `intention-vfr`, and `intention-tauri`
remain untouched, and M6–M9 behavior is untouched.

M3/M4 config revisions, snapshots, sessions, runs, events, cursors, queue
tickets, and bytes remain authoritative and unchanged. Historical runs receive
no synthetic post-M5 records; current state is never reconstructed. A
`LegacyM4SelectionBindingDto` references legacy bytes without rewriting them
(`legacy-uuid:<canonical UUID>`).

## Evidence and non-goals

Required evidence covers DTO round trips; canonical golden bytes/digests;
compatible-minor, incompatible-major, and unnegotiated fail-closed negotiation
fixtures; M3/M4 preservation; future-schema rejection; fake-secret absence;
and cross-platform determinism. Required gates are `make quick`, `make verify`,
`docs-check`, and Linux/Windows CI.

This ledger does not implement M6–M9 behavior and does not introduce a second
runtime, registry, scheduler, persistence authority, or sandbox.
