//! The input block: the badge header, one row per buffer line, and the status
//! row under them.
//!
//! The block grows with its buffer, one row per line beside its own chrome; the
//! block and its lines are painted by the kit's input brick, so the component
//! owns no cursor arithmetic and no chrome constants. The badge header names
//! the session's real run mode and the effective provider model its projection
//! carries. Both are projection facts, so a session no snapshot has named yet
//! shows no badge rather than a fabricated one.

import { selectDraft, selectSessionMode, selectSessionModel } from "../app/select";
import type { AppStore } from "../app/store";
import { Badge, Pane, TextField } from "../kit";
import type { Palette } from "../theme/palette";
import { StatusLine } from "./StatusRow";
import { useStore } from "./use-store";

/** Renders one badge chip on its own wash. */
function Badges({
  mode,
  model,
  palette,
}: {
  mode: string | null;
  model: string | null;
  palette: Palette;
}) {
  return (
    <box flexDirection="row" height={1}>
      {mode === null ? null : (
        <Badge label={mode} ink={palette.badgeInk} background={palette.badgeBg} />
      )}
      {mode !== null && model !== null ? <text> </text> : null}
      {model === null ? null : (
        <Badge label={model} ink={palette.badgeInk} background={palette.badgeBg} />
      )}
    </box>
  );
}

/** Renders the input block: its frame, the badges, the buffer, and the status. */
export function InputBlock({
  store,
  palette,
  rows,
}: {
  store: AppStore;
  palette: Palette;
  rows: number;
}) {
  const draft = useStore(store, selectDraft);
  const mode = useStore(store, selectSessionMode);
  const model = useStore(store, selectSessionModel);
  return (
    <Pane
      height={rows}
      borderColor={palette.accent}
      background={palette.panel}
      paddingLeft={1}
      paddingRight={1}
    >
      <Badges mode={mode} model={model} palette={palette} />
      <TextField text={draft.text} caret={draft.caret} rows={rows} palette={palette} />
      <box height={1}>
        <StatusLine store={store} palette={palette} />
      </box>
    </Pane>
  );
}
