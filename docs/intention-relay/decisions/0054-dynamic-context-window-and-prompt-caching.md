# ADR 0054: Dynamic context window and prompt-cache breakpoints

## Status

Accepted 2026-10-05. It replaces the context-immutability reading of model-step context with a mutable window over the
provider request, adds ephemeral prompt-cache breakpoints, and records the two provider configuration keys and their
validation errors that bound the window. It authorizes no new durable record, event family, storage schema, or wire
version.

## Scope and supersession

In scope: the provider request message list; the token estimate and its calibration; tool-result compression; the
capacity compression stub; prompt-cache breakpoints and their driver translation; and the `context_window_tokens` and
`context_capacity_tokens` configuration keys.

Out of scope: durable tool-result content and every durable fact; compaction semantics; Goal, Skill, memory, and
instruction-profile semantics; the instruction projection and its freeze at admission; provider retry and timeout
policy; the tool loop and its terminal vocabulary; and recorded M3/M4 bytes.

| Record | Superseded clause | Replaced by |
| --- | --- | --- |
| [ADR 0043](0043-instruction-sources-and-system-context.md) Decision, delivery clause | The clause read "with no driver-specific framing, rewrite, caching directive, or provider-side scan" | The projection is delivered as the leading system message with no driver-specific framing, rewrite, or provider-side scan, and the daemon-owned request marks the end of that instruction block with an ephemeral prompt-cache breakpoint; the marker adds no text and changes no projection |
| [ADR 0013](0013-goals-skills-context-memory-and-compaction.md) Decision, Skills clause | "each affected model step binds an immutable safe context projection" | Each affected model step binds a safe context projection; the provider request built from it is a mutable window under [architecture 08](../architecture/08-model-protocol-and-providers.md), while the manifest, the selections, and every durable fact stay recorded as written |

The named clause of each record is amended in place; every other clause of those records stays as written.

## Decision

The context sent to a provider is a mutable window, not an append-only transcript. Immutability remains only for
durable records: source manifests, model-step selections, memory records, compaction summaries, and the instruction
projection; the in-flight request is windowed and its tool results may be compressed in place.

**Window policy.** The `[provider]` table carries `context_window_tokens` (default `250000`) and
`context_capacity_tokens` (default `1000000`). Resolution requires a positive capacity and
`0 < context_window_tokens < context_capacity_tokens`; a rejected value fails closed with the typed
`invalid_provider_context_window_tokens` or `invalid_provider_context_capacity_tokens` validation error. Both values
resolve into the credential-free `ContextWindowPolicyDto` that `ResolvedConfigDto` and every `ConfigSnapshotDto` carry,
and the runtime creates one accounting state per provider attempt from its frozen policy ([architecture
09](../architecture/09-configuration-security-and-observability.md)).

**Token accounting.** The estimate of one request's input is its character count at four characters per token, rounded
up. Provider usage calibrates it: a `UsageDto::Reported` input count replaces the estimate for the request that produced
it, and every later increment is estimated at four characters per token; `UsageDto::NotReported` leaves the whole
estimate character-based.

**Trimming.** While the estimate exceeds `context_window_tokens`, the longest tool-role result that still shrinks is
compressed first, repeatedly. A compressed tool result keeps the first 16 characters of its trimmed text and appends a
fixed compression marker; when nothing printable remains, it becomes a fixed placeholder, so a compressed tool message
is never blank. Messages are never removed or reordered, and the assistant tool-call message and its tool replies stay
paired. The pass runs after the starting context is built and again after every appended tool result. Only the in-memory
request changes: every durable tool-result fact keeps its full recorded content.

**Capacity stub.** An estimate above `context_capacity_tokens` invokes the named `compress_context` pass. That pass is
not implemented: it changes nothing, fails nothing, and records nothing, so an over-capacity estimate stays windowed
exactly like a window-only crossing. Compression remains a future capability with a named owner.

**Prompt-cache breakpoints.** The window pass clears every earlier marker and then marks at most two messages with
`{"type": "ephemeral"}`: the last leading system-role message, when the message list begins with one, closes the leading
system/instruction block, and the last message in the list closes the stable window prefix that the next round repeats.
Each driver translates a marked message into its private cache marker — the generic Chat driver as a message-level
`cache_control`, the OpenRouter driver through the pinned SDK's cacheable content part — and marks the daemon-owned
system context message the same way when the request carries one. A breakpoint is a transient request-assembly hint
with no durable representation; it changes no content, and a trim never leaves a stale breakpoint behind.

## Rationale

- A long tool result can exceed a model's usable input by itself. Compressing the largest tool results first keeps the
  conversation structure, the assistant call/tool-reply pairing, and the recent turns intact while the request returns
  inside the window; removing messages would silently break that pairing.
