# Current user capability catalogue

## Scope

This catalogue describes what a user can access through the active desktop frontend at audited revision `8604fde0566d4dfadf8124e0724c5a82df3b89de`. Each included item has a static route from visible UI to backend effect. It does not claim runtime verification. See [`03-evidence-registry.json`](03-evidence-registry.json) for compact source chains and [`05-excluded-or-unproven.md`](05-excluded-or-unproven.md) for exclusions.

## 1. Run a coding agent in a project-backed conversation

**Entry point:** the message input, Send, Stop and Retry controls in the chat area.

**User action and outcome:** A user writes a request, starts a new session automatically if none is active, and receives a streaming agent response. The UI appends the user message immediately, renders assistant text and thinking/tool updates as they arrive, presents tool activity inline, and displays errors as system banners. While streaming, an additional submitted message is sent to the backend queue rather than starting another run. Stop requests cancellation for the active session. Retry resends the latest user message.

**Static path:** `InputArea.svelte → App.svelte:hSend/hStop/hRetry → chat-service.ts → gateway.ts → send_message/stop_agent/queue_message in commands.rs → AgentLoop`.

**Relevant details:** The chat is a model-driven tool-using loop, not a single completion. Tool calls and results are shown in the message stream. Markdown is parsed, sanitized and rendered as structured text/code/diff blocks. Streaming is active, but stable backend assistant-turn identity and late-event handling are known limitations, so concurrent/late delivery correctness is not claimed.

**Status:** Proved as a static route, with stream-identity limitations.

## 2. Create, restore, navigate and review sessions

**Entry point:** tab bar, session history panel, project switching, and session creation controls.

**User action and outcome:** A user can create a session, open a historical session in a tab, switch active tabs, review session history and restore prior conversation data. Switching saves/restores local chat view state and refreshes usage, todos, project linkage and session data. The backend stores sessions/messages in SQLite and restores a session on demand.

**Static path:** `TabBar/HistoryPanel → App handlers → session-service.ts/session-store.svelte.ts → gateway.ts → create_session/list_sessions/get_session/delete_session → SQLite storage and SessionManager`.

**Relevant details:** Sessions carry a mode, model and optional project directory. Open tabs are frontend-managed. Closing is user-visible but is classified as partial until its complete persistence path is verified independently.

**Status:** Proved for creation, opening, switching, listing and restoration. Closing is partial.

## 3. Work with projects and their contextual files

**Entry point:** project selection from tabs/history, native directory dialogs, sidebar project status and Relink.

**User action and outcome:** A user can choose a directory as a project, switch to a known project, create a session for it, see its active directory and see whether `AGENTS.md` and `.antibusy/memory.md` are present. If a session's project directory is missing, the user can choose a replacement directory with Relink. Dropping files into the chat sends their paths to the backend and appends returned content to the draft.

**Static path:** `TabBar/HistoryPanel/Sidebar/InputArea → App → sessionService, projectService, projectStore, inputService → gateway → project/relink/file-check/drop commands`.

**Relevant details:** Project selection updates the context used when building newly created agents. The visible status is file presence, not proof that every file has been loaded into an already running agent.

**Status:** Proved.

## 4. Select operating mode, model and reasoning effort

**Entry point:** Build/Plan buttons, model selection popup, provider/endpoint selection inside that popup, and reasoning-effort control in the sidebar.

**User action and outcome:** A user can toggle between Build and Plan, select a model, choose an endpoint/provider, and change reasoning effort for the active session. Model/provider/reasoning changes update frontend selection optimistically and roll back on a reported backend error, where implemented.

**Static path:** `Sidebar/ModelDetailPopup/ReasoningEffort → App.svelte → model-store.svelte.ts → gateway.ts → switch_mode/switch_model/select_provider/switch_reasoning_effort`.

**Relevant details:** Build and Plan maintain separate frontend selections loaded from config. Mode switching performs a non-awaited backend request, so its reconciliation/failure handling is incomplete. The model popup's selection callback currently passes model identity through the connected model-store route; endpoint/provider details should be verified as a separate UI behavior during runtime testing.

**Status:** Model and reasoning selection proved. Mode switching is partial.

## 5. Configure the application and personalize the desktop UI

**Entry point:** Configuration panel, schema form/raw TOML editor, theme control, language control.

**User action and outcome:** A user can open the Configuration panel, load configuration/schema/TOML, edit configuration, save JSON/TOML-derived updates, switch theme, and toggle English/Russian locale. The configuration backend validates and persists configuration and emits a config-changed event.

**Static path:** `Sidebar → TogglePanel → ConfigEditor → configService → gateway → get_config/get_config_schema_flattened/get_config_toml/update_config/update_config_from_toml`; appearance routes through `shellStore` and Paraglide.

**Relevant details:** The configuration surface exposes advanced agent settings, including prompt sources, context compression, VFR, custom tools, hooks, skills, MCP definitions and sub-agent policy. Persisting configuration does not establish live reload for all of those mechanisms. Many are constructed at application or agent creation, therefore they should currently be understood as startup/restart configuration.

