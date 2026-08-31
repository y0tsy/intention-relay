# Goals, Skills, Context, Memory, and Compaction

## Status and scope

## Traceability

- Normative owner: architecture 21.
- Decision record: [`0013`](../decisions/0013-goals-skills-context-memory-and-compaction.md).
- Reconciliation topics: `GOL/SKL/CTX/MEM/CMP`.
- Research provenance: [`m4plus_concept.md`](../m4plus_concept.md).
- Status: documentation-approved; implementation-authorized work requires a later activating specification.


**Approved future architecture, documentation-only.** This document is the sole
detailed owner for future Goal scope and evidence, Skill selection and safe
disclosure, context-source manifests, model-step projections, typed memory
records, and immutable compaction. It does not authorize a crate, retrieval
engine, vector index, prompt builder, storage migration, wire implementation,
retention/deletion/encryption policy, or production context behavior.

It applies only to future Mandate and VerifierMandate execution. M3/M4 bytes,
IDs, UUIDs, digests, cursors, events, snapshots, queue tickets, provider
behavior, replay, recovery, and M4 `ToolCallRecorded -> tool_execution_unavailable`
retain their recorded ordinary semantics. Retained Goal, Skill, memory, or
compaction material remains research provenance and historical-only where it
conflicts with architectures 13--20.

## Ownership and non-authorities

Architecture 13 owns Mandate lifecycle, fresh admission, uncertainty, and exact
reconciliation. Architecture 14 owns execution-envelope framing, canonical
encoding, digest validation, decoding, and compatibility classes. Architecture
15 owns the registry, frozen direct-tool selection, tool admission, model-tool
loop, `ToolCallId`, and generic effect facts. Architecture 16 owns scheduler
reevaluation and readiness. Architecture 17 owns child graph and verifier
authority. Architecture 18 owns MCP lifecycle. Architecture 19 owns bridge
grants, ingress, operation correlation, and bridge delivery. Architecture 20
owns kernel epochs, cells, checkpoints, and kernel-local safe projection.

This document owns only future non-authorizing context/evidence selection and
safe representation. A Goal, Skill, source manifest, projection, memory record,
card, disclosure, summary, omission, or compaction never creates or mutates a
`RunId`, Mandate reason, lifecycle state, scheduler eligibility, tool permission,
registry slot, `WorkspaceRoot`, child edge, verifier authority, MCP capability,
bridge grant, kernel epoch, provider route, or reconciliation decision. Context
is not a hidden prompt-mutation channel, sandbox, policy engine, or continuation
mechanism.

## Immutable selection and Goal scope

Architecture 14 owns canonical framing. This document owns detailed semantics of
these credential-free nested future Mandate-meaning fields:

| Architecture 14 field | Selection owner | Meaning |
| ---: | --- | --- |
| 5 | Context projection selection | Admission source-manifest and projection contract. |
| 7 | Goal context selection | Selected scoped Goal revisions and applicability links. |
| 14 | Skill selection | Selected immutable Skill revisions and disclosure contract. |

Each optional selection remains exactly `Disabled` or `Selected`. Unknown,
corrupt, unsupported, mismatched, or unavailable required selection blocks
dependent future work before a model, provider, tool, process, bridge, kernel,
child, or MCP effect. No current file, catalog, index, Goal, Skill, memory,
configuration, UI, ancestry, model, provider, bridge, kernel, or MCP state may
repair missing meaning.

A Goal is a user-managed, immutable revisioned acceptance/evidence record. It is
not an instruction channel or execution authority. A Goal has exactly one scope:
`Project` or `Session`. A project-scoped Goal applies to a session only through
an explicit immutable project-Goal-to-session applicability link selected at
admission. There is no implicit project-wide inheritance. A session-scoped Goal
does not silently become project-scoped.

```text
GoalContextSelectionV1
  selection_contract_revision
  ordered_goal_references
  ordered_applicability_link_references
  safe_acceptance_projection_revision
  canonical_goal_selection_digest
```

A selected reference freezes typed identity, revision, scope, canonical digest,
and safe acceptance/evidence projection. A later edit creates a new Goal
revision; it cannot rewrite admitted or model-step context. Goal evidence may be
referenced by a verifier baseline only through architecture 17's separately
issued authority. A Goal cannot create a verifier, target, verdict, target
mutation, child, or lifecycle transition.

