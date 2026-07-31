# Observability, agents and todos evidence

## Proved routes

- `StatusBar/session effects → usageStore → gateway.getUsageStats → get_usage_stats`.
- `Chat stream/session effects → chatStore → gateway.getTodos → get_todos`.
- `Agents panel → agentService.getSubAgents/cancelSubAgent → gateway → get_sub_agents/cancel_sub_agent`.
- `Logs panel → logService → gateway.getLogBuffer/openLogStream → log commands and Channel`.
- `McpStatusWidget → mcpService.getMcpStatus → gateway → get_mcp_status`.

## Visible result

The frontend exposes usage, todos, errors/toasts, sub-agent state and cancellation, logs, and MCP connection status.

## Scope boundary

MCP administration is not exposed, only status. Sub-agent creation is model-mediated through chat, while direct user controls are status and cancellation.
