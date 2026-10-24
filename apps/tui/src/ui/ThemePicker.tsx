//! The theme picker: the panel `/theme` opens above the chat's input block.
//!
//! The picker is one more row region of the one chat panel, directly above the
//! input block, exactly like the command hint band: its rows come out of the
//! transcript and no other pane gives any up, so the input block and the detail
//! line never move while it is open. The frame carries the `Theme` title, one
//! row per preference carries the preference's name and its one-line
//! description, and the last row carries the `enter apply | esc revert` legend.
//! The first row is `Auto`, the request to follow the terminal's own
//! background, so returning to detection never needs a hand-edited file.

import { TextAttributes } from "@opentui/core";
import { THEME_PREFERENCES, type ThemePreference } from "../app/action";
import { THEME_DETAILS } from "../app/menu";
import { Pane, pickerSlotRows } from "../kit";
import type { Palette } from "../theme/palette";
import { CURSOR_MARKER, ROW_MARKER } from "./glyphs";

/** The rows the picker panel occupies: its frame, one row per preference, its legend. */
export const PICKER_ROWS = pickerSlotRows(1 + THEME_PREFERENCES.length);

/** Renders the picker panel in the rows the panel gave it. */
export function ThemePicker({
  selection,
  current,
  palette,
}: {
  selection: number;
  current: ThemePreference;
  palette: Palette;
}) {
  return (
    <Pane
      title="Theme"
      titleColor={palette.accentDeep}
      borderColor={palette.accent}
      background={palette.headerBg}
      paddingLeft={1}
      paddingRight={1}
      height={PICKER_ROWS}
    >
      {THEME_PREFERENCES.map((preference, index) => {
        const highlighted = index === selection;
        return (
          <text
            key={preference}
            wrapMode="none"
            bg={highlighted ? palette.rowSelectedBg : palette.headerBg}
          >
            <span fg={palette.accent} attributes={highlighted ? TextAttributes.BOLD : undefined}>
              {highlighted ? CURSOR_MARKER : ROW_MARKER}
            </span>
            <span fg={palette.ink} attributes={highlighted ? TextAttributes.BOLD : undefined}>
              {preference}
            </span>
            <span fg={palette.inkMuted}>{`  ${THEME_DETAILS[preference]}`}</span>
            {preference === current ? <span fg={palette.success}> current</span> : null}
          </text>
        );
      })}
      <text wrapMode="none" fg={palette.inkFaint} bg={palette.headerBg}>
        {"enter apply | esc revert"}
      </text>
    </Pane>
  );
}
