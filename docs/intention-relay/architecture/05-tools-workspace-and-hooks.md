# Tools, Workspace, and Hooks

**Current policy.**

This document specifies typed core tools, `WorkspaceRoot` addressing semantics, tool execution policy, and the hook
system used by WorkspaceRoot, VFR, Headroom, and Plan mode.

## Tool ownership

`intention-tools` owns:

- typed tool metadata;
- input and output DTO contracts;
- typed tool registry;
- execution interfaces;
- core tool registrations.

Tools are domain/runtime capabilities, not UI commands. An adapter only renders tool results returned by the daemon.

## Core tool contract

M5 activates six executable registry entries: `read`, `write`, `edit`, `execute`, `glob`, and `grep`. `fetch_url`,
`ask_user`, `todo`, `retrieve`, `plan_submit`, `sub_agent`, `expand`, and `mcp` remain reserved slots with no
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

ToolContext
  session_id
  run_id
  call_id

ToolInput
  read | write | edit | execute | glob | grep  (typed per-tool parameters)

ToolDispatchOutcome
  completed(ToolResult) | interrupted(cause, partial)

ToolResultProjection
  schema_version
  tool
  content
  execution
```

Every active descriptor also declares `model_parameters_schema`: the code-owned JSON Schema text for its typed model
parameters. `intention_tools::model_visible_descriptors()` returns exactly the active descriptors that expose such a
schema, in registry order; the current model-visible set is `read`, `write`, `edit`, `execute`, `glob`, and `grep`, and
reserved slots are never included. Ordinary model requests advertise that set as typed tool definitions
([architecture 08](08-model-protocol-and-providers.md)).

The concrete Rust API can use traits and generic DTOs, but the runtime registry must not accept untyped tool inputs or
results.

## WorkspaceRoot is a required addressing anchor

A session's `WorkspaceRootDto` is passed to every tool that reads, writes, searches, expands, or executes against a
local path/process.

### Required behavior

- `resolve_path(relative) = workspace_root.join(relative)`: exactly one
resolution rule, no per-path canonicalization, and no per-tool alias;
- exactly one `WorkspaceRoot` exists per session; there is no second root and no per-tool root;
- tools must not use process `pwd` as a fallback;
- absolute paths and `..` are not contained; they are addressed as given, and
this is deliberate;
- symbolic links are ordinary filesystem material: no lexical symlink parser,
no containment check, and no fail-closed path rejection exists;
- `glob` and `grep` with no explicit path search from `workspace_root`; the
default scope decides what a pathless call addresses, not what the process may read;
- `execute` always starts with `cwd = workspace_root` and inherits the
invoking process environment without name-based filtering. WorkspaceRoot is an addressing anchor and default CWD, not a
security, environment, or privilege boundary.
- a tool result identifies the path/CWD used, with safe redaction as necessary;
- plan artifact storage is not implicitly included in `workspace_root`; it is authorized by mode policy.

### Project script library

The project script library is the logical, slash-separated, workspace-relative path `.ir/scripts` under
`workspace_root`, with `.ir` as the project-local hidden root for agent-authored reusable material
([architecture 20](20-ipython-kernel-lifecycle.md)):

- the convention names a location only; the library is not implicitly included
in, or excluded from, any other policy, and no plan artifact, daemon state, checkpoint, or configuration lives there;
- modules are created and edited only through `write` and `edit`, read through
`read`, `glob`, and `grep`, and run through `execute`; the relative-path addressing rules above apply unchanged;
- a tool result or error identifies a module by its logical relative path, with
the same redaction as every other workspace path; and
- `write` and `edit` remain incompatible in Plan mode, so library mutation stays
Build activity.

A raw `PathBuf` alone is not a workspace contract. It must be wrapped in an input DTO with semantic intent and pass the
workspace hook.

WorkspaceRoot is an anchor for addressing, not a security boundary: the daemon and its child processes run with the
user's ordinary OS authority. Real isolation, if it is ever required, must be an OS-level boundary such as a sandbox,
container, or ACL, never a lexical path check.

### Safe missing-path outcome

When M5 implements a file-oriented `not_found` outcome, it uses `ErrorDto` with `ErrorDetailDto::MissingWorkspacePath {
path: WorkspaceRelativePathDto }`. `path` is the logical relative path supplied under the session workspace, such as
`src/missing.rs`. The tool must not disclose the absolute workspace root, a resolved symlink target, an OS error string,
command details, or file content in the error message, detail, or display form.

## Tool pipeline

```mermaid
flowchart LR
  MT[Model tool call] --> IV[Invocation DTO]
  IV --> BI[Before invocation]
  BI --> WR[Workspace resolve]
  WR --> BV[Hook validate]
  BV --> BE[Before execute]
  BE --> EX[Base tool]
  EX --> AE[After execute]
  AE --> PE[Persist result]
  PE --> MC[Model context hook]
  MC --> PB[Publish frame]
  PB --> CE[Continue provider exchange]
