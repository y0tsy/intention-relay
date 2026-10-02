# 0032: Post-M5 Accepted Deferred Directions: Activity-Tree Metadata, Semantic Content Inspection, and Per-Call Cancellation

## Status

Accepted 2026-08-30. Not implemented; activation requires an activating
specification.

## Decision

Three items named in the `m4plus_concept.md` backlog as "still to decide" or
"deliberately deferred" are adopted as accepted future directions:

- **Tree-level metadata** (concept2 line 7608), owned by architecture 24:
  a future bounded, credential-free metadata surface on activity trees,
  distinct from journal records and notification state, that never becomes
  activity, authority, or a second sequence;
- **Semantic content inspection** (concept2 line 7303), owned by
  architecture 22: a future explicit, code-owned content-inspection policy for
  reasoning and provider content that never substitutes for central
  redaction, never rewrites stored facts, and remains non-authorizing; and
- **Per-call cancellation and owner-specific semantics beyond the selected
  direct-pair communication** (concept2 line 7304), owned by architectures 19
  and 24: a future owner-specific cancellation contract beyond the first-scope
  `StopRunCommandDto`/direct-pair boundary.

Each direction keeps M3/M4 behavior authoritative, affects fresh runs only
after its own activating specification, and remains bound to Milestone 5+.

## Normative invariants

1. Tree-level metadata is bounded, credential-free, and non-authorizing; it
   cannot create activity, notification, lifecycle, scheduler, tool, child,
   verifier, MCP, bridge, kernel, or reconciliation authority, and it does not
   replace or filter the activity journal or notification sequences.
2. Semantic content inspection is explicit, code-owned, and non-authorizing;
   it never substitutes for central redaction, never rewrites stored facts,
   and never exposes raw provider content.
3. Per-call cancellation and owner-specific semantics are additive future
   contracts; the first-scope `StopRunCommandDto`/direct-pair boundary remains
   authoritative until a later activating specification defines them.

## Failure semantics

- Each direction fails closed before effect when its future contract is
  unsupported, unnegotiated, or over-limit; no partial projection or partial
  cancellation is delivered.
- Recovery never resumes, retries, reattaches, or reruns work under any of the
  three directions.

## Security

Tree-level metadata, content inspection, and per-call cancellation never
expose credentials, provider payloads, prompts, paths, grants, or raw
transcripts; redaction stays central and every activating specification must
pass the fake-secret regression suite. The directions remain non-authorizing.

## Rationale

The three items were named in `m4plus_concept.md` as deferred or
still-to-decide but appeared nowhere in the authoritative documentation,
including the retired deferred/excluded register. Adopting them records the
directions without documenting any feature as implemented.

## Compatibility and non-goals

This decision supersedes the absence of the three items in the authoritative
documentation and records them as accepted future directions rather than
unregistered deferrals. The closed M4 baseline, M3/M4 bytes, and existing
behavior remain unchanged, and no code changes are authorized by this
decision.

Native OS notifications, remote push, provider-side parser administration,
remote continuation, and production activation remain outside this decision.
M5-M9 are not renumbered.

Owner: architectures 19, 22, and 24. Evidence: activating specification per
[architecture 12](../architecture/12-quality-gates-and-makefile.md).
