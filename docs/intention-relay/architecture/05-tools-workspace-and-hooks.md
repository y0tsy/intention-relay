# Tools, Workspace, and Hooks

**Current policy.**

This document specifies typed core tools, `WorkspaceRoot` addressing semantics, and tool execution policy.

## Tool ownership

`intention-tools` owns:

- typed tool identity and metadata;
- input and output DTO contracts;
- the six core tool contracts and their model-visible spec match;
- execution interfaces.

Tools are domain/runtime capabilities, not UI commands. An adapter only renders tool results returned by the daemon.

## Core tool contract

M5 activates six executable tools: `read`, `write`, `edit`, `execute`, `glob`, and `grep`. No other tool id exists:
a name that is not one of these six is rejected as unknown before any effect.

Every tool has:

```text
ToolSpec
  id
  description
  input_schema   (code-owned JSON Schema text)

ToolId
  read | write | edit | execute | glob | grep

ToolInput
  read | write | edit | execute | glob | grep  (typed per-tool parameters)

ToolDispatchOutcome
  completed(ToolResult) | interrupted(cause, partial)
```

Every model-visible tool declares `input_schema`: the code-owned JSON Schema text for its typed model parameters.
`intention_tools::model_visible_descriptors()` returns exactly those six tool specs in advertisement order, and
`intention_tools::spec(id)` is the single place where a tool's identity, description, and schema are defined. Ordinary
model requests advertise that set as typed tool definitions
([architecture 08](08-model-protocol-and-providers.md)).

The concrete Rust API can use traits and generic DTOs, but no tool entry point accepts untyped tool inputs or results.

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

A raw `PathBuf` alone is not a workspace contract. It must be wrapped in an input DTO with semantic intent and be bound
to the workspace root before execution.

WorkspaceRoot is an anchor for addressing, not a security boundary: the daemon and its child processes run with the
user's ordinary OS authority. Real isolation, if it is ever required, must be an OS-level boundary such as a sandbox,
container, or ACL, never a lexical path check.

### Safe missing-path outcome

A file-oriented failure outcome uses `ErrorDto` with a stable code and a code-owned safe message: a workspace read that
cannot open or read its target reports `tool_read_failed` with `unable to read workspace file`. The tool must not
disclose the absolute workspace root, a resolved symlink target, an OS error string, command details, or file content,
and the error carries no dynamic path detail payload.

## Tool pipeline

```mermaid
flowchart LR
  MT[Model tool call] --> IV[Typed invocation]
  IV --> TV[Validate tool identity]
  TV --> WC[Commit tool-call row]
  WC --> WR[Bind workspace root]
  WR --> EX[Base tool]
  EX --> TC[Commit terminal result]
  TC --> PB[Publish committed row]
  PB --> CE[Continue provider exchange]
```

<!-- Base tools do primitive work only; the application owns the durable sequence. -->

Ordinary model requests advertise the model-visible spec set as typed tool definitions. The model-tool loop feeds this
pipeline: a provider-emitted tool call becomes a typed invocation built by the application, executes through the
daemon-owned tool service, and its durable result is persisted before publication and returned to the provider exchange
as a tool-role message. Provider adapters never execute local tools. The runtime owns the provider continuation until the
provider finishes. Every workspace tool checks the invocation's cancellation signal before and between its I/O steps and
returns its captured output as a partial result when stopped.

### Tooling execution API and status rendering

`intention-tools` exposes exactly one current execution surface: the cancellation-aware dispatch
`ToolService::dispatch_with_cancellation(call, input, cancellation)` returns a typed `ToolDispatchOutcome`
(`completed(ToolResult)` or `interrupted(cause, partial)`), and the application renders each completed result into the
bounded, redacted text its durable rows carry. There are no compatibility wrappers without cancellation and no second
invocation or result-boundary entry. Every executed program is classified by a typed `ToolProcessStatus` (`success`,
`non_zero` with its numeric code, or `signal` with its recorded signal). The execute result's text rendering is derived
from that same typed status, so the text and the typed classification can never disagree and no arbitrary sentinel exit
code is invented for signal termination.

## Execution order and extension points

The application owns the base tool pipeline as one direct typed sequence: validate the typed input identity, commit the
tool-call row, bind the workspace root, dispatch once through `intention-tools`, commit the terminal result row with its
answering transcript row, and publish the committed row. `intention-tools` owns no registration, ordering, phase,
context, or dispatcher mechanism, and the production path has no phase dispatch site.

WorkspaceRoot binding, VFR transformation, Headroom compression, and Plan-mode authorization are therefore ordinary
calls, not extension points. Each attaches only when a phase has a behavioural owner with a contract to test; the
`intention-plans`, `intention-vfr`, and `intention-headroom` owners are not active implementations today
([architecture 15](15-tool-registry-and-model-tool-loop.md)).

## Policy separation

The following must remain distinct:

| Concern | Owner |
| --- | --- |
| Tool's primitive work | Base tool implementation. |
| Path addressing and CWD | WorkspaceRoot binding in the application. |
| Plan-mode mutation authorization | Plan policy. |
| Compression and retrieval metadata | Headroom. |
| Virtual source transformation | VFR. |
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
| Pre-effect rejection | Application integration test. | An unknown or mismatched tool id is rejected before any effect and leaves no durable row. |
| Adapter independence | Daemon tool-stream contract test from TUI and bridge clients. | Both see the same committed tool results. |

## Quality-gate integration

Tool and WorkspaceRoot tests are blocking `make verify` inputs under the coverage policy of [12 Quality Gates and
Makefile](12-quality-gates-and-makefile.md) (per-crate tiers). Architecture checks must reject direct process-CWD
fallback and
VFR/Headroom coupling inside base tools; line coverage cannot replace the explicit relative-addressing, search-scope,
execute-CWD, and policy-denial scenarios above.

## Open decisions

- exact capability taxonomy and audit policy for `execute`, network, and
destructive file actions. Build Autopilot does not use per-action confirmation; Plan `execute` is advisory-guided and
trusted-local.

## Autopilot tool boundary

Existing M3/M4 path-handling and confirmation behavior remains historical. The accepted Build Autopilot policy
intentionally removes per-action confirmation for the configured Build surface. No phase can add discretionary
confirmation or risk authorization between the identity check and the tool dispatch. The tool contracts, the
model-visible set, and the loop details are owned by
[Tool registry and model-tool loop](15-tool-registry-and-model-tool-loop.md). This does not create a second tool path or
bypass path. Plan `execute` remains available under advisory focus guidance and is not a sandbox;
ordinary Plan `write` and `edit` remain denied.

## Slice 1.5 tool contract

Slice 1.5 makes JSON Schema text the tool contract, and the merged `intention-tools` crate owns the six tool contracts
and the workspace anchor that enforce it.

- Schema text, not DTO type names. Each model-visible tool declares its argument schema in `input_schema` as code-owned
  JSON Schema text, and that text is the contract the model sees.
- Runtime validation before any effect. `ToolInput::from_arguments_json` matches the model-visible tool name and decodes
  the raw argument object into the typed payload before any effect, with `unknown_tool` for any other name and
  `invalid_tool_input_json` for malformed arguments. Results are built only through the typed payload constructors, so a
  shape mismatch never reaches the durable result.
- JSON only here. Tool inputs and outputs are JSON objects. Invocations, results, durable tool evidence, and every other
  contract above stay typed DTOs, and `serde_json::Value` stays prohibited in production code.
- One direct sequence. The application validates the identity, commits the call row, binds the workspace root,
  dispatches once, commits the terminal row, and publishes it, with no phase dispatch between those steps.
