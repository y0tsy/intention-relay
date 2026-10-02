# 0013: Goals, Skills, Context, Memory, and Compaction

## Status

Accepted.

## Decision

Future Goals are user-managed, immutable revisioned acceptance/evidence records
with project or session scope. Project-scoped Goals apply to a session only
through an explicit immutable applicability link. Goals are non-authorizing.

Future Skills are immutable, versioned, untrusted instructional content with
explicit progressive disclosure. Admission freezes a source manifest and each
affected model step binds an immutable safe context projection. Memory uses
immutable typed records, safe cards, explicit disclosure, and explicit
replacement/rollback relations. Compaction is an immutable safe summary over
exact completed durable history and retains its source provenance and
uncompacted suffix.

## Invariants

- Goals, Skills, context, memory, cards, disclosures, and summaries cannot create
  or widen Mandate, scheduler, tool, child, verifier, MCP, bridge, kernel,
  provider, or reconciliation authority;
- a current file, catalog, index, memory, Skill, Goal, configuration, UI, or
  runtime state cannot reconstruct a missing admitted selection or model-step
  projection;
- safe cards and projections are never broader than source content or its
  authorized audience;
- original durable facts remain authoritative and compaction cannot summarize
  incomplete/unknown work or become continuation state; and
- recovery validates persisted supported references only and never rediscloses,
  recompacts, resumes, retries, or performs external work.

## Compatibility and non-goals

M3/M4 bytes and ordinary behavior remain unchanged. Historical records gain no
synthetic context-related state, and M4 tool calls remain denial evidence. This
decision activates no crate, retrieval/index/search engine, prompt builder,
storage schema, migration, protocol, retention/deletion/encryption policy,
quality policy, or production behavior.

Provider evolution, session forks, activity/UI, physical Plan artifacts, direct
MCP administration, kernel process behavior, resource values, and concrete
storage/wire implementation remain separate.

## Primary owner and evidence

[Goals, Skills, context, memory, and compaction](../architecture/21-goals-skills-context-memory-and-compaction.md)
owns detailed behavior. A later implementation requires canonical, authority,
projection, memory, compaction, fault/recovery, protocol, compatibility,
redaction, cross-platform, and outcome evidence through the standard quality
gate.

## Provenance

`m4plus_concept.md`, selected Goal, Skill, memory, and compaction material,
reconciled against architectures 13--20.
