# ADR 0043: Instruction sources and system context

## Status

Accepted 2026-09-26 as a documentation-approved future direction within the retrospective scope of [ADR
0035](0035-m5plus-complete-foundation-activation.md). It extends [ADR
0017](0017-build-autopilot-and-plan-focus-continuity.md) by giving the Plan focus instruction one typed owner, respects
the context boundary of [ADR 0013](0013-goals-skills-context-memory-and-compaction.md) without changing its Skill,
memory, or projection rules, and amends ADR 0035 by adding a fifth activating slice to the Milestone 5+ sequence. It
authorizes no crate, DTO, tag, wire, storage schema, configuration field, UI page, migration, feature profile,
quality-policy target, or production behavior, and it is not the Slice 5 activating specification.

**Approved future design. Not implemented; activation requires an activating specification.**

Amended 2026-09-30: the intrinsic numeric bounds are superseded by [ADR
0048](0048-limits-by-precedent-and-no-content-scanning.md), and the digest wording and the workspace-boundary failure
condition are read through [ADR 0046](0046-typed-serde-json-contracts.md) and [ADR
0047](0047-workspace-root-addressing-anchor.md) as recorded below.

## Scope and supersession

This decision defines the one instruction channel of a model request: which instruction sources exist, how they become
an immutable effective instruction projection, how that projection is bounded, materialized, and observed, and why
instruction text never carries authority. It names [architecture
30](../architecture/30-instruction-sources-and-system-context.md) as the detailed owner and the delivery home as the
fifth activating slice of Milestone 5+. It supersedes nothing else; it amends two recorded positions: [architecture
21](../architecture/21-goals-skills-context-memory-and-compaction.md)'s documentation-only exclusion that leaves prompt
assembly undefined (instruction assembly is owned by architecture 30, while architecture 21 keeps Goals, Skills, context
manifests, memory, and compaction), and the slice sequence of [ADR 0035](0035-m5plus-complete-foundation-activation.md),
whose fifth slice this decision declares.

The M3/M4 baseline, the closed M4 charter, the current model contract, and the recorded `system_context` channel keep
their meaning. This decision answers a concrete gap: the legacy Antibusy system assembled a static prompt set for every
session, the current project has no owner for that capability, and the `effective_instruction_projection` fields of the
fork records are defined nowhere.

## Decision

The instruction channel is a closed set of typed instruction sources assembled, once per admitted run, into one
immutable effective instruction projection. [Architecture
30](../architecture/30-instruction-sources-and-system-context.md) owns the typed records (`InstructionSourceV1`,
`InstructionProfileRevisionV1`, `InstructionProjectionV1`), the source tables, the assembly separators, and the field
lists.

- **Source kinds are closed**: `Identity`, `Guidelines`, `ToolUsage`,
`CodingConventions`, `Custom`, `ProjectInstructions`, `Mode`, and `Vfr`. The first four mirror the legacy static set and
are adapted, never consumed unchanged; `Mode` and `Vfr` are reserved to their architecture owners.
- **Source scopes are closed**: `Deployment` (daemon-packaged defaults;
enable/disable only), `User`, `Project`, and `Session` (durable configuration edited through the typed control-plane
surface of architecture 25). Exactly one live configuration format version exists ([ADR
0038](0038-no-backward-compatibility-and-legacy-removal.md)).
- **Workspace project instructions**: `AGENTS.md` at the session's
`WorkspaceRoot` is the only file-based instruction source, addressed by workspace path, read as text, never executed,
and never treated as authority ([ADR 0047](0047-workspace-root-addressing-anchor.md)); an absent file contributes
nothing and is not a failure.
- **Fragment profile**: all configured fragments form an immutable
`InstructionProfileRevisionV1`; creating, editing, reordering, enabling, disabling, or re-scoping a fragment produces a
new profile revision identity.
- **Canonical assembly order** is fixed: `Deployment`, `User`, `Project`, and
`Session` fragments in declared order, then `ProjectInstructions`, `Mode`, and `Vfr`; order is part of the projection
identity.
- **The effective instruction projection is frozen at admission**, before the
first model step of a run. Later model steps reuse it; a fork inherits the materialized projection verbatim; a plan
handoff materializes it into its frozen snapshot; and no run, fork, regeneration, or replay re-derives instructions from
current configuration, current `AGENTS.md` content, or current session state.
- **Delivery uses the existing `system_context` channel**: the daemon passes
the assembled projection as the request's optional system context, and the drivers translate it into the leading system
message with no driver-specific framing, rewrite, caching directive, or provider-side scan.
- **Materialization**: the projection is the contents of the
`effective_instruction_projection` and `materialized_effective_instruction_projection` fields of the fork records
([architecture 23](../architecture/23-non-destructive-session-branching-and-regeneration.md)), and its revision identity
is recorded as safe usage provenance.
- **Required editing surface**: the control plane lists, creates, edits,
duplicates, enables, disables, reorders, and re-scopes fragments, rejects invalid edits with typed errors, shows the
profile revision identity, and previews the effective projection for a chosen session, policy, and mode without
admitting a run; TUI/REPL remains contract-equivalent.
- **Numeric bounds** are not fixed now: any future character, fragment-count,
or size bound requires a recorded precedent under [ADR 0048](0048-limits-by-precedent-and-no-content-scanning.md), no
text is truncated, sampled, or silently replaced by a previous revision, and an unrepresentable, inconsistent, or
unreadable source fails closed before admission.
- **Observability**: logs, activity, notification, and audit surfaces carry
revision identities only; instruction text stays on the configuration surface where the user edits it.

