# 0001: Mandate Authority and Fresh-Run Lifecycle

## Status

Accepted.

## Decision

A Mandate is durable user-issued work authority, not a Goal, prompt, tool
permission, provider continuation, daemon, or second runtime. User commands own
product lifecycle and revision decisions. The daemon may only record explicitly
defined operational facts, including trigger capture, admission, known terminal
disposition, and required uncertainty pausing.

A continuation always admits a fresh run with a new `RunId`. Revision changes
affect only future admission. No provider request, tool call, process, kernel,
child work, MCP operation, or external effect resumes after restart.

## Invariants

- exactly one non-terminal Mandate run exists at a time;
- triggers are durable causal evidence, not legacy queued turns;
- Goals, Skills, parentage, activity, MCP source, model output, bridge grants,
  kernel state, and adapter state grant no lifecycle authority;
- intrinsic bounds, typed capacity availability, and forbidden product ceilings
  remain separate concepts.

## Compatibility and non-goals

M3/M4 runs retain recorded ordinary semantics. This record does not define
Mandate DTO fields, scheduler ordering, direct tool admission, WorkspaceRoot
behavior, child graphs, provider profiles, or implementation crates.

## Evidence

Future delivery requires lifecycle/race/fresh-run/recovery/typed-capacity
fixtures, no-synthetic-history fixtures, and outcome evidence through the
standard quality gate.

## Provenance

`m4plus_concept2.md`, selected semi-autonomous Mandate overlay and transition
linearization sections.
