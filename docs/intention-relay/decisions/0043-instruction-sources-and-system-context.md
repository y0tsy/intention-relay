# ADR 0043: Instruction sources and system context

## Status

Accepted 2026-09-26 as a documentation-approved future direction within the
retrospective scope of
[ADR 0035](0035-m5plus-complete-foundation-activation.md). It extends
[ADR 0017](0017-build-autopilot-and-plan-focus-continuity.md) by giving the
Plan focus instruction one typed owner, respects the context boundary of
[ADR 0013](0013-goals-skills-context-memory-and-compaction.md) without changing
its Skill, memory, or projection rules, and amends ADR 0035 by adding a fifth
activating slice to the Milestone 5+ sequence. It activates nothing by itself:
no crate, DTO, tag, wire, storage schema, configuration field, UI page,
migration, feature profile, quality-policy target, or production behavior is
authorized here, and it is not the Slice 5 activating specification.

## Scope and supersession

This decision defines the one instruction channel of a model request: which
instruction sources exist, how they become an immutable effective instruction
projection, how that projection is bounded, materialized, and observed, and why
instruction text never carries authority. It creates
[architecture 30](../architecture/30-instruction-sources-and-system-context.md)
as the sole detailed owner and names the delivery home as the fifth activating
slice of Milestone 5+.

It supersedes nothing. It amends two recorded positions:

- the documentation-only exclusion in
  [architecture 21](../architecture/21-goals-skills-context-memory-and-compaction.md)
  that leaves prompt assembly undefined; instruction assembly is now owned by
  architecture 30 while architecture 21 keeps Goals, Skills, context manifests,
  memory, and compaction;
- the five-slice sequence of
  [ADR 0035](0035-m5plus-complete-foundation-activation.md), whose fifth slice
  this decision declares.

The M3/M4 baseline, the closed M4 charter, the current model contract, and the
recorded `system_context` channel keep their meaning. This decision answers a
concrete gap: the legacy Antibusy system assembled a static prompt set for every
session, the current project has no owner for that capability, and the
`effective_instruction_projection` fields of the fork records are defined
nowhere.

## Decision

The instruction channel is a closed set of typed instruction sources assembled,
once per admitted run, into one immutable effective instruction projection.

```text
InstructionSourceV1
  source_contract_revision
  source_kind
  source_scope
  source_identity
  declared_order
  enabled
  text_digest
  canonical_source_digest

InstructionProfileRevisionV1
  profile_contract_revision
  ordered_source_references
  profile_revision_identity
  canonical_profile_digest

InstructionProjectionV1
  projection_contract_revision
  ordered_contribution_references
  contribution_revision_and_digest
  declared_audience
  total_instruction_size
  canonical_projection_digest
```

- **Source kinds are closed**: `Identity`, `Guidelines`, `ToolUsage`,
  `CodingConventions`, `Custom`, `ProjectInstructions`, `Mode`, and `Vfr`. The
  first four mirror the legacy static set and are adapted, never consumed
  unchanged. `Custom` carries user-authored fragments. `ProjectInstructions`
  is the workspace `AGENTS.md` source. `Mode` and `Vfr` are reserved to their
  architecture owners.
- **Source scopes are closed**: `Deployment` (daemon-packaged defaults;
  enable/disable only), `User`, `Project`, and `Session` (durable configuration
  edited through the typed control-plane surface of architecture 25). Exactly
  one live configuration format version exists; there is no migration and no
  second format ([ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md)).
- **Workspace project instructions**: `AGENTS.md` at the session's
  `WorkspaceRoot` is the only file-based instruction source. It is resolved
  inside the workspace boundary, read as text, addressed by digest, never
  executed, and never treated as authority. An absent file contributes
  nothing and is not a failure.
- **Fragment profile**: all configured fragments form an immutable
  `InstructionProfileRevisionV1`. Creating, editing, reordering, enabling,
  disabling, or re-scoping a fragment produces a new profile revision identity;
  an existing revision is never mutated.
- **Canonical assembly order** is fixed: `Deployment` fragments in declared
  order, then `User`, `Project`, and `Session` fragments in declared order,
  then `ProjectInstructions`, then `Mode`, then `Vfr`. Order is part of the
  canonical bytes and the digest.
- **The effective instruction projection is frozen at admission**, before the
  first model step of a run. Every later model step of that run reuses it; a
  fork inherits the materialized projection verbatim; a plan handoff
  materializes it into its frozen snapshot; no run, fork, regeneration, or
  replay re-derives instructions from current configuration, current
  `AGENTS.md` content, or current session state.
- **Delivery into the request uses the existing `system_context` channel**: the
  daemon passes the assembled projection as the request's optional system
  context, and the current drivers keep translating it into the leading system
  message with no driver-specific framing, rewrite, caching directive, or
  provider-side scan.