- Four characters per token is a cheap deliberate estimate, and provider-reported input usage is the only authoritative
  calibration available without shipping a tokenizer. Recalibrating from that report keeps later estimates close to the
  provider's own count.
- The capacity decision is named even though compression is not implemented: it records the intended second stage and
  keeps an over-capacity estimate a no-op instead of an error or an unrecorded discard.
- Breakpoints are computed at the ends of the two prefixes that actually repeat — the instruction block and the window
  prefix — so a provider can reuse them between rounds; recomputing them after every trim prevents a stale breakpoint
  from pointing at content that no longer matches.
- Keeping the compression in the provider-neutral request lets both adapters translate the marker through their own SDK
  mechanisms without leaking SDK types.

## Invariants

1. Messages are never removed, reordered, or unpaired; a compression replaces one tool-role message's content in place.
2. Durable tool-result facts and every other durable record keep their full recorded content; only the in-memory request
   is windowed.
3. The instruction projection is never compressed; it stays the stable leading system block of every request and is
   never re-derived.
4. `context_capacity_tokens` is positive and `context_window_tokens` is positive and below it.
5. The estimate is calibrated by a reported provider input count and is otherwise four characters per token; a
   not-reported usage never claims a provider count.
6. The capacity stub changes nothing: no content is discarded, no failure is raised, and no compression is claimed.
7. Breakpoints are recomputed on every pass and are never stale: the window pass marks at most two messages, and each
   driver adds at most one marker for the daemon-owned system context when the request carries one.
8. No new durable order, event, schema version, or compatibility path is introduced.

## Compatibility

The feature evolves in place under the single live configuration and schema version 1 ([ADR
0038](0038-no-backward-compatibility-and-legacy-removal.md)): the resolved configuration and snapshot shapes gain the
explicit context-window policy with no second version and no migration. M3/M4 requests keep the optional system context
absent, and recorded M3/M4 bytes, meanings, replay, and recovery are unchanged. A configuration document without the
two keys resolves their defaults exactly like a document that names them.

## Security and failure behavior

- The window is a representation bound for the provider request, not authority: it cannot select a provider, widen a
  tool policy, or prove an external effect.
- A cache breakpoint is fixed marker text with no content and no credential; it is a provider request hint and never
  enters a snapshot, event, log, or adapter projection.
- Configuration validation fails closed with a typed error; no invalid window is silently clamped to a default.
- Compression never fabricates output: the retained prefix is source text and the appended marker names the
  compression; the compressed request content is never recorded as the tool result.
- The capacity stub fails nothing and discards nothing, so an over-capacity request is still sent with the window
  applied rather than silently dropped.
- No runtime content scanning is introduced ([ADR 0048](0048-limits-by-precedent-and-no-content-scanning.md)).

## Non-goals

No full compression, summarization, or eviction of messages; no tokenizer or model-specific counting; no prompt-cache
lifetime, scope, or tuning configuration; no provider-native prompt framing or templates; no change to durable fact
content, compaction, or history; no new bound beyond the two configuration keys; no migration, aliasing, or
compatibility layer; no logging facade or warning surface.

## Affected documents

[Architecture 08](../architecture/08-model-protocol-and-providers.md) owns the request mechanics, the accounting, the
trimming pass, the capacity stub, and the breakpoint computation and driver translation. [Architecture
09](../architecture/09-configuration-security-and-observability.md) owns the two configuration keys, their defaults,
and their validation. [Architecture 21](../architecture/21-goals-skills-context-memory-and-compaction.md) owns the
context manifest and projection semantics that the window reads through. [Architecture
30](../architecture/30-instruction-sources-and-system-context.md) owns the instruction projection delivered as the
leading system block whose end carries the instruction-block breakpoint. [ADR
0043](0043-instruction-sources-and-system-context.md) and [ADR
0013](0013-goals-skills-context-memory-and-compaction.md) carry the amended clauses. Secondary cleanup: [architecture
15](../architecture/15-tool-registry-and-model-tool-loop.md) drops the stale 4 MiB group bound that [ADR
0048](0048-limits-by-precedent-and-no-content-scanning.md) already superseded.

## Evidence

The change is accepted only together with: the configuration tests that resolve both defaults and overrides into the
safe snapshot and reject out-of-range values with the typed errors; the model test that round-trips the transient
message marker and the in-place content replacement; the runtime tests that compress the largest tool result first,
walk down to the 16-character floor, keep the assistant call/tool-reply pairing, calibrate from reported usage with
character-based later increments, and request the compression stub for an over-capacity estimate; the runtime
integration test that proves a large tool result is compressed in the continuation request while its durable fact keeps
the full content; the generic Chat and OpenRouter translation tests that assert the message-level and content-part
cache markers; and the documentation and architecture checks. Gates: `make quick`, `make verify`, `docs-check`, `make
architecture`, Linux/Windows CI.
