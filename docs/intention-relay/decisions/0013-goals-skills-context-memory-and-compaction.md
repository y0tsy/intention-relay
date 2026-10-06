# 0013: Goals, Skills, Context, Memory, and Compaction

## Status

Accepted.

## Decision

Future Goals are user-managed, immutable revisioned acceptance/evidence records with project or session scope.
Project-scoped Goals apply to a session only through an explicit immutable applicability link. Goals are
non-authorizing.

Future Skills are immutable, versioned, untrusted instructional content with explicit progressive disclosure. Admission
freezes a source manifest, and each affected model step binds a safe context projection whose provider request is a
mutable window under [architecture 08](../architecture/08-model-protocol-and-providers.md) ([ADR
0054](../decisions/0054-dynamic-context-window-and-prompt-caching.md)); the manifest, the selections, and every durable
fact stay recorded as written. Memory uses
immutable typed records, safe cards, explicit disclosure, and explicit replacement/rollback relations. Compaction is an
immutable safe summary over exact completed durable history and retains its source provenance and uncompacted suffix.

## Invariants

- Goals, Skills, context, memory, cards, disclosures, and summaries cannot
create or widen tool, MCP, bridge, kernel, provider, or reconciliation authority;
- a current file, catalog, index, memory, Skill, Goal, configuration, UI, or
runtime state cannot reconstruct a missing admitted selection or model-step projection;
- safe cards and projections are never broader than source content or its
authorized audience;
- original durable facts remain authoritative and compaction cannot summarize
incomplete/unknown work or become continuation state; and
- recovery validates persisted supported references only and never rediscloses,
recompacts, resumes, retries, or performs external work.

## Compatibility and non-goals

M3/M4 bytes and ordinary behavior remain unchanged; historical records gain no synthetic context-related state, and M4
tool calls remain denial evidence. Provider evolution, session forks, activity/UI, physical Plan artifacts, direct MCP
administration, kernel process behavior, resource values, and concrete storage/wire implementation remain separate.

Owner: architecture 21. Evidence: activating specification per [architecture
12](../architecture/12-quality-gates-and-makefile.md).

Provenance: `m4plus_concept.md`, selected Goal, Skill, memory, and compaction material, reconciled against architectures
14, 15, and 18--20.
