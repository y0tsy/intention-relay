# 0008: Durable Mandate Scheduler and Readiness-Driven Admission

## Status

Accepted.

## Decision

Future Mandate scheduling is one daemon-owned logical coordinator over durable
Mandate reasons. It reevaluates candidates using typed readiness/capacity
evidence and delegates all lifecycle mutation and fresh-run creation to the
architecture-13 admission transaction.

Readiness is operational evidence, not authority, immutable meaning, a
reservation, or a promise. Unavailability retains the reason without a `RunId`,
retry counter, product quota, or scheduler-owned claim. Candidate ordering stays
explicit-user-first, then `first_observed_at`, `MandateId`, and `ReasonId`.

## Invariants

- scheduler wakeups and task state are non-authoritative hints; durable reread
  is the correctness boundary;
- only the lifecycle-owned transaction can consume/hold a reason, create a RunId,
  bind meaning, and transition `Active -> Working`;
- user lifecycle/revision changes retain optimistic-conflict precedence;
- no scheduler/provider/tool/process/network effect occurs in a durable
  transition transaction;
- recovery completes before new scheduler admission and never resumes old work;
- current readiness/configuration/registry/provider state cannot reconstruct or
  alter immutable historical meaning.

## Compatibility and non-goals

M3/M4 queues, bytes, IDs, provider kinds, tool denial, replay, and recovery
remain unchanged. This decision does not adopt calendar/interval/time-zone/DST
semantics, worker topology, child/verifier, MCP, bridge/IPython, Skills/Goals,
provider evolution, UI, SQL, protocol implementation, crates, or runtime
activation.

## Primary owner and evidence

[Mandate scheduler and readiness-driven admission](../architecture/16-mandate-scheduler-and-readiness-driven-admission.md)
owns detailed rules. A later implementation requires deterministic ordering,
readiness/unavailability, conflict, fault-injection, recovery, compatibility,
redaction, protocol, and end-to-end fresh-admission fixtures through the standard
quality gate.

## Provenance

`m4plus_concept2.md`, Mandate trigger/recovery/limit sections and retained
continual-harness scheduling research.