## Skills and progressive disclosure

A Skill is immutable, versioned, untrusted instructional content with typed
provenance, declared audience, selection digest, and safe disclosure policy. A
Skill cannot contain executable authority, hidden tool permissions, registry
mutation, provider credential, bridge/kernel/MCP resource, or implicit
child/verifier/scheduler/lifecycle power.

```text
SkillSelectionV1
  skill_contract_revision
  ordered_skill_references
  disclosure_policy_revision
  safe_projection_revision
  canonical_skill_selection_digest
```

A model step receives only its explicitly selected bounded safe Skill projection.
Full source remains private unless the selection's disclosure policy expressly
permits its safe representation. Omitted, incompatible, or unsafe material is
recorded as a typed omission/degradation outcome; it is never silently replaced
with current catalog content or hidden injected text.

## Factory-aligned Skill model

This section adopts the Factory Droid Skill interaction model as the detailed
Skill shape: a reusable workflow has a mandatory `SKILL.md`-equivalent entry
record, validated frontmatter, an instructional body, and optional supporting
resources. Its required `name` and `description` are the routing surface: the
description states the action, trigger, and boundary by which a model or user
can select the Skill. A Skill is distinct from always-on instructions, a user
command, a delegated agent, a hook, an MCP server, and a registered tool.

```text
SkillDto
  skill_id
  canonical_name
  origin
  owner_scope
  lifecycle_state
  active_revision
  safe_card
  provenance
  canonical_skill_digest

SkillRevisionDto
  skill_id
  revision
  frontmatter
  textual_body_reference
  supplemental_resource_references
  typed_reference_set
  replacement_reference_when_present
  rollback_reference_when_present
  canonical_revision_digest

SkillFrontmatterV1
  name
  description
  allowed_tool_ids
  enabled
  user_invocable
  model_invocable
  license_when_present
  compatibility_when_present
  validated_metadata
  declared_version_when_present

SkillCardDto
  skill_id
  canonical_name
  origin
  owner_scope
  lifecycle_state
  exact_revision
  description
  safe_purpose
  applicability
  model_invocable
  user_invocable
  allowed_tool_ids
  content_reference
  canonical_card_digest

SkillSupplementDto
  supplement_id
  skill_revision_reference
  logical_name
  kind = Checklist | Schema | ReferenceText
  retained_content_reference
  canonical_supplement_digest
```

`name` is a canonical lowercase letters/numbers/hyphens identifier and
`description` is bounded safe routing text. `allowed_tool_ids` is declarative
review/presentation metadata only: it neither grants unavailable tools nor
sandboxes an invocation. `enabled = false` disables all invocation;
`user_invocable = false` suppresses the slash-like user command only; and
`model_invocable = false` suppresses model matching only. Validated optional
license, compatibility, version, and metadata values carry no credential,
authority, executable, or configuration semantics. Unknown or untyped
frontmatter cannot change behavior.

Bodies and supplements are bounded immutable UTF-8 text or typed structured
reference content only. They contain no binary/image/rich-MIME payload,
executable code, shell template, Python package, install instruction, plug-in
runtime, raw JSON schema, arbitrary URL/host/header, endpoint, credential,
provider/SDK value, process/kernel/socket handle, hidden external effect, or
dynamic registration. The only typed references are immutable memory, role,
gate-template, registered descriptor revision, retained content, and an exact
Mandate MCP capability revision that is already present in the same frozen
run-local MCP selection.

```text
SkillOriginDto
  Session
  Goal
  Project
  Personal
  ProjectPlugin
  UserPlugin
  Builtin

SkillLifecycleStateDto
  Enabled
  Disabled
  Archived
  Revoked
  Overridden
  Invalid
```

The first three origins retain their selected durable IR scope. `Personal`,
`ProjectPlugin`, `UserPlugin`, and `Builtin` preserve Factory distribution and
resolution semantics as daemon-owned provenance, not ambient filesystem
authority: no recursive path scan, symlink traversal, remote fetch, marketplace
activity, executable package, MCP/server activation, or installation occurs at
runtime. A user may explicitly import a reviewed Factory-compatible textual
bundle; the daemon validates it and creates an immutable Skill revision with
source identity/revision/digest, importer, and time. An import never installs or
activates another plugin component.

