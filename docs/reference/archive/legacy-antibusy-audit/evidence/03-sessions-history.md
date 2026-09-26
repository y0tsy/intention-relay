# Sessions and history evidence

## Proved routes

- `TabBar/HistoryPanel → App → sessionService.createSession/openInTab/switchTab → sessionStore/gateway → create_session/get_session/list_sessions`.
- `sessionService` saves/restores local chat view state, refreshes usage/todos and resolves the session project after opening or switching.
- Rust `create_session`, `get_session` and `list_sessions` route through storage and `SessionManager`.

## Visible result

Users can create sessions, open historical sessions in tabs, switch active conversations and view history.

## Limit

`closeTab` is visible but its full persistent delete/lifecycle route was not independently demonstrated in this source audit, so it is partial.