- **Materialization**: the projection is the contents of the
  `effective_instruction_projection` and
  `materialized_effective_instruction_projection` fields of the fork records
  ([architecture 23](../architecture/23-non-destructive-session-branching-and-regeneration.md)),
  and its canonical digest is recorded as safe usage provenance. It adds no
  event sequence and no authority.
- **Required editing surface**: the control plane lists, creates, edits,
  duplicates, enables, disables, reorders, and re-scopes fragments, rejects
  invalid edits with typed errors, shows the profile revision identity and
  digest, and previews the effective instruction projection for a chosen
  session, policy, and mode without admitting a run. The primary UI exposes the
  surface; TUI/REPL remains contract-equivalent.
- **Intrinsic bounds** are total projection at most 100,000 characters, one
  fragment at most 16,384 characters, at most 64 enabled fragments, and
  project instructions at most 16,384 characters. The Slice 5 activating
  specification aligns the total bound with the existing `system_context`
  validation. An unrepresentable, oversized, inconsistent, unreadable, or
  boundary-violating source fails closed before admission; text is never
  truncated, sampled, or silently replaced by a previous revision.
- **Observability**: logs, activity, notification, and audit surfaces carry
  revision identities and digests only. Instruction text stays on the
  configuration surface where the user edits it.

## Invariants

1. No authority. Instruction text grants no tool, policy, Mandate, child,
   verifier, MCP, bridge, kernel, provider, scheduler, reconciliation, or
   confirmation authority, and it cannot prevent or prove the absence of any
   external effect. The advisory rule of ADR 0017 is unchanged.
2. One channel. Exactly one effective instruction projection exists per
   admitted run, it has exactly one owner, and no second prompt-assembly path,
   hidden system text, or provider-supplied instruction may be added beside it.
3. Channel closure. Skill bodies, memory records, tool output, repository
   content, fetched material, provider output, and model output never enter the
   instruction channel; they remain context-manifest, Skill-disclosure, or
   evidence material under architecture 21.
4. Immutability. Profile revisions and materialized projections are immutable;
   a change creates a new revision; forks and historical records are never
   rewritten or reconstructed.
5. Determinism. The same profile revision, project instruction digest, mode,
   and configuration produce byte-identical canonical projection bytes and the
   same digest; order, separators, and canonicalization are fixed.
6. Boundedness. The projection is bounded, credential-free, and safe to record;
   no instruction source may carry a secret, a credential, raw provider or tool
   payload, an implementation resource, or executable content.
7. Fail closed. A missing, unreadable, inconsistent, boundary-violating, or
   oversized configured source blocks admission with a closed typed failure; no
   fallback, partial assembly, or silent omission is permitted.
8. Fresh runs only. Instructions affect newly admitted runs. M3/M4 runs,
   retained history, and recorded bytes gain no instruction state.

## Compatibility

Historical M3/M4 requests keep the optional `system_context` absent, and
existing runs, events, snapshots, replay, recovery, and tool denial keep their
recorded meaning. The mechanism adds no second model protocol, no second
storage schema, no new event sequence, and no compatibility layer; under ADR
0038 it is part of the single first-scope instruction contract rather than a
versioned upgrade path. Fork, plan, and handoff records that must freeze context
carry the materialized projection; records that must not (historical runs,
activity projections, audit records) carry nothing new.

## Security and failure behavior

The instruction channel is the only trusted instruction surface, and it accepts
content from declared sources only. Repository content, tool output, fetched
material, Skill bodies, memory cards, and provider output remain untrusted data
and cannot be promoted into it, so the ADR 0019 prompt-injection risk keeps its
existing boundary and gains one explicit rule: an instruction source cannot
widen tool policy, provider selection, admission policy, or confirmation
requirements.

The closed instruction failure set is:

```text
instruction_profile_unavailable
instruction_projection_too_large
instruction_source_unavailable
```

- `instruction_profile_unavailable` covers a configured profile revision that
  is missing, corrupt, inconsistent, or no longer resolvable.
- `instruction_projection_too_large` covers a projection that exceeds an
  intrinsic bound.
- `instruction_source_unavailable` covers a workspace instruction source that
  is unreadable, escapes the workspace boundary, is not representable as text,
  or exceeds its own bound.

Each failure is a typed known pre-effect rejection that discloses no
credential, absolute path, file content, raw payload, or implementation detail.
No fallback revision, partial projection, silent omission, or historical
substitution is permitted. Fake-secret regression covers every public, durable,
log, and activity surface.

## Non-goals

