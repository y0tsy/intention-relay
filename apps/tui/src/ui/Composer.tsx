//! The prompt line: the draft, its caret, and the placeholder that fills it.

import type { Palette } from "../theme/palette";
import { type Draft, renderDraft } from "./editor";

/** Renders one composer line. */
export function Composer({
  draft,
  palette,
  focused,
}: {
  draft: Draft;
  palette: Palette;
  focused: boolean;
}) {
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