For one canonical name, effective resolution is `Session`, then nearest selected
Goal, then `Project`, `ProjectPlugin`, `Personal`, `UserPlugin`, and `Builtin`.
Same-name records across origins are not merged: the winner and every overridden
loser remain safely queryable with identity, revision, origin, digest, and
resolution reason. A duplicate canonical name in one origin is invalid. A
disabled effective record prevents invocation. Explicit replacement is still
required to replace a record's semantic content; precedence selects one exposed
name and never invents a text conflict resolution.

Discovery, selection, and invocation use progressive disclosure: the model
initially receives only applicable enabled model-invocable cards, ordered
deterministically and pinned to one catalog revision. It compares the request
to description and applicability, then records a selected exact revision before
the full body is disclosed. A user may invoke `/skill:<name> <arguments>`
through an equivalent typed command when the effective Skill is enabled and
user-invocable; the arguments are bounded user text attached to the procedure,
not authority to widen its scope or capabilities. User and model invocation
have identical freeze, disclosure, audit, redaction, and no-authority semantics
and differ only in invocation origin. A custom user command of the same name
remains its own command; it does not alter model Skill matching.

Supporting resources are never implicitly loaded. The body may name a
`SkillSupplementDto`, but only an explicit `retrieve` against that exact frozen
reference discloses its bounded content. A Skill invocation is instructional
context and selected references only. Every actual action still travels through
the selected Rust-owned descriptor/gateway path. A card, body, supplement,
frontmatter, declared tools, origin, role link, Goal, child relation, verifier,
IPython facade, MCP server, or model request cannot create a ToolId, descriptor,
registry entry, credential access, provider selection, authorization,
confirmation, policy/corridor/quota, lifecycle mutation, scheduler trigger,
child authority, gate success, or external effect.

Skill text is untrusted model-visible data, not policy or executable authority.
It may recommend already available work and may narrow a task, class,
context/result bound, or selected descriptor subset through an exact typed role
or delegation reference. It cannot widen any of them. A Skill reference to MCP
cannot acquire, discover, refresh, install, register, or invoke an unselected
capability. Python/IPython may render a selected Skill as convenience
documentation only; it cannot import a Skill package, execute a body, or bypass
the typed Skill disclosure and tool gateway paths.

```text
SkillSelectionV1
  skill_id
  exact_revision
  origin
  owner_scope_provenance
  card_digest
  body_reference_when_disclosed
  disclosed_supplement_references
  typed_reference_set_digest
  invocation_origin = UserSlash | ModelMatch
  canonical_selection_digest
```

Selection is immutable before the next affected model step or external action.
`GoalRunSelectionV1` records exact Skill selections; ordinary runs use a
separately versioned optional selection; Mandate and VerifierMandate execution
meaning carries the same non-authorizing frozen Skill context. A child Mandate
receives only exact selected Skill/role/reference values explicitly placed in
its delegation snapshot. A verifier Skill never derives authority. Forks retain
the exact selected card/revision/disclosure references in their immutable base
snapshot. Retry, replay, queue promotion, and recovery validate stored selection
and never rediscover a current Skill. A new revision, disable, archive, revoke,
replacement, or restore affects future discovery/admission only; it never
rewrites an admitted run, child, fork, verifier, historical selection, or body.

The daemon owns one append-only Skill sequence separate from session/run
cursors. `SkillCreated`, `SkillImported`, `SkillRevisionCreated`,
`SkillReplacementLinked`, `SkillRolledBack`, `SkillArchived`, `SkillRestored`,
`SkillRevoked`, `SkillResolutionRecorded`, `SkillSelected`, `SkillDisclosed`,
`SkillSupplementDisclosed`, `SkillProposalAccepted`, and
`SkillProposalRejected` are typed facts. Every state-changing command atomically
commits its projection, event(s), canonical digest, idempotency binding, and
affected snapshot(s), then publishes only after durable reread. Current cards
and catalog snapshots accelerate queries but never reconstruct historical use.

