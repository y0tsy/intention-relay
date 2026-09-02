# Chat and streaming evidence

## Proved route

`InputArea → App.svelte:hSend → chatService.sendMessage → gateway.sendMessage → commands.sendMessage → AgentLoop.run → Tauri Channel chunks → chatService stream reducer → ChatArea/ChatMessage/ToolCallInline`.

The same route supports automatic session creation. `hStop → chatService.stopAgent → gateway.stopAgent → stop_agent`; retry sends the latest user message through the same send route. While streaming, `chatService` sends further input to `queue_message`.

## Visible result

User/assistant messages, thinking/tool chunks, todo updates, markdown/code/diff presentation and error banners are updated through the active chat store.

## Limits

The static path is active, but it does not prove ordering, late-chunk safety or stable server-defined assistant-turn identity.
