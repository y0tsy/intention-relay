# Production ceiling removal — scope

**Status:** the domain part (candidate rows 3–8 below) is removed in this PR
(`df1e8a1`, `794b41c`). Tools (rows 1–2) and config/runtime (rows 9–10) remain
undecided; the audit said tools cuts carry context/memory risk and the runtime
timeout is retained by concept2.

## Purpose

Remove product ceilings (product counters, reservations, quotas) from
post-M4 production code where [`m4plus_concept2.md`](m4plus_concept2.md) marks
them as forbidden for new Mandate work or as historical-only. Intrinsic
correctness bounds and capacity availability must remain untouched.

## Classification (concept2)

- `ProductCeiling` — forbidden for new Mandate admission: "A fixed number of
  calls, queued reasons, agents, child depth, messages, lifetime, output
  bytes, page items, retries, catalog entries, or calendar actions must not
  silently define what an otherwise valid Mandate may accomplish" (L335–336).
- `IntrinsicBound` — mandatory correctness boundary of the canonical
  representation, identifier, schema, ordering, framing, or atomic commit; it
  remains mandatory and rejects without truncation (L306–311).
- `CapacityAvailability` — temporary finite resource availability; typed
  `Unavailable` outcome, never a quota or a successful result (L313–321).
- Numeric constraints retained later in concept2 are historical first-scope
  semantics and compatibility data, not future admission policy (L337–338);
  "Retained policy inheritance and product ceilings are historical-only for
  new Mandate execution" (L5175).

## Candidate ceilings (TBD)

| # | Crate | Cap | Value | Kind | Notes |
|---|---|---|---|---|---|
| 1 | intention-tools | `MAX_TOOL_OUTPUT_BYTES` with `[truncated]` marker and truncate-with-flag on read/execute/grep | 64 KiB | ProductCeiling | lib.rs L52; forbidden per L335–336, historical-only per L337–338 |
| 2 | intention-tools | `MAX_GLOB_MATCHES` / `MAX_GREP_MATCHES` (page items) | 10 000 | ProductCeiling | lib.rs L54–55, L937–945 |
| 3 | intention-domain | `ToolLifecycleEventDto.detail` inline cap | 4 KiB | ProductCeiling | **REMOVED** in `df1e8a1` |
| 4 | intention-domain | `MAX_TOOL_RESULT_CONTENT_BYTES` | 4 KiB | ProductCeiling | lib.rs L938, L1110; **REMOVED** in `df1e8a1` |
| 5 | intention-domain | `MAX_TOOL_RESULT_METADATA_ENTRIES` | 16 | ProductCeiling | lib.rs L940, L1112; **REMOVED** in `df1e8a1` |
| 6 | intention-domain | `MAX_TOOL_RESULT_METADATA_KEY_BYTES` | 128 | ProductCeiling | lib.rs L942, L1010; **REMOVED** in `df1e8a1` |
| 7 | intention-domain | `MAX_TOOL_RESULT_METADATA_VALUE_BYTES` | 1024 | ProductCeiling | lib.rs L944, L1011; **REMOVED** in `df1e8a1` |
| 8 | intention-domain | `ToolResultOutcomeDto::succeeded` reuses the 4-KiB content cap | 4 KiB | ProductCeiling | **REMOVED** in `794b41c` |
| 9 | intention-config / intention-runtime | `max_attempts` (default 2, schema cap 1..=2) | 2 | ProductCeiling | retry counter, listed at L335; historical-only; config lib.rs L604–637 |
| 10 | intention-config / intention-runtime | `attempt_timeout_seconds` (default 30, schema cap 1..=60) | 30 | confirm | timeout; concept2 retains attempt timeouts (L709); likely out of scope |

Line numbers refer to `origin/main` at `49d6b5a`.

## Watch items (classify before deciding)

- `intention-application` `MAX_DURABLE_TOOL_RESULT_BYTES` = 512 KiB — durable
  tool-result bound; gray zone, needs classification.
- `intention-storage` `MAX_TOOL_RESULT_CONTENT_BYTES` = 512 KiB (lib.rs
  L90, L174) — distinguish from the retained 256-facts / 512-KiB page bounds
  (L912, L4586).
- concept2 L7598: per-run output and required transferred history are each
  capped at 4 MiB — a defined first-scope accounting limit; confirm in or out
  of scope.

## Explicitly out of scope (retained)

- Intrinsic bounds: NUL/blank/unique-key validation on tool-result metadata,
  canonical encodings, identifier widths, ordering, idempotency, atomic
  durable commit, protocol framing (L306–311).
- Capacity availability: typed `Unavailable` outcomes for finite runtime,
  storage, provider, registry, process, kernel, or scheduler availability
  (L313–321).
- Attempt timeouts (L709); slow-peer 64-frame / 10 s client bounds.
- History page bounds: at most 256 facts and 512 KiB per page (L912, L4586).

## Provenance

The candidate list comes from an audit of the 19 post-M4 production files
against the M4 baseline `d2a85370` using the classification above. A first cut
was attempted and then fully reverted inside PR #14 (`ce80989` →
`cc4bcb3`); this PR is the new vehicle for that work.
