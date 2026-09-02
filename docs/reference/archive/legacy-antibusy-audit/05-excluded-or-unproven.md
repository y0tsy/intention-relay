# Excluded or unproven paths

This register is intentionally separate from the product catalogue. A feature is not current user-facing capability merely because it has a Rust command, a TOML field, tests, or a widget.

## Frontend-inaccessible

- **Manual sub-agent spawning:** `spawn_sub_agent` is registered on the Tauri surface, but the inspected frontend exposes only agent-initiated spawning indirectly through a chat turn, status, and cancellation. There is no direct user control to create one.
- **MCP server administration:** the UI displays MCP status through `getMcpStatus`; no proven UI path creates, edits, connects, disconnects, or refreshes servers.
- **Skills management:** active skills can be represented in configuration and loaded at application initialization, but no direct UI exists to discover or toggle skills.
- **Hook management and custom tool management:** generic configuration can persist them, but Save does not rebuild the current registry/agents. They are startup-configured behavior, not a live UI management feature.
- **VFR LSP validation:** LSP fields exist in config but no route to an active LSP client was evidenced.
- **Raw file browsing:** `list_files` and `read_file` commands exist, but this audited composition root exposes no active `files` panel body in `TogglePanel.svelte`.
- **Project rename, pin, archive, delete:** commands and store methods exist, but the inspected active UI path does not present their controls.

## Incomplete or limited routes

- **Mode switch:** Build/Plan controls use the same toggle handler and invoke `gateway.switchMode` without awaiting or visible rollback (`model-store.svelte.ts`). It is included as a user action, flagged partial.
- **Session close:** the visible route calls `sessionService.closeTab`, whose complete persistent deletion behavior was not independently traced in this audit.
- **Permission confirmation lifecycle:** distinct actions exist, but queueing, dismissal, timeout and duplicate-request semantics remain incomplete.
- **Stream identity:** frontend stream handling is active, but the current plan identifies local turn identity and late/out-of-session reconciliation gaps. The catalogue describes visible streaming, not strong concurrent-turn correctness.
- **Configuration live reload:** config save replaces stored config, but several behavior-changing facilities are built at application/agent creation. Saving does not prove immediate replacement of active compression, tools, hooks, skills, or prompt state.

## Audit rule applied

These routes are excluded from the main catalogue or called out as limitations so that a rewrite does not mistake backend potential for a shipped user capability.
