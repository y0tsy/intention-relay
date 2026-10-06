# Limits by precedent

**Status:** current policy, recorded by [ADR 0014](decisions/0014-limits-by-precedent-and-no-content-scanning.md) and
amended by [ADR 0014](decisions/0014-limits-by-precedent-and-no-content-scanning.md). This document replaces the
earlier product-ceiling audit scope. The PR #15 ceiling removal and the M5+
Slice 1 cleanup are historical inputs, not open work items.

## Purpose

State when a numeric or procedural bound may exist at all. The default is no
bound: a limit is a product decision that silently defines what otherwise valid
work may accomplish, so it needs a real, demonstrated precedent before it can
exist. Liveness safeguards are the single exception, and even they exist to
prevent a concrete self-inflicted failure, never to ration work.

## The rule

A limit, quota, cap, reservation, counter, or bureaucratic bound may be added
only when a real, demonstrated precedent exists. A precedent is an observed
failure or an externally fixed constraint, not a hypothetical:

- an observed crash, hang, unbounded memory growth, or file/stream overrun;
- a bound imposed by a real platform, protocol, or operating-system interface;
- a capacity fact the system must report rather than hide.

The precedent is written down together with the failure it prevents. If no
precedent is recorded, the limit does not ship. The following are forbidden:

- product counters that cap calls, actions, messages, retries, page items,
  catalog entries, child depth, lifetime, or output without a measured reason;
- reservations, corridors, leases, or entitlements that turn capacity into
  admission policy;
- speculative contract-level shape caps (for example field-size or
  count-of-items validation) that reject otherwise valid typed content;
- truncation markers that pretend a capped result is complete: where a read or
  output window ends content, the cut is reported through an explicit
  truncation flag and marker, never refused and never presented as complete;
- calendar or period quotas (Day/Week/Month) and queue-admission audits or
  reconciliation machinery added without a demonstrated queue failure.

Capacity availability stays a typed, observable outcome. A resource that is
temporarily unavailable reports unavailability; it is never converted into a
quota, a successful truncated result, or an admission ceiling.

## Keep-list: liveness safeguards

These bounds already prevent a concrete self-inflicted failure and stay. Each
one is a transport, storage, or runtime liveness safeguard, not a policy quota;
if any is ever revisited, its precedent and failure mode are recorded with it.

| Safeguard | Where | Precedent it answers |
| --- | --- | --- |
| Maximum message size | Local transport framing | A single inbound/outbound message could exhaust memory or stall the connection. |
| Connect and synchronous I/O timeouts | Local transport client/server | A dead or stalled peer could hang a caller indefinitely. |
| Stale-socket probe and reclaim | Daemon socket lifecycle | A leftover socket file after a crash could block startup or connect. |
| Storage read bounds (`MAX_TAIL_FACTS`, `MAX_*_BYTES`) | Storage reads/pages | An unbounded read could load unbounded history into memory. |
| Tool read and output windows | Tool read and render path | A single large file, match set, or command stream could force unbounded allocation or a hang; the cut is reported explicitly. |
| Host subscriber queue and write deadline (`SUBSCRIBER_QUEUE_CAPACITY`, `SUBSCRIBER_WRITE_DEADLINE`) | Daemon run streaming | A peer that stops accepting frames could otherwise block execution, persistence, or healthy subscribers. |
| Model progress and retry timeouts | Model runtime | A provider stream that stops producing could hang a run forever. |

These seven safeguards are the only numeric safeguards retained; a new one or a
changed value requires its recorded reason. Socket permissions (0700/0600) and
boundary redaction are structural protections rather than numeric limits, and
they stay. Everything outside this keep-list needs a precedent, and a removed
limit is never replaced by a warning, a soft cap, a counter, or a periodic
audit. When a bound is kept, the record says what it protects and why the
specific value is sufficient; the value itself is not policy.

## Related policy

- Runtime credential-content scanners are removed and banned: user, model, and
  tool content is never scanned or filtered for credential-shaped strings.
  Secrets are protected structurally (never collected, echoed, stored, or
  logged), while the CI documentation secret scan and the fake-secret absence
  tests remain.
- Corridors, reservations, the Day/Week/Month period engine, and
  unavailable-queue promotion/reconciliation audits are removed from the
  corpus; the M3 turn queue, its tickets, and its promotion were removed by
  [ADR 0019](decisions/0019-pending-turns-and-cooperative-interruption.md), and a user turn accepted during an
  active run is a pending turn joined to that run's live context.
- The protocol and domain contracts are typed serde JSON; canonicalization is
  RFC 8785 only, and only when a first real consumer exists.
