# Agent behavior visible through the current frontend

This document is the reader-oriented counterpart of [`evidence/12-agent-runtime-behavior.md`](evidence/12-agent-runtime-behavior.md). It does not elevate backend mechanisms into standalone product features. Each item below is included because the user can reach it through the Configuration panel plus a project-backed chat session.

## Configuration lifecycle

The Configuration panel can persist agent settings. The static route is:

```text
ConfigEditor → configService → gateway → update_config/update_config_from_toml
→ persisted AppState configuration → later application/agent initialization
→ Chat input → AgentLoop
```

Several mechanisms are assembled at application or agent creation. Therefore Save is evidence of configuration persistence, but not evidence that every advanced setting takes effect immediately for an already running agent.

## Headroom and CCR

Headroom can compress eligible tool output before it is retained in agent context and presented through the tool-result stream. A compressed result can contain CCR retrieval markers; the agent has a `retrieve` tool for recovering retained originals. The configuration defaults to enabled Headroom, memory-backed CCR, capacity 1000, 1800-second TTL, 500-character minimum input and disabled ML detection.

User meaning: large tool output may be shortened, while the agent can ask to recover stored details. This is behavior of a configured chat run, not a separately operated UI panel.

## VFR

VFR can transform a qualifying large code-file read into a virtual representation with placeholders for selected bodies, imports or tests. The agent receives instructions to request an expansion or a raw read when needed. Default support covers Rust, Python, TypeScript, JavaScript and Go, with a 200-line file threshold and a five-line body threshold.

User meaning: chat-visible file-tool results may be intentionally concise instead of showing full large-file source. No evidence supports treating the configured LSP fields as a working visible feature.

## System prompt context

When a new agent is created for a session, it assembles context from the project directory, mode, date, Git information, conventions and optionally project `AGENTS.md`, `.antibusy/memory.md`, few-shot examples and MCP prompts. The prompt is capped at 100,000 characters.

User meaning: choosing a project and starting a session can influence the agent without the user having to paste project instructions into every message. The sidebar exposes presence indicators for `AGENTS.md` and memory, but presence is not a live read confirmation for an existing agent.

## Custom tools, hooks and skills

The generic configuration surface can persist custom tools, hooks and active skill settings. After startup initialization:

- custom shell tools can become callable by the model and appear as streamed tool activity;
- hooks can run around sessions and tool usage, affecting local work but without dedicated chat output;
- skills can add prompt guidance and constrain model-visible tools.

There is no direct frontend interface to browse, activate, or live-reload these facilities. They are advanced startup-oriented behavior.

## Sub-agents

During a user chat run, the main model can call its sub-agent tool. The frontend receives status/result events, displays active sub-agents, and gives the user a cancellation control. Default configuration permits up to 10 concurrent sub-agents, a 300-second timeout and nesting depth 5.

User meaning: sub-agents are supporting workers initiated by the agent, observed and cancellable by the user. Direct manual spawning is not exposed.

## General execution limits

Configuration also supplies loop/output limits that affect a chat run: default LLM timeout is 120 seconds, maximum output tokens 4096, maximum iterations 1000, and persisted tool-result history is capped at 50,000 characters. These are configuration-defined behavior constraints, not independently surfaced controls.

## Evidence

Exact static chains, sources and exclusions: [`evidence/12-agent-runtime-behavior.md`](evidence/12-agent-runtime-behavior.md).
