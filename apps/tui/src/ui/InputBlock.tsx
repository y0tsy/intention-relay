//! The input block: the badge header, one row per buffer line, and the status
//! row under them.
//!
//! The block grows with its buffer, one row per line beside its own chrome; the
//! block and its lines are painted by the kit's input brick, so the component
//! owns no cursor arithmetic and no chrome constants. The badge header names
//! the session's real run mode and the effective provider model its projection
//! carries. Both are projection facts, so a session no snapshot has named yet
//! shows the composer's own label rather than a fabricated mode.

import { selectDraft, selectSessionMode, selectSessionModel } from "../app/select";
import type { AppStore } from "../app/store";
import { Badge, Line, Pane, TextField } from "../kit";
import type { Palette } from "../theme/palette";
import { StatusLine } from "./StatusRow";
import { useStore } from "./use-store";

/** The label a composer with no projected mode or model carries. */
const PROMPT_LABEL = "prompt";

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
    <Line>
      <Badge
        label={mode ?? PROMPT_LABEL}
        ink={mode === null ? palette.inkMuted : palette.badgeInk}
        background={palette.badgeBg}
      />
      {model === null ? null : <text> </text>}
      {model === null ? null : (
        <Badge label={model} ink={palette.badgeInk} background={palette.badgeBg} />
      )}
    </Line>
  );
}

/** Renders the input block: its frame, the badges, the buffer, and the status. */
export function InputBlock({ store, palette }: { store: AppStore; palette: Palette }) {
  const draft = useStore(store, selectDraft);
  const mode = useStore(store, selectSessionMode);
  const model = useStore(store, selectSessionModel);
  return (
    <Pane fill borderColor={palette.accent} background={palette.panel}>
      <Badges mode={mode} model={model} palette={palette} />
      <TextField text={draft.text} caret={draft.caret} palette={palette} />
      <Line>
        <StatusLine store={store} palette={palette} />
      </Line>
    </Pane>
  );
}