## Invariants

1. No authority. Instruction text grants no tool, policy, Mandate, child,
verifier, MCP, bridge, kernel, provider, scheduler, reconciliation, or confirmation authority, and it cannot prevent or
prove the absence of any external effect. The advisory rule of ADR 0017 is unchanged.
2. One channel. Exactly one effective instruction projection exists per
admitted run, it has exactly one owner, and no second prompt-assembly path, hidden system text, or provider-supplied
instruction may be added beside it.
3. Channel closure. Skill bodies, memory records, tool output, repository
content, fetched material, provider output, and model output never enter the instruction channel; they remain
context-manifest, Skill-disclosure, or evidence material under architecture 21.
4. Immutability. Profile revisions and materialized projections are immutable;
a change creates a new revision; forks and historical records are never rewritten or reconstructed.
5. Determinism. The same profile revision, project instruction source, mode,
and configuration produce the same ordered projection and the same projection identity; order and separators are fixed.
Byte-level canonicalization is settled at activation under [ADR 0046](0046-typed-serde-json-contracts.md).
6. Safety. The projection is credential-free and safe to record; no
instruction source may carry a secret, a credential, raw provider or tool payload, an implementation resource, or
executable content.
7. Fail closed. A missing, unreadable, or inconsistent configured source
blocks admission with a closed typed failure; no fallback, partial assembly, or silent omission is permitted.
8. Fresh runs only. Instructions affect newly admitted runs. M3/M4 runs,
retained history, and recorded bytes gain no instruction state.

## Compatibility

Historical M3/M4 requests keep the optional `system_context` absent, and existing runs, events, snapshots, replay,
recovery, and tool denial keep their recorded meaning. The mechanism adds no second model protocol, no second storage
schema, no new event sequence, and no compatibility layer; under ADR 0038 it is part of the single first-scope
instruction contract rather than a versioned upgrade path. Fork, plan, and handoff records that must freeze context
carry the materialized projection; records that must not (historical runs, activity projections, audit records) carry
nothing new.

## Security and failure behavior

The instruction channel is the only trusted instruction surface, and it accepts content from declared sources only.
Repository content, tool output, fetched material, Skill bodies, memory cards, and provider output remain untrusted data
and cannot be promoted into it, so the ADR 0019 prompt-injection risk keeps its existing boundary and gains one explicit
rule: an instruction source cannot widen tool policy, provider selection, admission policy, or confirmation
requirements.

The closed instruction failure set is:

```text
instruction_profile_unavailable
instruction_source_unavailable
```

- `instruction_profile_unavailable`: a configured profile revision that is
missing, corrupt, inconsistent, or no longer resolvable.
- `instruction_source_unavailable`: a workspace instruction source that is
unreadable or not representable as text.

