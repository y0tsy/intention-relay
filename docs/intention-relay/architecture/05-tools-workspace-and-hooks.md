# Tools, Workspace, and Hooks

## Scope

This document specifies typed core tools, mandatory `WorkspaceRoot` enforcement, tool execution policy, and the hook system used by WorkspaceRoot, VFR, Headroom, and Plan mode.

## Tool ownership

`intention-tools` owns:

- typed tool metadata;
- input and output DTO contracts;
- typed tool registry;
- execution interfaces;
- core tool registrations.

Tools are domain/runtime capabilities, not UI commands. An adapter only renders tool events returned by the daemon.

## Core tool contract

Every tool has:

```text
ToolDescriptorDto
  tool_id
  display_name
  input_schema_version
  input_dto_type
  output_dto_type
  required_capabilities
  mutation_kind
  observability_policy

ToolInvocationDto
  invocation_id
  session_id
  run_id
  workspace_root
  mode
  input

ToolResultDto
  invocation_id
  outcome
  normalized_content
  structured_metadata
  policy_decisions
  timing
```

The concrete Rust API can use traits and generic DTOs, but the runtime registry must not accept untyped tool inputs or results.

## WorkspaceRoot is mandatory

A session's `WorkspaceRootDto` is passed to every tool that reads, writes, searches, expands, or executes against a local path/process.

### Required behavior

- all relative paths resolve from `workspace_root`;
- tools must not use process `pwd` as a fallback;
- absolute paths are normalized and rejected if outside the allowed root;
- symbolic-link and path traversal behavior must be explicitly verified before access;
- `execute` always starts with `cwd = workspace_root`;
- a tool result identifies the normalized path/CWD used, with safe redaction as necessary;
- plan artifact storage is not implicitly included in `workspace_root`; it is authorized by mode policy.

A raw `PathBuf` alone is not a workspace contract. It must be wrapped in an input DTO with semantic intent and pass the workspace hook.

### Safe missing-path outcome

When M5 implements a file-oriented `not_found` outcome, it uses `ErrorDto` with `ErrorDetailDto::MissingWorkspacePath { path: WorkspaceRelativePathDto }`. `path` is the logical relative path supplied under the authorized workspace, such as `src/missing.rs`. The tool must not disclose the absolute workspace root, a canonical or symlink target, an OS error string, command details, or file content in the error message, detail, or display form.

## Tool pipeline

```mermaid
flowchart LR
  IV[Invocation DTO] --> BI[Before invocation]
  BI --> WR[Workspace resolve]
  WR --> BV[Boundary validate]
  BV --> BE[Before execute]
  BE --> EX[Base tool]
  EX --> AE[After execute]
  AE --> PE[Persist result]
  PE --> MC[Model context hook]
  MC --> PB[Publish event]
```

<!-- The phases map to the typed hook lifecycle. Base tools do primitive work only. -->

## Hook system

`intention-hooks` owns typed registration, order, hook context, and dispatcher behavior.

### Hook phases

```text
BeforeToolInvocation
BeforeWorkspaceResolution
AfterWorkspaceResolution
BeforeToolExecution
AfterToolExecution
BeforeToolResultPersist
BeforeToolResultModelContext
AfterToolResultPublished
```

### Hook contract rules

- each hook declares supported phases and explicit priority;
- the registry produces a deterministic order;
- a hook receives only the typed context it needs;
- hooks return a typed continue/transform/reject outcome;
- a rejection creates a policy/result DTO, never an unstructured panic;
- hook failures are classified as fail-closed or fail-open per phase and policy, not by incidental error handling;
- hooks cannot directly commit storage or publish a competing event;
- hook execution itself is observable with safe metadata.

### Required initial hooks

| Hook owner | Responsibility |
| --- | --- |
| `intention-workspace` | Resolve paths, validate workspace boundary, set process CWD. |
| `intention-plans` | Enforce Plan-mode artifact directory mutations and hide frontmatter. |
| `intention-vfr` | Transform eligible read output into a virtual representation. |
| `intention-headroom` | Transform eligible tool output before model-context insertion. |

## Policy separation

The following must remain distinct:

| Concern | Owner |
| --- | --- |
| Tool's primitive work | Base tool implementation. |
| Path/CWD enforcement | WorkspaceRoot hook. |
| Plan-mode mutation authorization | Plan policy hook. |
| Compression and retrieval metadata | Headroom hook. |
| Virtual source transformation | VFR hook. |
| Confirmation/risk determination | Application/runtime policy service. |
| Persistence transaction | Application/storage. |
| UI rendering | Adapter. |

## Trusted workspace boundary

v1 does not sandbox tools or containerize processes. `WorkspaceRoot` prevents accidental path drift and tool-level filesystem escape, but a shell command can still interact with the wider user environment. This limitation is explicit, especially in Plan mode.

## Required tests and outcomes

| Requirement | Test evidence | Observable outcome |
| --- | --- | --- |
| Relative resolution | Tool contract test with changed process CWD. | Tool reads/writes only under declared `WorkspaceRoot`. |
| Absolute escape | Path-policy test. | Outside-root path returns typed policy error. |
| Traversal/symlink | Security-focused workspace test. | Escape attempt is rejected or safely resolved according to documented policy. |
| Execute CWD | Process fixture test. | Child process observes workspace root as CWD. |
| Hook ordering | Registry unit/property test. | Same registration yields deterministic execution order. |
| Hook rejection | Integration test. | Rejected invocation persists/publishes a typed outcome without base tool execution. |
| Adapter independence | Daemon tool-stream contract test from TUI and bridge clients. | Both see the same tool event DTOs. |

## Quality-gate integration

Tool, WorkspaceRoot, and hook enforcement are Tier B coverage targets and blocking `make verify` inputs. Architecture checks must reject direct process-CWD fallback and VFR/Headroom coupling inside base tools. Line coverage cannot replace the explicit path-escape, execute-CWD, hook-order, and policy-denial scenarios above. See [12 Quality Gates and Makefile](12-quality-gates-and-makefile.md).

## Open decisions

- exact symlink policy for paths whose resolved target leaves a workspace;
- which core tools ship in the first vertical slice;
- exact tool capability taxonomy;
- risk categories and confirmation policy for `execute`, network, and destructive file actions.

## Post-M4 Mandate tool boundary

The v1 ordinary containment and confirmation rules above remain authoritative
for ordinary/M3/M4 behavior. Future Mandate execution intentionally differs:
WorkspaceRoot is a required default base/CWD with safe observation, not a path
containment authority, and compatible frozen active descriptors admit without
ordinary confirmation or risk gates. Hooks remain typed and mandatory but cannot
recreate discretionary Mandate authorization. The fixed registry, descriptor
revisions, direct admission, and loop details are owned by [Tool registry and
direct Mandate tool loop](15-tool-registry-and-mandate-tool-loop.md). This does
not change current tools or activate future slots.
