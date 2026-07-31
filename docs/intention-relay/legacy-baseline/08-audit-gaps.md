# Audit gaps and limits

## Evidence limitation

This is a static source audit. It proves that the inspected frontend has an active-looking path to registered IPC/events and a code-defined backend effect. It does not prove that the desktop application starts, the UI mounts every control, provider credentials work, a model chooses a tool, a command succeeds on a real filesystem, or a streamed event arrives in a specific order.

## Required follow-up before rewrite acceptance

1. Run manual smoke flows for chat, plan approval, session restore, project directory selection, configuration save/restart, model switch, ask-user, confirmation, log stream, and cancellation.
2. Capture an IPC/event trace for every catalogue capability, including failure paths.
3. Determine which configuration behavior must become live-reloadable versus restart-only.
4. Decide whether Headroom/CCR, VFR, prompt assembly, skills and MCP are v1 product commitments or advanced configuration.
5. Reproduce concurrent or late stream events and define stable assistant-turn IDs before carrying streaming forward.
6. Decide an explicit product policy for permissions, plan approval, queued input, sub-agent visibility, and cancellation.

## Source inconsistencies that affect interpretation

- Several models/configuration fallbacks and raw JSON casts are frontend implementation details, not user-facing semantics.
- Existing `PLAN.md` records architectural gaps. This audit does not use those claims as proof unless a source chain also exists.

## Boundary of the naming brief

The naming brief derives from proved interaction and observable behavior. It deliberately does not imply a promise of fully autonomous execution, perfect streaming reliability, live configuration application, or direct MCP administration.