Each failure is a typed known pre-effect rejection that discloses no credential, absolute path, file content, raw
payload, or implementation detail. No fallback revision, partial projection, silent omission, or historical substitution
is permitted. Fake-secret regression covers every public, durable, log, and activity surface.

## Non-goals

Few-shot example selection, memory-derived prompt material, MCP-provided prompts, provider-native prompt framing, prompt
caching configuration, model-specific prompt templates, prompt tuning, prompt telemetry, A/B prompt evaluation,
automatic instruction generation or summarization, cross-project or shared instruction libraries, executable or scripted
instructions, sandbox or privilege claims, and UI design beyond the declared control-plane surface.
Date/time/Git/session-derived context injection and the remaining deferred items are recorded in the deferred and
excluded register with their reconsideration owners.

## Affected documents

[Architecture 30](../architecture/30-instruction-sources-and-system-context.md) owns the source model, profile
revisions, assembly, projection, bounds, failures, and observability. [Architecture
21](../architecture/21-goals-skills-context-memory-and-compaction.md) keeps Goals, Skills, context manifests, memory,
and compaction. [Architecture 23](../architecture/23-non-destructive-session-branching-and-regeneration.md) defines the
fork projection fields that carry this projection; [architecture 07](../architecture/07-plan-and-build-modes.md) owns
the `Mode` contribution and [architecture 06](../architecture/06-vfr-and-headroom.md) the `Vfr` contribution.
[Architecture 08](../architecture/08-model-protocol-and-providers.md) owns the request system-context semantics;
[architecture 09](../architecture/09-configuration-security-and-observability.md) configuration, classification, and
observability; [architecture 25](../architecture/25-configuration-provider-control-plane.md) the editing and preview
surface; and [architecture 04](../architecture/04-sessions-runs-events-and-storage.md) with [architecture
14](../architecture/14-run-execution-meaning-and-historical-compatibility.md) the persisted provenance and compatibility
rules.

## Evidence

Evidence: activating specification per [architecture 12](../architecture/12-quality-gates-and-makefile.md); it must
cover at least:

- deterministic assembly, ordering, separators, and identity stability across
repeated admissions;
- profile revision immutability for create, edit, duplicate, enable, disable,
reorder, and re-scope operations, with typed validation failures;
- limit precedence: any future numeric bound carries its recorded precedent,
and no text is truncated or sampled;
- an absent `AGENTS.md` contributing nothing; an unreadable or non-text one and
a missing, corrupt, or inconsistent profile failing closed with `instruction_source_unavailable` /
`instruction_profile_unavailable`, with no fallback or partial projection;
- projection freeze at admission, reuse across later steps, verbatim fork and
plan handoff inheritance, and proof that no current-configuration re-derivation occurs;
- proof that Skill bodies, memory records, tool output, repository content, and
provider output never enter the instruction channel;
- credential-free, path-free, raw-payload-free, and activity-free public,
durable, log, and audit surfaces, including fake-secret regression;
- the control-plane editing and preview contract (preview non-authority: a
preview creates no run, no reason, no selection, and no durable instruction state) and its TUI/REPL equivalence;
- M3/M4 and retained-history byte, meaning, replay, recovery, and tool-denial
preservation.

## Research provenance

- `legacy-antibusy-prompts/`: the read-only reference copy of the legacy
static session prompts (source revision `8604fde0566d4dfadf8124e0724c5a82db3f89de`, assembly site
`crates/tauri-app/src/state.rs`, `build_system_prompt()`), whose four static sources this decision adapts instead of
consuming unchanged; its size cap is not carried over. `legacy-baseline/04-agent-behavior.md` records the user-visible
legacy behavior of assembling a system prompt from project context, conventions, mode, and optional project files.
- [ADR 0017](0017-build-autopilot-and-plan-focus-continuity.md): the accepted
advisory Plan focus instruction, which becomes the `Mode` contribution. [ADR 0019](0019-production-model-tool-loop.md):
the recorded prompt-injection risk of the production model-tool loop, whose boundary this decision keeps.
- `m4plus_concept.md`: research provenance only. The roadmap declares it
immutable, so this direction is recorded from the legacy reference material and the user-requested direction of
2026-09-26 rather than by editing the concept.
