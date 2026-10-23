//! The prompt line: the draft, its caret, and the placeholder that fills it.
//!
//! The line subscribes to the draft itself, so a keystroke repaints the composer
//! and never the transcript it sits under.

import { selectDraft } from "../app/select";
import type { AppStore } from "../app/store";
import { renderDraft } from "../app/editor";
import type { Palette } from "../theme/palette";
import { useStore } from "./use-store";

/** Renders the prompt line. */
export function Composer({
  store,
  palette,
  focused,
}: {
  store: AppStore;
  palette: Palette;
  focused: boolean;
}) {
  const draft = useStore(store, selectDraft);
  const empty = draft.text.length === 0;
  return (
    <box flexDirection="row" paddingLeft={1} paddingRight={1}>
      <text fg={focused ? palette.accent : palette.muted}>{"> "}</text>
      {empty ? (
        <text wrapMode="word" fg={palette.faint}>
          {focused ? "▏type a prompt" : "type a prompt"}
        </text>
      ) : (
        <text wrapMode="word" fg={palette.foreground}>
          {renderDraft(draft)}
        </text>
      )}
    </box>
  );
}
