# 0004: Rust-Owned Capability Plane and Fixed Tool Registry

## Status

Accepted.

## Decision

All future model, bridge, kernel, child, and MCP capability invocation reaches
one daemon-owned Rust capability path. Concrete assembly belongs only to the
composition root. No subsystem may create a second registry, direct primitive
bypass, persistence authority, daemon, or lifecycle authority.

Registry/descriptor detail, direct Mandate admission, WorkspaceRoot behavior,
Plan/Build policy, tool-loop facts, and individual slots are deferred to later
owner packages.

## Invariants

- DTO-only capability boundaries;
- composition-only concrete selection;
- providers, bridges, Skills, MCP sources, and kernels are not authority;
- product controls are not OS sandboxing.

## Evidence

Future work requires dependency/API architecture fixtures, registry ownership
fixtures, and outcome paths proving calls cannot bypass the gateway.

## Provenance

`m4plus_concept.md`, unified registry, model-tool loop, bridge, kernel, Skill,
and MCP sections.
