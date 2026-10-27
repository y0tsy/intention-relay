//! The launch composer: one prompt row inside the accent frame.
//!
//! The welcome has no session yet, so the composer carries no mode badge and no
//! status line: the header above it names the connection, and the detail line
//! below it names the keys. The box is the input's own frame and one buffer row,
//! and it grows one row per line break the same way every other input does.

import { selectDraft } from "../app/select";
import type { AppStore } from "../app/store";
import { COMPOSER_CHROME_ROWS, Pane, TextField, useContentColumns } from "../kit";
import type { Palette } from "../theme/palette";
import { useStore } from "./use-store";

/** Renders the launch composer: the prompt, the caret, and its placeholder. */
export function WelcomeComposer({ store, palette }: { store: AppStore; palette: Palette }) {
  const draft = useStore(store, selectDraft);
  const columns = useContentColumns();
  return (
    <Pane fill borderColor={palette.accent} background={palette.inputBg} fallbackBare>
      <TextField
        text={draft.text}
        caret={draft.caret}
        prompt="› "
        placeholder=" Ask anything…"
        placeholderInk={palette.inkMuted}
        trailing={columns >= 50 ? "enter send" : undefined}
        trailingInk={palette.inkFaint}
        chromeRows={COMPOSER_CHROME_ROWS}
        palette={{
          accent: palette.accent,
          inputBg: palette.inputBg,
          ink: palette.ink,
        }}
      />
    </Pane>
  );
}