`skills_v1` is an additive negotiated capability for card-only `ListSkills`,
exact inspect/disclosure, user invocation, lifecycle commands, and durable
skill audit. A list captures one catalog revision, uses stable ordering and an
opaque token, and returns `has_more`; a malformed, cross-scope, or stale token
fails typed conflict/resynchronization. An older client keeps existing
session/run behavior and receives `skill_capability_required` for a Skill
operation rather than a partially understood frame. Canonical Skill records use
new fixed tags/field tables in the selected typed-TLV/SHA-256 family. Unknown,
corrupt, incompatible, stale, missing, archived, revoked, or over-limit content
blocks only the dependent disclosure/model step before external work while
unrelated history remains readable; no current card, body, origin, or live path
is substituted.

At minimum, closed failures are `skill_invalid`, `skill_disabled`,
`skill_overridden`, `skill_not_user_invocable`, `skill_not_model_invocable`,
`skill_scope_unavailable`, `skill_revision_stale`,
`skill_reference_unavailable`, `skill_content_unavailable`,
`skill_supplement_unavailable`, `skill_catalog_token_invalid`,
`skill_catalog_conflict`, `skill_name_collision`, `skill_frontmatter_invalid`,
`skill_content_too_large`, `skill_forbidden_content`,
`skill_capability_reference_unavailable`, `skill_selection_incompatible`,
`skill_capability_required`, and `skill_revision_conflict`. They reveal no full
body, secret, private host, path, raw reference body, implementation resource,
or external state.

## Context sources, projections, and memory

Same-Session Plan-to-Build continuation may retain the conversation as model
context, but it must bind the approved plan revision and create a fresh Build
run. Conversation history is context evidence, not proof of current mutable
state or completion of an earlier effect. Before mutation, the Build run must
revalidate mutable project and external state. An optional new-Session handoff
materializes the full available safe conversation context, approved plan, and
execution prompt into an immutable credential-free snapshot; it never transfers
live runtime resources, credentials, grants, queues, or unfinished effects.

Admission freezes an exact typed manifest before a dependent model step begins:

```text
ContextSourceManifestV1
  manifest_contract_revision
  ordered_source_references
  source_revision_and_digest
  declared_audience
  representation_policy_revision
  omission_references
  canonical_manifest_digest

ModelContextProjectionV1
  projection_contract_revision
  manifest_reference
  model_step_id
  ordered_safe_items
  disclosure_decisions
  omission_or_degradation_evidence
  canonical_projection_digest
```

The manifest binds identities, revisions, digests, audience, safe
representation policy, declared semantic order, and typed omission reasons. A
projection binds one model step to an ordered safe representation of that exact
manifest. It is immutable before the step begins. A source or card may be
narrower than its original content but can never be broader or visible to a
wider audience.

Memory uses immutable typed records, not mutable hidden prompt state. A memory
record has provenance, scope, safe card, audience, explicit disclosure status,
and supersession/replacement/rollback references. Retrieval and disclosure may
refuse safely or omit a record with typed reason. They cannot infer conflicts,
mutate historical context, inject undisclosed content, or leak source material.
A replacement creates a new immutable record and explicit relation; rollback
selects an earlier record explicitly and does not erase either record.

## Compaction, cancellation, and recovery

Compaction creates an immutable safe summary over an exact ordered set of
**completed** durable history. It retains source ranges/digests, compaction
policy/version, summary digest, safe audience, omission evidence, and exact
uncompacted suffix. Original durable facts remain authoritative and are never
deleted, replaced, or reinterpreted by a summary. A compaction cannot cover
unfinished or unknown effects, synthesize completion, reorder history, create
authority, or become continuation/recovery state.

```mermaid
sequenceDiagram
  participant U as User
  participant G as Goals
  participant C as Context
  participant M as Meaning
  participant S as Model step
  participant F as Durable facts

  U->>G: Create or revise Goal
  G->>C: Select explicit references
  C->>M: Freeze manifest and selections
  M->>S: Bind immutable projection
  S->>F: Commit completed facts
  F->>C: Provide exact completed range
  C->>C: Create immutable safe summary
```

