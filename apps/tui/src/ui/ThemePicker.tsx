//! The theme picker: the panel `/theme` opens above the chat's input block.
//!
//! The picker spends the hint region exactly as the command band does, so the
//! input block and the detail line never move while it is open. The kernel's
//! `Picker` owns the frame, the markers, the selected wash, and the legend; this
//! module names the preferences and their one-line details.

import { THEME_PREFERENCES, type ThemePreference } from "../app/action";
import { THEME_DETAILS } from "../app/menu";
import { Picker, pickerRows } from "../kit";
import type { Palette } from "../theme/palette";
import { CURSOR_MARKER, ROW_MARKER } from "./glyphs";

/** The rows the picker panel occupies: its frame, one row per preference, its legend. */
export const PICKER_ROWS = pickerRows(THEME_PREFERENCES.length);

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
    <Picker
      items={THEME_PREFERENCES.map((preference) => ({
        key: preference,
        label: preference,
        detail: THEME_DETAILS[preference],
        current: preference === current,
      }))}
      selected={selection}
      title="Theme"
      legend="enter apply · esc cancel"
      marker={CURSOR_MARKER}
      rowMarker={ROW_MARKER}
      palette={palette}
    />
  );
}
