# Backend command effects evidence

`tauri-app/src/lib.rs:specta_builder` registers session, agent, model, config, file, sub-agent, MCP, observability, plan, project and confirmation commands. The frontend gateway exposes a typed subset as its active IPC boundary.

Linked command effects include SQLite-backed session/project operations, AgentLoop creation/execution/cancellation, persisted config replacement, provider/model changes, plan continuation, question/confirmation responses, sub-agent orchestration, logs, usage and status queries.

Registered commands without a visible frontend route are not product capabilities and are listed in `05-excluded-or-unproven.md` when relevant.
