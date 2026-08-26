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

- activity identity is distinct from Session fork lineage and every other cursor;
- activity and notification delivery is negotiated, bounded, replay-safe, and
  read-only with respect to external work;
- notification cursor is observation-only, while acknowledgement is a separate
  durable presentation aggregate;
- Tauri/TUI/REPL consume the same daemon-owned DTO families through
  `intention-client`; and
- M3/M4 records remain unchanged and may have compatibility-only projections,
  never synthetic activity state.

## Compatibility and non-goals

This is documentation-only. It activates no crate, migration, protocol, OS
notification, quality policy, or UI implementation. Native notifications, remote
push, accounts, destructive retention, and visual information architecture remain
separate.

## Primary owner and evidence

[Activity, UI, and adapters](../architecture/24-activity-ui-and-adapters.md)
owns detailed behavior. A later M6 implementation requires contract, transaction,
replay, recovery, redaction, adapter-parity, cross-platform, and outcome evidence.

## Provenance

`m4plus_concept2.md`, selected agent communication, observation, notification,
and presentation material, reconciled against architectures 03 and 13--23.
