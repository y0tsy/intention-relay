# ADR 0042: Project script library for kernel cells

## Status

Accepted as a documentation-approved future direction that extends
[ADR 0012](0012-ipython-kernel-lifecycle.md) and
[ADR 0027](0027-child-kernel-bridge-mcp-detail-directions.md) without changing
them, within the retrospective scope of
[ADR 0035](0035-m5plus-complete-foundation-activation.md). It is not a slice
activation: it authorizes no crate, Python/Jupyter dependency, listener,
protocol implementation, storage schema, migration, feature profile,
quality-policy target, process supervisor, or production kernel execution, and
it leaves the M5+ Slice 3 and Slice 4 reservations untouched.

Amended 2026-09-30: the canonical digest fields, the bounded import-evidence
wording, and the workspace-boundary failure clause are superseded by
[ADR 0046](0046-typed-serde-json-contracts.md) and
[ADR 0047](0047-workspace-root-addressing-anchor.md) as recorded below.

## Scope and supersession

This decision adds one accepted capability to the future run-scoped IPython
kernel direction: an agent that writes a reusable Python module saves it as an
ordinary project file under a declared hidden folder, and a later run reuses it
by reading that file instead of depending on namespace carry-over. It
supersedes nothing. It does not replace the checkpoint direction, the Skill
model, the tool registry, or the workspace path policy; it names the location
rule those owners apply and the kernel-side import and evidence rules that go
with it.

The direction answers a concrete gap: checkpoints persist namespace values
under a rule that excludes executable payload, so an agent could keep a
variable across runs but not the code that produced it. Reusable code is a
project artifact, not checkpoint state.

## Decision

The project script library is the logical, slash-separated, workspace-relative
path `.ir/scripts` under the session's `WorkspaceRoot`, and `.ir` is the
project-local hidden root for agent-authored reusable material. This decision
declares only the `scripts` subtree.

```text
KernelScriptLibraryV1
  script_library_contract_revision
  library_path = .ir/scripts
  import_surface_revision
  script_evidence_revision
```

- The library holds plain Python module files. It is not a package repository:
  no wheels, vendored dependencies, installation instructions, or network
  acquisition belong to it.
- Files are created and edited only through the existing frozen registry
  descriptors `write` and `edit`, read through `read`, `glob`, and `grep`, and
  run through `execute` or a kernel foreground cell. No new `ToolId`, registry
  slot, listener, primitive path, or file API is introduced.
- Saving is deliberate. An agent persists a module when reuse is expected; the
  daemon never writes, rewrites, or collects a script on its own, and no
  namespace or checkpoint migration creates one.
- The immutable kernel selection gains one additive reference to
  `KernelScriptLibraryV1`. The reference carries the contract revisions and the
  logical relative library path; it carries no file content, no absolute path,
  and no credential.
- An epoch's import surface contains exactly the referenced library directory.
  The parent, the workspace root, a second library, and any path outside the
  workspace never enter it. This is a kernel-side scope choice, not a
  containment guarantee
  ([ADR 0047](0047-workspace-root-addressing-anchor.md)). A missing library
  directory yields an empty import surface rather than a failure, so a project
  without saved scripts behaves exactly as today.
- A fresh run reuses a script by reading the file inside its own new epoch; a
  live namespace, cell, grant, task, checkpoint, or process is never carried
  over to provide reuse.
- Every foreground cell that imports library modules records script-import
  evidence: for each imported module, the logical workspace-relative path. The
  evidence contains no source text, no absolute path, and no Python value, and
  it is published as run facts through architecture 15's post-commit
  publication gate under architecture 20's cell rules.
- Checkpoint payload and metadata never contain script source, and the existing
  exclusion of executable payload is unchanged.
- Retention and deletion stay explicit user or agent actions through the
  ordinary tools. No silent garbage collection, no age-based eviction, and no
  deletion triggered by idle disposal, checkpoint promotion, or restart.

## Invariants

1. No authority. A library path, an import, or a persisted module grants no
   lifecycle, scheduling, tool, child, verifier, MCP, bridge, reconciliation,
   or confirmation authority, and it never substitutes for a frozen registry
   selection.
2. No executable payload in checkpoint payload, namespace snapshot, or any
   public or durable surface. The library is file material, not state carried
   inside a checkpoint.
3. Epoch isolation holds: one live epoch belongs to exactly one admitted run, a
   fresh run never reuses a live kernel or namespace, and reuse happens only
   through the persisted project file.
4. The import surface is exact. A library path that cannot be addressed under
   the session `WorkspaceRoot` fails before any cell effect with the closed
   `kernel_script_library_unavailable`. The root is an addressing anchor, not a
   security boundary
   ([ADR 0047](0047-workspace-root-addressing-anchor.md)).
