# M5 Closure Evidence

## Status and purpose

M5 is recorded against implementation baseline `b969546` (the current clean
HEAD). No immutable M4+ concept document is changed by this closeout. The
implementation activates typed core tools, fail-closed `WorkspaceRoot`
resolution, and typed deterministic hooks.

This document records observed evidence from the clean implementation baseline.

| Item | Value |
| --- | --- |
| Implementation baseline | `b969546` (`chore(deps): update uuid to 1.26.0`) |
| Focused verification | `cargo test -p intention-tools -p intention-workspace -p intention-hooks` — exit status `0`, executed 2026-08-26 in the current worktree |
| Focused result | 32 tests passed, 0 failed, 0 ignored; all three package doctest suites passed |
| Documentation verification | `make docs-check` — exit status `0`, executed 2026-08-26; Rust docs passed for default, no-default-features, and all-features profiles, and Markdown/Mermaid/navigation/secret checks passed |
| Full required gate | `make verify` — exit status `0`, completed on clean HEAD `b969546` |
| Environment | Linux `7.1.8-200.fc44.x86_64`, `x86_64` GNU/Linux; repository reports stable Rust toolchain via `rust-toolchain.toml` |
| Coverage scope and result | `default`, `no_default`, and `all` feature profiles; each reports the same per-crate totals. Region coverage ranges from 84.62% (`intention-transport`) to 98.42% (`intention-workspace`); tools 89.02%, workspace 98.42%, hooks 96.91%. Function coverage for the profiles ranges from 68.10% to 100%; line coverage ranges from 86.39% to 98.16%. Threshold evaluation passed with the configured profile scope. |
| Coverage policy exception | `intention-tools` and `intention-hooks` each have an explicitly approved 80% floor override. Tools: M5 tool-boundary transition floor; owner `intention-tools`; reviewer M5 quality council; reviewed 2026-08-26. Hooks: M5 hook-boundary transition floor; owner `intention-hooks`; reviewer M5 quality council; reviewed 2026-08-26. Ordering, rejection, short-circuit, policy-denial, and workspace-path semantic tests remain mandatory; all other crate tiers and branch requirements remain unchanged. |
| CI matrix | Linux/Windows CI matrix (`ubuntu-24.04` and `windows-2025`, `make ci`) has not been run for this closeout; Windows behavior is therefore not independently evidenced. |
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

- Required `ubuntu-24.04` and `windows-2025` `make ci` results are not recorded
  for this M5 baseline. In particular, Windows named-pipe/filesystem behavior
  remains outside this evidence run.
- The current worktree already contained unrelated or parent-agent changes;
  this closeout does not stage, commit, or normalize them.
- Later Plan/Build artifact policy remains M7 scope. M5 evidence does not claim
  physical plans, Plan mode, Build Autopilot, or UI delivery.

## Closeout rule

This is a documentation-only evidence record for implementation baseline
`b969546`. The complete local quality gate and coverage/profile results are
recorded above; the Linux/Windows CI matrix remains a follow-up before M5 is
declared fully closed across supported platforms.
