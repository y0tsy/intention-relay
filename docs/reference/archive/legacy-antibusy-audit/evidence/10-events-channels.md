# Events and channels evidence

`App.svelte` installs `initEventListeners(eventHandlers)` through the shell. `event-listeners.ts` subscribes to agent start/done/error, confirmation, session update, sub-agent update/result, configuration, ask-user, plan-submitted and project-deleted events. App handlers update feature stores, overlays and refresh work.

Chat and plan continuations use `createStreamChannel`; log output uses `createLogChannel`. Rust `send_message`, plan commands and log stream forward backend values through Tauri Channels.

The routes are statically active. Event ownership remains distributed in App handlers and some callbacks are no-ops, therefore this evidence proves reachability rather than complete lifecycle semantics.