Few-shot example selection, memory-derived prompt material, MCP-provided
prompts, date/time/Git/session-derived context injection, provider-native prompt
framing, prompt caching configuration, model-specific prompt templates, prompt
tuning, prompt telemetry, A/B prompt evaluation, automatic instruction
generation or summarization, cross-project or shared instruction libraries,
executable or scripted instructions, sandbox or privilege claims, UI design
beyond the declared control-plane surface, and any implementation authorization
are outside this decision. Deferred items are recorded in the deferred and
excluded register with their reconsideration owners.

## Affected documents

- [Architecture 30](../architecture/30-instruction-sources-and-system-context.md)
  is the sole detailed owner of the source model, profile revisions, assembly,
  projection, bounds, failures, and observability.
- [Architecture 21](../architecture/21-goals-skills-context-memory-and-compaction.md)
  keeps Goals, Skills, context manifests, memory, and compaction, and records
  that instruction assembly is owned by architecture 30.
- [Architecture 23](../architecture/23-non-destructive-session-branching-and-regeneration.md)
  defines the fork projection fields that carry this projection.
- [Architecture 07](../architecture/07-plan-and-build-modes.md) owns the `Mode`
  contribution; [architecture 06](../architecture/06-vfr-and-headroom.md) owns
  the `Vfr` contribution.
- [Architecture 08](../architecture/08-model-protocol-and-providers.md) owns the
  request system-context semantics; [architecture 09](../architecture/09-configuration-security-and-observability.md)
  owns configuration, classification, and observability.
- [Architecture 25](../architecture/25-configuration-provider-control-plane.md)
  owns the editing and preview surface.
- [Architecture 04](../architecture/04-sessions-runs-events-and-storage.md) and
  [architecture 14](../architecture/14-run-execution-meaning-and-historical-compatibility.md)
  own the persisted provenance and compatibility rules.
- [Architecture 00](../architecture/00-principles-and-scope.md),
  [architecture 11](../architecture/11-implementation-roadmap.md), the
  [architecture README](../architecture/README.md), and the reconciliation
  registers carry the principle, slice, ownership, topic, and evidence rows.

## Evidence

The Slice 5 activating specification must declare exact crate owners, test
targets, coverage tiers, feature profiles, storage/wire versions, and fixtures,
then pass `make quick`, `make verify`, docs-check, and Linux/Windows CI. It must
cover at least:

- deterministic canonical assembly, ordering, separators, and digest stability
  across repeated admissions;
- profile revision immutability for create, edit, duplicate, enable, disable,
  reorder, and re-scope operations, with typed validation failures;
- intrinsic-bound rejection for the total projection, a single fragment, the
  fragment count, and workspace instructions, with no truncation or sampling;
- an absent `AGENTS.md` contributing nothing, and an unreadable,
  boundary-escaping, non-text, or oversized one failing closed with
  `instruction_source_unavailable`;
- fail-closed behavior for `instruction_profile_unavailable` and
  `instruction_projection_too_large`, with no fallback or partial projection;
- projection freeze at admission, reuse across later steps, verbatim fork and
  plan handoff inheritance, and proof that no current-configuration
  re-derivation occurs;
- proof that Skill bodies, memory records, tool output, repository content, and
  provider output never enter the instruction channel;
- credential-free, path-free, raw-payload-free, and activity-free public,
  durable, log, and audit surfaces, including fake-secret regression;
- preview non-authority: a preview creates no run, no reason, no selection, and
  no durable instruction state;
- the control-plane editing and preview contract and its TUI/REPL equivalence;
- M3/M4 and retained-history byte, meaning, replay, recovery, and tool-denial
  preservation.

## Research provenance

- [`legacy-antibusy-prompts/`](../legacy-antibusy-prompts/README.md): the
  read-only reference copy of the legacy static session prompts (source
  revision `8604fde0566d4dfadf8124e0724c5a82db3f89de`, assembly site
  `crates/tauri-app/src/state.rs`, `build_system_prompt()`), whose four
  static sources and 100,000-character cap this decision adapts for Intention
  Relay instead of consuming unchanged.
- [`legacy-baseline/02-capability-catalog.md`](../legacy-baseline/02-capability-catalog.md)
  and [`legacy-baseline/04-agent-behavior.md`](../legacy-baseline/04-agent-behavior.md):
  the recorded user-visible legacy behavior of assembling a system prompt from
  project context, conventions, mode, and optional project files.
- [ADR 0017](0017-build-autopilot-and-plan-focus-continuity.md): the accepted
  advisory Plan focus instruction, which becomes the `Mode` contribution.
- [ADR 0019](0019-production-model-tool-loop.md): the recorded prompt-injection
  risk of the production model-tool loop, whose boundary this decision keeps.
- [`m4plus_concept.md`](../m4plus_concept.md): research provenance only. The
  roadmap declares it immutable, so this direction is recorded from the legacy
  reference material and the user-requested direction of 2026-09-26 rather than
  by editing the concept.