5. Import evidence is deterministic. The same modules in the same order produce
   the same evidence list; an unrepresentable list fails before publication
   instead of being truncated, sampled, or stringified.
6. Scripts are untrusted project material executed under the existing
   trusted-local model. They are not sandboxed, and the library neither widens
   nor narrows that model.
7. No secret material. Credentials, tokens, endpoints, and provider values are
   never written into the library, and the redaction rules of architecture 09
   apply to every path and error this capability produces.

## Compatibility

M3/M4 bytes, IDs, events, snapshots, replay, recovery, tool denial, and
retained IPython/RLM history keep their recorded meaning. The capability adds no
second kernel version, no second workspace root, and no parallel registry,
storage, or wire family; under
[ADR 0038](0038-no-backward-compatibility-and-legacy-removal.md) the library is
part of the single first-scope kernel contract rather than a compatibility
layer. A project that has no `.ir/scripts` directory keeps today's behavior,
including an empty import surface.

## Security and failure behavior

The closed kernel safe-failure set gains one member:

```text
kernel_script_library_unavailable
```

It covers a library path that cannot be addressed under the session root and
script-import evidence that cannot be represented. It discloses no credential,
no absolute path, no Python value, no Jupyter frame, no raw traceback, and no
implementation detail. A Python-level
import error raised inside a cell remains ordinary safe `Error` output under
architecture 20's closed text-only projection.

## Non-goals

Automatic saving or collection of scripts, silent garbage collection or
retention policy, a package manager, dependency installation, vendored wheels,
network acquisition, a sandbox or privilege boundary, a second interpreter, a
cross-project or shared library, changes to the Skill model, a new tool
descriptor, direct filesystem access that bypasses the tool gateway, and any
implementation authorization are outside this decision.

## Affected documents

- [Architecture 20](../architecture/20-ipython-kernel-lifecycle.md) owns the
  kernel-side selection reference, import surface, script-import evidence,
  checkpoint exclusion rule, and closed failure.
- [ADR 0046](0046-typed-serde-json-contracts.md) removes the canonical digest
  fields and [ADR 0047](0047-workspace-root-addressing-anchor.md) replaces the
  boundary-validation wording, as recorded in the status above.
- [Architecture 05](../architecture/05-tools-workspace-and-hooks.md) owns the
  project-local path convention and the ordinary tool rules that create,
  read, and run the modules.
- [Architecture 15](../architecture/15-tool-registry-and-mandate-tool-loop.md)
  owns Mandate-scoped `WorkspaceRoot` semantics, the frozen-descriptor rule
  that admits the existing tools unchanged, and the publication gate that
  carries the import evidence.
- [Architecture 21](../architecture/21-goals-skills-context-memory-and-compaction.md)
  records the boundary that the library is not a Skill body, supplement, or
  package reference.
- [Architecture 09](../architecture/09-configuration-security-and-observability.md)
  redaction and classification rules apply unchanged.
- [Architecture 11](../architecture/11-implementation-roadmap.md),
  [architecture README](../architecture/README.md), and the reconciliation
  registers carry the ownership, topic, and evidence rows.

## Evidence

A later activating specification must declare exact crate owners, test targets,
coverage tiers, feature profiles, and architecture fixtures, then pass
`make quick`, `make verify`, and Linux/Windows CI. It must cover at minimum:

- dot-folder path resolution through `WorkspaceRoot` with no process-CWD
  fallback, and a logical relative path in every tool result and error;
- an import surface containing exactly the library directory, never the parent,
  the workspace root, a second library, or an outside path;
- a missing library directory yielding an empty import surface and unchanged
  behavior;
- fail-closed handling of a library path that cannot be addressed under the
  session root, with `kernel_script_library_unavailable` before effect;
- deterministic script-import evidence and the absence of source text,
  absolute paths, and Python values;
- checkpoint payload and metadata that carry no script source;
- no automatic save, rewrite, or collection path, and explicit-only deletion;
- project reuse across runs through the file with no namespace, cell, grant,
  task, or checkpoint carry-over; and
- secret-free, path-free, and raw-output-free public and durable surfaces.

## Research provenance

`m4plus_concept.md` research,
[`docs/reference/prime-agent-research/rlm-ipython-harness-integration-analysis.md`](../../reference/prime-agent-research/rlm-ipython-harness-integration-analysis.md)
(persistent scratch state in the RLM/IPython harness analysis),
[architecture 20](../architecture/20-ipython-kernel-lifecycle.md) checkpoint
exclusion of executable payload, and the user-requested direction recorded on
2026-09-25 that an agent must be able to persist its reusable kernel scripts in
the project rather than only namespace variables.
