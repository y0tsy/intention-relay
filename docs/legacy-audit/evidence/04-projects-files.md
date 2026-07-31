# Projects and files evidence

## Proved routes

- `TabBar/HistoryPanel → sessionService.switchToProject/createOrOpenProject → projectStore/gateway → touch_project/get_project/create_or_open_project → session creation`.
- `Sidebar Relink → projectService.relinkProject → native directory dialog → gateway.relinkProject → relink_project`.
- `Sidebar project status → projectStore.checkProjectFiles → gateway.checkProjectFiles → check_project_files`.
- `InputArea drop → inputService.ingestDroppedFiles → gateway.ingestDroppedFiles → ingest_dropped_files`.

## Visible result

The user can choose/relink project directories, start sessions for them, see AGENTS.md and memory-file presence, and append dropped-file content to a draft.

## Exclusion

No active generic Files panel was found in `TogglePanel.svelte`.