```

<!-- The phases map to the typed hook lifecycle. Base tools do primitive work only. -->

Ordinary model requests advertise the model-visible descriptor set as typed tool definitions. The model-tool loop feeds
this pipeline: a provider-emitted tool call becomes a typed invocation built by the application, executes through the
daemon-owned registry, and its durable result is persisted before publication and returned to the provider exchange as a
tool-role message. Provider adapters never execute local tools. The runtime owns the provider continuation until the
provider finishes. Every workspace tool checks the invocation's cancellation signal before and between its I/O steps and
returns its captured output as a partial result when stopped.

### Tooling execution API and status rendering

`intention-tools` exposes exactly one current execution surface: the cancellation-aware bare-result dispatch
(`dispatch_with_cancellation`) and the envelope entry (`invoke_enveloped` / `invoke_enveloped_with_cancellation`) that
returns the result-boundary envelope with observability and execution metadata. There are no compatibility wrappers
without cancellation, and no caller-facing path that bypasses the typed invocation envelope when invocation identity and
durable metadata are required. Every executed program is classified by a typed `ToolProcessStatus` (`success`,
`non_zero` with its numeric code, or `signal` with its recorded signal); the classification is carried on the durable
execution metadata. The execute result's text rendering is derived from that same typed status, so the text and the
typed classification can never disagree and no arbitrary sentinel exit code is invented for signal termination.

## Hook system

`intention-tools` owns typed registration, order, hook context, and dispatcher behavior.

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
- hooks cannot directly commit storage or publish independently;
- hook execution itself is observable with safe metadata.

### M5 ownership and execution order

The composition root registers the workspace and hook services. The application owns the pipeline and durable
lifecycle/result persistence; the dispatcher owns typed ordering and short-circuit outcomes; base tools perform only
primitive work. VFR, Headroom, and Plan owners are not active M5 implementations merely because their hook phases exist.

### Required initial hooks

| Hook owner | Responsibility |
| --- | --- |
| `intention-tools` | Resolve paths from the root anchor, set process CWD, apply the default search scope. |
| `intention-plans` | Enforce Plan-mode artifact directory mutations and hide frontmatter. |
| `intention-vfr` | Transform eligible read output into a virtual representation. |
| `intention-headroom` | Transform eligible tool output before model-context insertion. |

## Policy separation

The following must remain distinct:

| Concern | Owner |
| --- | --- |
| Tool's primitive work | Base tool implementation. |
| Path addressing and CWD | WorkspaceRoot hook. |
| Plan-mode mutation authorization | Plan policy hook. |
| Compression and retrieval metadata | Headroom hook. |
| Virtual source transformation | VFR hook. |
| Persistence transaction | Application/storage. |
| UI rendering | Adapter. |

## Trusted workspace boundary

v1 does not sandbox tools or containerize processes. `WorkspaceRoot` anchors relative addressing, starts `execute` at
the root, and scopes pathless `glob`/`grep`; it does not contain absolute paths, parent paths, or symbolic links, and a
shell command can still interact with the wider user environment. This limitation is explicit in Plan and Build
Autopilot. Plan's advisory instruction is not a technical boundary.

## Required tests and outcomes

| Requirement | Test evidence | Observable outcome |
| --- | --- | --- |
| Relative resolution | Tool contract test with changed process CWD. | A relative path resolves from the declared root, never from process CWD. |
| Addressing anchor | Workspace contract test. | A pathless `glob`/`grep` searches from the root; absolute and parent paths are addressed as given, not contained. |
| Execute CWD | Process fixture test. | Child process observes workspace root as CWD. |
| Hook ordering | Registry unit/property test. | Same registration yields deterministic execution order. |
| Hook rejection | Integration test. | Rejected invocation persists/publishes a typed outcome without base tool execution. |
| Adapter independence | Daemon tool-stream contract test from TUI and bridge clients. | Both see the same committed tool results. |

## Quality-gate integration

Tool, WorkspaceRoot, and hook tests are blocking `make verify` inputs under the coverage policy of [12 Quality Gates and
Makefile](12-quality-gates-and-makefile.md) (per-crate tiers). Architecture checks must reject direct process-CWD
fallback and
VFR/Headroom coupling inside base tools; line coverage cannot replace the explicit relative-addressing, search-scope,
execute-CWD, hook-order, and policy-denial scenarios above.

## Open decisions

- exact capability taxonomy and audit policy for `execute`, network, and
destructive file actions. Build Autopilot does not use per-action confirmation; Plan `execute` is advisory-guided and
trusted-local.

## Autopilot tool boundary

Existing M3/M4 path-handling and confirmation behavior remains historical. The accepted Build Autopilot policy
intentionally removes per-action confirmation for the configured Build surface. Hooks remain typed and mandatory but
cannot add discretionary confirmation or risk authorization. The fixed registry, descriptor revisions, and loop details
are owned by [Tool registry and model-tool loop](15-tool-registry-and-model-tool-loop.md). This does not create a
second registry or bypass path. Plan `execute` remains available under advisory focus guidance and is not a sandbox;
ordinary Plan `write` and `edit` remain denied.

## Slice 1.5 tool contract

Slice 1.5 makes JSON Schema text the tool contract, and the merged `intention-tools` crate owns the registry, the hook
lifecycle, and the workspace anchor that enforce it.

- Descriptor by schema, not by DTO type name. Every active descriptor declares its model-visible argument schema in
  `input_schema` and its result schema in `output_schema` as code-owned JSON Schema text; the old DTO-type-name fields
  and the separate `model_parameters_schema` are gone, and the schema text is the contract the model sees.
- Runtime validation at the registry boundary. `ToolInput::from_arguments_json` resolves the model-visible tool name
  through the registry and decodes the raw argument object into the typed payload before any effect, with
  `unknown_tool` for unregistered or reserved names and `invalid_tool_input_json` for malformed arguments. Results are
  built only through the typed payload constructors, so a shape mismatch never reaches the durable result.
- JSON only here. Tool inputs and outputs are JSON objects. Invocations, results, durable tool evidence, hook contexts,
  and every other contract above stay typed DTOs, and `serde_json::Value` stays prohibited in production code.
- Eight hook phases, fully wired. The phase list, its deterministic ordering, its continue/transform/reject outcomes,
  and the policy separation table are unchanged, and every phase has a dispatch site in the production path with an
  order test: the engine entry before identity validation (`BeforeToolInvocation`), the workspace owner around the
  boundary resolve (`BeforeWorkspaceResolution`, `AfterWorkspaceResolution`), the cancellation-aware dispatch
  (`BeforeToolExecution`, `AfterToolExecution`), the one storage transaction (`BeforeToolResultPersist`), the
  tool-result message build (`BeforeToolResultModelContext`), and publication (`AfterToolResultPublished`). The plan,
  VFR, and Headroom hook owners keep their responsibilities.
