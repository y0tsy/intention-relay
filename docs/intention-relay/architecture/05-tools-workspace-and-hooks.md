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

M5 activates six executable registry entries: `read`, `write`, `edit`,
`execute`, `glob`, and `grep`. `fetch_url`, `ask_user`, `todo`, `retrieve`,
`plan_submit`, `sub_agent`, `expand`, and `mcp` remain reserved slots with no
input/output contract or executor and are not active tools.

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

### Execution-kind scope

The containment rules in this document apply to ordinary M3/M4 and ordinary v1
execution. Future Mandate WorkspaceRoot semantics are owned by architecture 15:
WorkspaceRoot supplies the default relative base and `execute` CWD, while explicit
absolute or parent paths are observed and evidenced rather than denied solely by
location. Hooks remain typed and mandatory in both modes, but future Mandate hooks
cannot add discretionary confirmation, risk, corridor, quota, reservation, or
root-origin authorization.


## WorkspaceRoot is mandatory

A session's `WorkspaceRootDto` is passed to every tool that reads, writes, searches, expands, or executes against a local path/process.

### Required behavior

- all relative paths resolve from `workspace_root`;
- tools must not use process `pwd` as a fallback;
- absolute paths are normalized and rejected if outside the allowed root;
- symbolic-link and path traversal behavior must be explicitly verified before access. The proven v1 policy allows symlinks only when their resolved target is proven to remain within `workspace_root`; outward, unprovable, and dangling symlinks are rejected fail-closed;
- `execute` always starts with `cwd = workspace_root` and inherits the
  invoking process environment without name-based filtering. WorkspaceRoot is
  a filesystem and CWD boundary, not an environment or privilege boundary.
- a tool result identifies the normalized path/CWD used, with safe redaction as necessary;
- plan artifact storage is not implicitly included in `workspace_root`; it is authorized by mode policy.

A raw `PathBuf` alone is not a workspace contract. It must be wrapped in an input DTO with semantic intent and pass the workspace hook.

This check is necessarily subject to a TOCTOU residual risk: validation and the
subsequent filesystem operation are separate OS operations. M5 narrows that
risk with repeated symlink metadata checks and fail-closed errors, but does not
claim atomic filesystem confinement or sandbox/privilege isolation.

### Safe missing-path outcome

When M5 implements a file-oriented `not_found` outcome, it uses `ErrorDto` with `ErrorDetailDto::MissingWorkspacePath { path: WorkspaceRelativePathDto }`. `path` is the logical relative path supplied under the authorized workspace, such as `src/missing.rs`. The tool must not disclose the absolute workspace root, a canonical or symlink target, an OS error string, command details, or file content in the error message, detail, or display form.

## Tool pipeline

```mermaid
flowchart LR
  MT[Model tool call] --> IV[Invocation DTO]
  IV --> BI[Before invocation]
  BI --> WR[Workspace resolve]
  WR --> BV[Boundary validate]
  BV --> BE[Before execute]
  BE --> EX[Base tool]
  EX --> AE[After execute]
  AE --> PE[Persist result]
  PE --> MC[Model context hook]
  MC --> PB[Publish event]
  PB --> CE[Continue provider exchange]
```

<!-- The phases map to the typed hook lifecycle. Base tools do primitive work only. -->

The model-tool loop feeds this pipeline: a provider-emitted tool call becomes
a typed invocation built by the application, executes through the daemon-owned
registry, and its durable result is persisted before publication and returned
to the provider exchange as a tool-role message. Provider adapters never
execute local tools. The runtime owns the bounded provider continuation, and
the loop is bounded by the immutable run execution policy.

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

### M5 ownership and execution order

The composition root registers the workspace and hook services. The
application owns the pipeline and durable lifecycle/result persistence; the
dispatcher owns typed ordering and short-circuit outcomes; base tools perform
only primitive work. VFR, Headroom, and Plan owners are not active M5
implementations merely because their hook phases exist.

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

v1 does not sandbox tools or containerize processes. `WorkspaceRoot` prevents
accidental path drift and tool-level filesystem escape for regular typed path
operations, but a shell command can still interact with the wider user
environment. This limitation is explicit in Plan and Build Autopilot. Plan's
advisory instruction is not a technical boundary.

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

- exact capability taxonomy and audit policy for `execute`, network, and
  destructive file actions. Build Autopilot does not use per-action
  confirmation; Plan `execute` is advisory-guided and trusted-local.

## Autopilot and Mandate tool boundary

Existing M3/M4 containment and confirmation behavior remains historical. The
accepted Build Autopilot policy intentionally removes per-action confirmation
for the configured Build surface. Future Mandate execution also differs:
WorkspaceRoot is a required default base/CWD with safe observation, not a path
containment authority, and compatible frozen active descriptors admit without
ordinary confirmation or risk gates. Hooks remain typed and mandatory but cannot
recreate discretionary Mandate authorization. The fixed registry, descriptor
revisions, direct admission, and loop details are owned by [Tool registry and
direct Mandate tool loop](15-tool-registry-and-mandate-tool-loop.md). This does
not create a second registry or bypass path. Plan `execute` remains available
under advisory focus guidance and is not a sandbox; ordinary Plan `write` and
`edit` remain denied.
