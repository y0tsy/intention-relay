# 0014: Provider Evolution, Profiles, and Reasoning

## Status

Accepted.

## Decision

Future provider evolution uses immutable credential-free provider and
model-capability selections under execution-meaning fields 2 and 3. Future
canonical Responses support is kind `responses`; `openai` is only a future
parse-time compatibility alias and never a persisted/provider-routing identity.

Provider profiles, kind descriptors, catalog revisions, endpoint/credential
transport metadata, capability intersections, reasoning policy, and driver
contracts are selection/compatibility evidence, never lifecycle, scheduler,
tool, child, verifier, MCP, bridge, kernel, context, or reconciliation authority.

## Invariants

- M4 keeps only `openrouter` and `generic-chat-completion-api`; model IDs never
  select kind, driver, endpoint, capability, or execution kind;
- architecture 14 remains the sole owner of `IRCR` canonical framing, digest,
  and decoder policy; provider semantics cannot introduce another codec;
- current configuration, catalog, default, model, endpoint, credential, or
  driver cannot reconstruct, reroute, or replace stored provider meaning;
- `responses` is local-history-first with `store: false`, no remote continuation,
  encrypted/opaque reasoning, provider-managed history, or built-in tools;
- provider availability only retains/re-evaluates a Mandate reason before fresh
  admission; recovery never resumes, reattaches, retries, or dispatches old
  provider work; and
- reasoning normalization cannot choose or disclose context sources, audiences,
  or model projections, which remain architecture-21 authority.

## Compatibility and non-goals

M3/M4 bytes, UUIDs, config snapshots, provider kinds, retries, facts, cursors,
replay, recovery, and tool-denial evidence remain unchanged. Historical records
gain no synthetic profile, catalog, Responses, capability, reasoning-summary, or
execution-kind state.

This decision activates no SDK, crate, parser, TOML schema, storage migration,
protocol, profile UI, quality policy, or production behavior. Credential
rotation, keychains, discovery, health tests, pricing, live reload, multimodal,
structured output, raw templates, plugin drivers, session branching, and UI
remain separate.

## Primary owner and evidence

[Provider evolution, profiles, and reasoning](../architecture/22-provider-evolution-profiles-and-reasoning.md)
owns detailed behavior. A later implementation requires canonical, provider,
catalog, reasoning, recovery, protocol, compatibility, redaction, cross-platform,
and outcome evidence through the standard quality gate.

## Provenance

`m4plus_concept2.md`, selected provider contracts, profiles, Responses, and
reasoning material, reconciled against architectures 8 and 13--21.
