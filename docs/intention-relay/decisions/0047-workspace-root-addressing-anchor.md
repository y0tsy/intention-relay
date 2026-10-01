# ADR 0047: WorkspaceRoot as an addressing anchor

## Status

Accepted 2026-09-30. It reduces `WorkspaceRoot` to three functions: the
anchor for relative paths, the working directory of child processes, and the
default search scope. It removes the lexical symlink machinery and the
containment checks, and it records that the root is not a security boundary.
It supersedes the containment and symlink clauses of accepted directions,
including [ADR 0042](0042-project-script-library-for-kernel-cells.md), as
recorded below. It activates no sandbox and no new isolation mechanism.

## Scope and supersession

In scope: `intention-workspace` root semantics and path resolution,
`intention-tools` path handling for `write`, `edit`, `glob`, and `grep`, the
default search scope, the removed workspace error codes, and the tests that
assert them.

| Record | Superseded clause | Replaced by |
| --- | --- | --- |
| [ADR 0042](0042-project-script-library-for-kernel-cells.md) | The `kernel_script_library_unavailable` condition for a library path that "fails the workspace boundary check", and the wording that resolves "through an outward, unprovable, or dangling symbolic link" | A library path that cannot be addressed under the session root fails before effect; the kernel import surface remains a kernel-side scope choice, not a containment guarantee |
| [ADR 0025](0025-base-tool-contracts-and-tool-loop-bounds.md) | The invariant wording that ties `execute` to `WorkspaceRoot` CWD is read through this record; its substantive rule (ordinary OS authority, no sandbox) is unchanged | Child processes start with the root as their working directory, which addresses rather than restricts |

Any other accepted direction that treats the root as a security boundary, or
that requires a lexical symlink or containment check, is read through this
record. The removed failure codes `workspace_path_symlink` and
`workspace_path_outside_root` no longer exist and are not replaced.

## Decision

### Root semantics

1. `WorkspaceRoot` stores the root as given. It does not canonicalize each
   path, does not scan path components for symbolic links, and does not fail
   closed on symbolic links.
2. `resolve_path(relative) = root.join(relative)`.
   `resolve_new_file_path(relative)` uses the same join. There is no separate
   resolution algorithm and no per-tool alias.
3. `execute_cwd() = root`. Child processes run with the workspace root as
   their working directory; `execute` already works this way, and this record
   confirms it as the rule.
4. `glob` and `grep` with no explicit path search from the workspace root.
   The default scope is an addressing convention: it decides what a pathless
   call addresses, not what the process may read.
5. The root does not contain. `root.join(relative)` is the whole addressing
   rule: `root.join("/etc/passwd")` yields the absolute path and
   `root.join("../x")` yields the parent-relative path, with no containment
   check at resolution. The typed inputs still reject absolute and parent
   (`..`) paths before resolution — `WorkspaceRelativePathDto` for tool paths
   and the search-pattern validator for `glob` and `grep` patterns — but that
   is an input-shape check, not a boundary. Non-containment is observable
   through symbolic links: a path inside the root may resolve outside it
   through a link, and nothing detects that.

### Removed machinery

6. `intention-workspace` removes `contains_symlink_component`, the per-path
   canonicalization, the fail-closed resolution logic, the
   `workspace_path_symlink` and `workspace_path_outside_root` error codes,
   and the related error helpers.
7. `intention-tools` removes the duplicate symlink checks in `write`, `edit`,
   `glob`, and `grep`, and the local `contains_symlink_component`. The
   `MAX_GLOB_MATCHES` cap and its truncated-result behavior are removed by
   [ADR 0048](0048-limits-by-precedent-and-no-content-scanning.md).
8. Tests are rewritten to positive semantics: a relative path joins to the
   root, `execute` sees `cwd = root`, a pathless `glob` or `grep` searches
   from the root, and the removed symlink and outside-root cases no longer
   exist.

## Invariants

1. Join only. A relative path addresses the root joined with that path; there
   is no second resolution rule and no canonicalized path identity.
