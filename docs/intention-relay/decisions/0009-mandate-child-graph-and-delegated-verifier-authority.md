# 0009: Mandate Child Graph and Delegated Verifier Authority

## Status

Accepted.

## Decision

Future `sub_agent` work creates durable child Mandates connected by immutable direct edges and credential-free delegation snapshots. Parenthood grants only closed direct-child controls. It grants no inherited lifecycle, scheduler, tool, or verifier authority. Child outcomes are evidence and never implicitly mutate a parent.

Target mutation by a verifier requires separate user-issued, revisioned, target-scoped authority with exact allowed operations, immutable audit baseline, and qualifying evidence. A verdict alone grants nothing. User lifecycle, revision, reconciliation, revocation, and authority changes win conflicts.

## Invariants

- each child has one immutable parent and root graph, self-links, cycles, reparenting, detaching, merging, and cross-graph links fail before mutation;
- creation, controls, cascades, messages, and mutations are durable/idempotent, commit before publication, and reread before dispatch;
- child and verifier selections are immutable nested execution meaning, current ancestry, authority, configuration, registry, provider, or UI cannot rebuild missing meaning;
- child and verifier unknown effects remain local to their owning Mandate;
- recovery never resumes, retries, reattaches, rediscovers, or reapplies old child/verifier work or target mutations;
- a verifier target set never expands through graph, Goal, activity, session, branch, prompt, evidence, or relationship; and
- M3/M4 and retained RLM history remain unchanged and receive no synthetic future child or verifier state.

## Compatibility and non-goals

This is additive future architecture only. It preserves M3/M4 queue/replay, provider behavior, tool-call denial, recovery, and all historical bytes/meaning. It does not define an executor, worker topology, RLM/IPython, MCP, Skills/Goals, provider evolution, forks, UI, schema, migrations, crates, protocol implementation, or quality-policy activation.

## Primary owner and evidence

[Mandate Child Graph and Delegated Verifier Authority](../architecture/17-mandate-child-graph-and-delegated-verifier-authority.md) owns detailed behavior. A later implementation requires graph/authority canonical goldens, atomic fault injection, deterministic races, recovery/no-resume, historical compatibility, redaction, negotiated replay, and end-to-end outcomes through the standard quality gate.

## Provenance

`m4plus_concept.md`, selected Mandate child adaptation, delegated verification Mandates, and retained RLM graph/activity material.
