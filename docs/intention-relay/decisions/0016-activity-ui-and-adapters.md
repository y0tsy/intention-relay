# 0016: Activity, UI, and Adapters

## Status

Accepted.

## Decision

Future activity, notification, and acknowledgement behavior extends ordinary M6
through daemon-owned safe projections and one shared typed client path. Activity
identity, direct-pair communication, journals, notification summaries, and
presentation acknowledgement are separate durable concerns; they do not create
lifecycle, scheduler, tool, child/verifier, provider, fork, or reconciliation
authority.

## Invariants

- activity identity is distinct from Session fork lineage and every other
  cursor;
- activity and notification delivery is negotiated, bounded, replay-safe, and
  read-only with respect to external work;
- notification cursor is observation-only, while acknowledgement is a separate
  durable presentation aggregate;
- Tauri/TUI/REPL consume the same daemon-owned DTO families through
  `intention-client`; and
- M3/M4 records remain unchanged and may have compatibility-only projections,
  never synthetic activity state.

## Compatibility and non-goals

Native notifications, remote push, accounts, destructive retention, and visual
information architecture remain separate.

Owner: architecture 24. Evidence: activating specification per
[architecture 12](../architecture/12-quality-gates-and-makefile.md).

Provenance: `m4plus_concept.md`, selected agent communication, observation,
notification, and presentation material, reconciled against architectures 03
and 13--23.
