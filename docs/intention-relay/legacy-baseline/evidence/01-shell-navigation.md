# Shell and navigation evidence

## Proved routes

- Sidebar opens Configuration, Agents and Logs panels through `App.svelte` panel handlers and `shellStore`; `TogglePanel.svelte` renders `ConfigEditor`, `SubAgentsPanel` and `LogPanel`.
- Sidebar history opens the History panel through `shellStore.setOpenPanel('history')`; `TogglePanel.svelte` renders `HistoryPanel`.
- Theme and locale buttons call App shell handlers and update local shell/Paraglide state.
- The tab shell passes session/project intents to `sessionService`.

## Chain

`Sidebar/TabBar → App.svelte handlers → shellStore/sessionService → relevant store/service → visible mounted panel or updated application state`.

## Classification

Navigation/personalization: **proved**.
