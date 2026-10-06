# 0014: Provider Evolution, Profiles, and Reasoning

## Status

Accepted.

## Decision

Future provider evolution uses immutable credential-free provider and model-capability selections under
execution-meaning fields 2 and 3. Future canonical Responses support is kind `responses`; `openai` is only a future
parse-time compatibility alias and never a persisted/provider-routing identity.

Provider profiles, kind descriptors, catalog revisions, endpoint/credential transport metadata, capability
intersections, reasoning policy, and driver contracts are selection/compatibility evidence, never lifecycle, tool, MCP,
bridge, kernel, context, or reconciliation authority.

## Invariants

- M4 keeps only `openrouter` and `generic-chat-completion-api`; model IDs never
select kind, driver, endpoint, capability, or execution kind;
- records are typed serde JSON (ADR 0046); the removed `IRCR` canonical
framing, digest, and decoder policy is not revived, and provider semantics cannot introduce another codec;
- current configuration, catalog, default, model, endpoint, credential, or
driver cannot reconstruct, reroute, or replace stored provider meaning;
- `responses` is local-history-first with `store: false`, no remote
continuation, encrypted/opaque reasoning, provider-managed history, or built-in tools;
- recovery never resumes, reattaches, retries, or dispatches old provider work; and
- reasoning normalization cannot choose or disclose context sources, audiences,
or model projections, which remain architecture-21 authority.

## Compatibility and non-goals

M3/M4 bytes, UUIDs, config snapshots, provider kinds, retries, facts, cursors, replay, recovery, and tool-denial
evidence remain unchanged. Historical records gain no synthetic profile, catalog, Responses, capability,
reasoning-summary, or execution-kind state. Credential rotation, keychains, discovery, health tests, pricing, live
reload, multimodal, structured output, raw templates, plugin drivers, session branching, and UI remain separate.

Owner: architecture 22. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).

Provenance: `m4plus_concept.md`, selected provider contracts, profiles, Responses, and reasoning material, reconciled
against architectures 8, 14, 15, and 18--21.
