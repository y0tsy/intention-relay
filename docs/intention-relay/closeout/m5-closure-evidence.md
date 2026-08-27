# M5 Closure Evidence

## Status and purpose

M5 is recorded as a **worktree evidence closeout** at the current repository
revision (`12217d96e1a2ec51c3cd13f59520a14a56a54878`). No immutable M4+ concept
document is changed by this closeout. The implementation activates typed core
tools, fail-closed `WorkspaceRoot` resolution, and typed deterministic hooks.

This document records only observed evidence from the current worktree. It does
not claim a committed M5 implementation baseline or a completed full quality
gate.

| Item | Value |
| --- | --- |
| Evidence revision | `12217d96e1a2ec51c3cd13f59520a14a56a54878` (`docs(autopilot): define trusted build continuity`) |
| Focused verification | `cargo test -p intention-tools -p intention-workspace -p intention-hooks` — exit status `0`, executed 2026-08-26 in the current worktree |
| Focused result | 32 tests passed, 0 failed, 0 ignored; all three package doctest suites passed |
| Documentation verification | `make docs-check` — exit status `0`, executed 2026-08-26; Rust docs passed for default, no-default-features, and all-features profiles, and Markdown/Mermaid/navigation/secret checks passed |
| Full required gate | Not run for this closeout; no `make verify` or CI result is claimed |
| Environment | Linux `7.1.8-200.fc44.x86_64`, `x86_64` GNU/Linux; repository reports stable Rust toolchain via `rust-toolchain.toml` |
| Full-environment deviation | The required full Linux/Windows matrix and complete `make verify` evidence are unavailable in this worktree record; Windows behavior is therefore not independently evidenced here |
| Coverage policy exception | `intention-tools` and `intention-hooks` have explicit 80% overrides, with an 80% approved floor and rationale/owner/review metadata; all other crate tiers and branch requirements remain unchanged |
| Immutable documents | `m4plus_concept2.md` and other immutable documents were not edited |

## Acceptance evidence

| Requirement | Observed proof |
| --- | --- |
| Typed core tool registry | `intention-tools` unit and `tool_contracts` tests cover fixed unique registry ordering, DTO round trips, metadata, delegation, bounded input, file operations, search/glob, and typed failures |
| WorkspaceRoot policy | `intention-workspace` unit and `workspace_contract` tests cover explicit-root resolution, CWD independence, missing paths, new-file parents, and rejection of symlinks outside the root |
| Execute CWD | Contract tests prove execute uses the declared workspace root rather than process CWD |
| Typed hooks | `intention-hooks` unit and `hook_contracts` tests cover phase mapping, stable ordering, duplicate rejection, chained transforms, typed rejection, and short-circuit before tool execution |
| Tool event/lifecycle boundary | The focused tool contract suite covers typed metadata and observability DTO round trips; durable application/storage integration is not claimed by this focused run |

## Known limitations and follow-up

- `make verify` must be run before treating M5 as fully accepted. That gate is
  required to establish formatting, lint, architecture, feature-profile,
  coverage, documentation, and supply-chain evidence.
- Required `ubuntu-24.04` and `windows-2025` `make ci` results are not recorded
  for this M5 worktree. In particular, Windows named-pipe/filesystem behavior
  remains outside this evidence run.
- The current worktree already contained unrelated or parent-agent changes;
  this closeout does not stage, commit, or normalize them.
- Later Plan/Build artifact policy remains M7 scope. M5 evidence does not claim
  physical plans, Plan mode, Build Autopilot, or UI delivery.

## Closeout rule

This is a documentation-only evidence record for the observed worktree and
focused tests. A future implementation baseline should record its exact commit
SHA, complete `make verify` output, CI matrix links, coverage artifacts, and any
approved policy exceptions before M5 is declared fully closed.
