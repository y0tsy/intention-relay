# 0012: Run-Scoped IPython Kernel Lifecycle

## Status

Accepted.

## Decision

Future IPython is a private daemon-managed, run-scoped kernel sidecar. One
kernel epoch belongs to one admitted Mandate run and is disposed at
terminalization, cancellation, failure, or recovery. A later fresh run may seed
a replacement kernel only from an explicitly selected verified checkpoint.

Kernel host requests consume the negotiated Gateway/RLM bridge and the existing
Rust-owned tool path. The kernel creates no ToolId, registry, listener, daemon,
sequence, result stream, lifecycle authority, or direct primitive path.

## Invariants

- kernel selection is immutable credential-free Mandate meaning; grants, epochs,
  channels, namespace, handles, resources, and payloads are not;
- namespace/checkpoint state is convenience evidence, never lifecycle, scheduler,
  child, verifier, MCP, or reconciliation authority;
- checkpoints are private, verified, bounded, and exclude live resources and
  unfinished work;
- a required checkpoint restore fails closed; optional restore may start empty
  only in a new run with an explicit degraded outcome;
- cell effects and host requests follow the existing before-start/started/known/
  unknown law, and unproven started work pauses only its owning Mandate; and
- restart never adopts, reattaches, retries, resumes, reruns, or replays old
  kernel/cell/task/grant/operation/external work.

## Compatibility and non-goals

M3/M4 bytes and ordinary behavior remain unchanged. Historical M4 tool calls
remain denial evidence. Retained session-scoped IPython/RLM material remains
historical where it conflicts with run-scoped Mandate isolation. The kernel is a
trusted-local convenience sidecar, not a sandbox or privilege boundary.

This decision activates no crate, Python/Jupyter dependency, protocol, listener,
storage schema, migration, runtime, quality policy, or production behavior.
Resource-limit values, process supervision implementation, RLM executor topology,
continual harness, Skills/Goals/context, provider evolution, session forks,
activity/UI, and MCP administration remain separate.

## Primary owner and evidence

[Run-scoped IPython kernel lifecycle](../architecture/20-ipython-kernel-lifecycle.md)
owns detailed behavior. A later implementation requires canonical, lifecycle,
checkpoint, authority, fault/recovery, protocol, compatibility, redaction,
cross-platform, and outcome evidence through the standard quality gate.

## Provenance

`m4plus_concept.md`, selected IPython kernel lifecycle and Mandate recovery
overlay, reconciled against architectures 13--19.