**Status:** Proved for editor access and persistence path. Live application of individual advanced settings is limited.

## 6. Review, approve or reject agent plans

**Entry point:** plan overlay shown after a backend `PlanSubmittedEvent`.

**User action and outcome:** When the agent submits a plan, the UI displays a plan approval overlay. A user can approve it, optionally add a comment, or reject it with feedback. Both paths create a streaming channel and continue the agent interaction with the backend's plan commands.

**Static path:** `PlanSubmittedEvent → event-listeners.ts → App/planService/overlay store → OverlayHost/PlanApproval → chat-service.ts → gateway.ts → confirm_plan or reject_plan`.

**Relevant details:** Plan workflow has a frontend finite-state reducer and a dedicated service. Session association and stale-event safeguards remain architectural gaps, so the user-visible capability is catalogued without claiming all race conditions are solved.

**Status:** Proved static path.

## 7. Answer agent questions and respond to confirmations

**Entry point:** Ask User modal and confirmation overlay.

**User action and outcome:** The agent can cause the desktop UI to display a question with options. The user selects an answer, which is passed to the backend waiting response channel. Confirmation requests similarly show action/risk/message context and send allow, deny, or always-allow responses.

**Static path:** `AskUserEvent or ConfirmationRequest event → event-listeners.ts → overlay state → OverlayHost → AskUserModal/ConfirmationBanner → confirmationService → gateway.ts → answer_ask_user/confirm_action`.

**Relevant details:** Ask-user support is an interactive agent capability, not merely a static prompt instruction. Confirmation lifecycle details, including dismissal, timeout, request queueing and duplicate policy, are incomplete.

**Status:** Ask-user path proved. Confirmation action route proved, lifecycle partial.

## 8. Observe work, usage, tasks, logs and sub-agents

**Entry point:** status bar, inline tool-call UI, todos, Agents and Logs panels, toast/error UI.

**User action and outcome:** The user can see streaming tool calls/results in chat, session usage statistics, agent streaming/error state, todo items, log output, and sub-agent statuses. The Agents panel offers cancellation of a running sub-agent. The Logs panel loads buffered logs and opens a log channel. MCP status is visible through a sidebar widget.

**Static path:** `StatusBar/ChatArea/TogglePanel/McpStatusWidget → stores/services → gateway and event listeners → get_usage_stats/get_todos/get_log_buffer/open_log_stream/get_sub_agents/cancel_sub_agent/get_mcp_status plus Tauri events`.

**Relevant details:** A sub-agent is normally initiated by the main model while handling a chat request, rather than directly created by a user button. The UI exposes observation and cancellation. MCP is status-only from the inspected frontend.

**Status:** Proved static routes.

## 9. Agent behavior configured through the UI

The following mechanisms are not separate UI products. They shape the behavior of an agent started from the chat after configuration is persisted and relevant application/agent initialization occurs. Full details and exact source chains are in [`evidence/12-agent-runtime-behavior.md`](evidence/12-agent-runtime-behavior.md).

### Context compression and recovery, Headroom/CCR

The configuration editor exposes Headroom settings. For newly constructed agents, eligible tool outputs can be compressed before they are added to the model context and shown in the tool stream. CCR markers can be retrieved by the agent with its registered `retrieve` tool. Defaults include enabled compression, in-memory CCR, capacity 1000, TTL 1800 seconds, and a 500-character minimum input. Saving this configuration does not rebuild the already-created pipeline.

### Virtual File Representation, VFR

The configuration editor exposes VFR. A newly constructed agent can receive shortened structured reads of qualifying large Rust, Python, TypeScript, JavaScript and Go files, with placeholders for hidden bodies/import/test sections. Its prompt tells it to expand a placeholder or request a raw read. Defaults include enabled VFR, a 200-line file threshold and a five-line body threshold. This behavior affects what tool output appears in chat and reaches the model.

### Prompt context

New agents assemble a system prompt from project context, date, mode, Git context, conventions and optionally `AGENTS.md`, `.antibusy/memory.md`, few-shot examples and MCP prompts. The configuration editor exposes the relevant prompt controls. The combined prompt is capped at 100,000 characters.

### Custom tools, hooks and skills

A user can persist definitions through the general configuration surface. After the relevant startup initialization, custom shell tools become model-callable and their calls/results appear in chat; hooks run around session/tool lifecycle points but do not have dedicated chat output; active skills contribute prompt text and filter tools. The current UI has no direct live management interface and saving does not prove activation in the existing process.

### Sub-agent orchestration

The main agent can use its registered spawn-sub-agent tool during a user chat run. The UI then shows status, elapsed time, depth/mode and cancellation. The configured default policy sets a maximum of 10 concurrent agents, 300-second timeout and nesting depth of 5. Direct manual spawning is not exposed.

## Non-capabilities deliberately excluded

Direct MCP server administration, skill discovery/toggling, direct custom-tool/hook management, VFR LSP behavior, a working Files panel, direct manual sub-agent spawning, and project management actions without visible controls are excluded. See [`05-excluded-or-unproven.md`](05-excluded-or-unproven.md).