2. No containment claim. The root contains nothing: a resolved path may leave
   it through a symbolic link, and the typed inputs reject absolute and `..`
   paths as an input-shape rule rather than a boundary. Symbolic links are
   ordinary filesystem material rather than a path-check input.
3. No security boundary. The root is a project scope and an addressing
   anchor. The daemon and its child processes run with the user's ordinary OS
   authority; the root neither grants nor removes access to anything. Real
   isolation, if it is ever required, must be an OS-level boundary such as a
   sandbox, container, or ACL, never a lexical path check.
4. One root. Exactly one `WorkspaceRoot` exists per session; there is no
   second root and no per-tool root.
5. Predictable cwd. Child processes of `execute` start in the root.
6. Default scope. A pathless `glob` or `grep` addresses the root.
7. Trusted local unchanged. The trusted-local model is unchanged, including
   the ordinary OS authority of `execute`.

## Compatibility

M3/M4 sessions and runs keep their recorded root values and stored paths.
Removing the containment checks does not rewrite or reinterpret persisted
data. Relative addressing and the `execute` working directory behave as the
current runtime already behaves, so this record simplifies the code and the
documentation rather than changing relative-path behavior. An absolute path or
a `..` component is still rejected by the typed input validation
(`WorkspaceRelativePathDto` and the `glob`/`grep` pattern validator) before it
reaches the root; the removed error codes are deleted rather than replaced, so
no new error path appears.

## Security and failure behavior

This record corrects an over-claim. The lexical checks did reject absolute
paths, `..`, and symlinked path components at check time, but they could not
enforce a boundary: the check and the filesystem operation that follows it are
separate, so a check-then-use race defeats them, and a lexical check is not an
OS boundary. Their presence suggested an isolation guarantee that never
existed. After this record, the documentation says plainly that the root is
not a boundary, while the typed inputs (`WorkspaceRelativePathDto` and the
`glob`/`grep` pattern validator) still reject absolute and parent paths as an
input-shape rule rather than a containment guarantee. The daemon still relies
on OS permissions, socket permissions, and the trusted-local model, none of
which change. No code path
emits `workspace_path_symlink` or `workspace_path_outside_root` after the
change. Tools and kernel features that need a scope state it as an addressing
rule and do not present it as containment.

## Non-goals

No sandbox, chroot, container, or ACL work; no capability-based filesystem;
no path-scoped permission model; no second workspace root; no per-tool root;
no change to the OS authority of `execute`; no compatibility path for the
removed error codes; no new configurable root policy.

## Affected documents

- [Architecture 05](../architecture/05-tools-workspace-and-hooks.md) owns the
  workspace, path, and tool rules that this record simplifies.
- [Architecture 02](../architecture/02-dto-and-contract-policy.md) owns the
  error vocabulary that loses the two removed codes.
- [Architecture 11](../architecture/11-implementation-roadmap.md) and the
  [architecture README](../architecture/README.md) record the new root
  semantics.
- The [reconciliation registers](../reconciliation/README.md) record the
  removed containment rows and the corrected boundary wording.
- [ADR 0042](0042-project-script-library-for-kernel-cells.md) carries the
  superseded boundary clause listed above; its other rules stand.
- [Decisions README](README.md) indexes this record.

## Evidence

The change is accepted only together with:

- the workspace contract tests rewritten to the positive join, cwd, and
  scope semantics, with the symlink and outside-root cases removed;
- the `intention-tools` tests updated to the same semantics, including the
  removal of the glob match cap;
- a repository search receipt showing no `contains_symlink_component`,
  `workspace_path_symlink`, or `workspace_path_outside_root` remains;
- green `make quick`, `make verify`, `docs-check`, and Linux/Windows CI,
  including the platform-native path fixtures;
- an unchanged live `make e2e-real-api` path, where tools continue to run
  under the session root.

## Research provenance

The audit finding that two independent symlink scanners existed, one in
`intention-workspace` and a duplicate in `intention-tools`, that neither
provided containment, and that both encoded an isolation claim the trusted
local model does not make; the existing `execute` behavior of running with
the root as its working directory; and the decision to state the addressing
semantics honestly instead of keeping unenforceable checks.