Cancellation, terminalization, selection replacement, audience revocation, or
restart blocks later disclosure and model steps as applicable but cannot rewrite
an already committed projection or summary. Late source, card, retrieval,
disclosure, or compaction output after cancellation, terminalization, replacement,
or restart is non-authoritative and cannot append facts or repair uncertainty.

Recovery completes before new context-driven admission or step construction. It
may validate persisted supported references, but never rediscloses, recompacts,
fetches current catalog/file/index content, resumes/retries work, or reconstructs
projections from mutable state. Unsupported, corrupt, or missing selected context
blocks only dependent future work before effect; unrelated readable history stays
isolated.

## Child, verifier, MCP, bridge, kernel, protocol, and compatibility boundaries

A child receives only separately selected frozen safe references. It never
inherits a live manifest, undisclosed memory, Goal applicability, Skill source,
bridge grant, kernel namespace/checkpoint, provider continuation, MCP selection,
connection, process, or unfinished effect. Child context is independent and
non-authorizing. A verifier receives only architecture-17-authorized safe
baseline/evidence references; context cannot widen verifier authority or mutate
a target.

MCP, bridge, and kernel context is safe projection only. Context cannot discover
or invoke MCP, issue a bridge grant/operation, create a kernel epoch, restore a
checkpoint, or cause a host request. Future delivery is separately negotiated,
correlated, history-before-live, read-only safe projection or typed resync/error.
Replay, reconnect, or audit cannot create a Goal, disclose memory, recompact,
execute a model step, invoke a tool, start a child, issue authority, or perform
external work. Unnegotiated peers fail closed.

M3/M4 and retained records gain no Goal, Skill, source-manifest, projection,
memory, disclosure, summary, applicability link, Mandate, child, verifier, MCP,
activity, policy, or execution-kind state. Historical M4 tool calls remain denial
evidence. No current mutable state may reconstruct missing future context
meaning.

## Dependencies, non-goals, and evidence

This document depends on architectures 13--20 and decisions 0001--0012. It does
not define actual Goal persistence, search/index/vector retrieval, prompt
assembly, SQL/wire tags, migrations, retention/deletion/encryption, source-page
sizes, resource values, provider evolution, architecture-23 session branching, activity/UI, physical
Plan artifacts, direct MCP administration, Python/Jupyter process behavior,
Cargo, Makefile/CI, or production activation.

A later activating specification must declare exact crate owners, test targets,
coverage tiers, feature profiles, storage/wire versions, retention/deletion/
encryption policy, and intrinsic versus capacity bounds. It must pass `make
quick`, `make verify`, and Linux/Windows CI, and cover:

- canonical and negative vectors for Goal scope/applicability, Skills, manifests,
  projections, memory cards/disclosures/supersession, and summary provenance;
- admission/model-step fault injection proving no dependent external effect
  occurs before immutable selection and projection binding;
- project-to-session link validation, session isolation, no implicit inheritance,
  revision immutability, and Goal/verifier non-authority;
- untrusted Skill/progressive-disclosure cases with no hidden authority or
  sensitive-content leakage;
- no-current-state-reconstruction across Goals, Skills, files, indexes, memory,
  configuration, UI, ancestry, provider, bridge, kernel, and MCP state;
- typed memory refusal/disclosure/replacement/rollback without audience widening
  or hidden prompt mutation;
- compaction order/provenance/suffix/incomplete-history/corruption cases that
  preserve original facts and exclude continuation authority;
- recovery, cancellation, replay/resync, child/verifier/MCP/bridge/kernel
  isolation, late-data rejection, and zero-effect reconnect outcomes;
- M3/M4 and retained-history byte/meaning/startup/replay/recovery/tool-denial
  preservation; and
- fake-secret, raw source, memory payload, corrupt bytes, private references,
  paths, handles, provider material, and unsafe output absence from all public or
  durable surfaces.

Architecture 22 owns provider kind/profile/capability/reasoning normalization.
Provider may declare compatible reasoning input requirements, but only this
document selects sources, audiences, disclosures, and model-step context
projections; provider cannot scan or inject historical context.

Architecture 24 owns activity/UI projections. Context records may be referenced
only through safe selected provenance and cannot become activity authority or
a hidden presentation disclosure channel.
