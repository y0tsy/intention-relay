//! The theme picker.
//!
//! Three entries, one highlighted, and the choice is written back to the
//! configuration file. Like the browser it owns the whole panel while it is
//! open, so the surface a key reaches is always the surface on screen.

import { TextAttributes } from "@opentui/core";
import { THEME_PREFERENCES, type ThemePreference } from "../app/action";
import type { Palette } from "../theme/palette";
import { renderHints } from "./format";

/** The keys the picker offers. */
export const PICKER_HINTS = [
  { keys: "↑↓", action: "move" },
  { keys: "enter", action: "apply" },
  { keys: "esc", action: "close" },
];

/** One entry the picker explains. */
interface Entry {
  readonly preference: ThemePreference;
  readonly label: string;
  readonly detail: string;
}

/** What the picker says about each preference. */
const DETAILS: Record<ThemePreference, string> = {
  auto: "follow the terminal background",
  dark: "for a dark terminal",
  light: "for a light terminal",
};

/**
 * Every entry, in the order the reducer indexes them.
 *
 * The order is the reducer's, never the view's: the highlight is an index into
 * `THEME_PREFERENCES`, so a view that listed the entries differently would
 * commit the preference next to the one the reader picked.
 */
export const THEME_ENTRIES: readonly Entry[] = THEME_PREFERENCES.map((preference) => ({
  preference,
  label: preference,
  detail: DETAILS[preference],
}));

/** Renders the picker. */
export function ThemePicker({
  selection,
  current,
  palette,
  width,
  height,
}: {
  selection: number;
  current: ThemePreference;
  palette: Palette;
  width: number;
  height: number;
}) {
  const roomy = height >= 10 && width >= 40;
  return (
    <box flexDirection="column" flexGrow={1} paddingLeft={1} paddingRight={1}>
      <text fg={palette.ink} attributes={TextAttributes.BOLD}>
        theme
      </text>
      {/* Three entries in a tall panel read as a broken list when they hang off
          the title, so a roomy panel centres the block and a tight one keeps it
          at the top where the reader looks first. */}
      <box
        flexDirection="column"
        flexGrow={1}
        justifyContent={height >= 16 ? "center" : "flex-start"}
      >
        {THEME_ENTRIES.map((entry, index) => {
          const selected = index === selection;
          const inForce = entry.preference === current;
          return (
            <box key={entry.preference} flexDirection="row" gap={1}>
              <text fg={selected ? palette.accent : palette.inkFaint}>{selected ? "›" : " "}</text>
              <text
                fg={selected ? palette.ink : palette.inkMuted}
                attributes={selected ? TextAttributes.BOLD : undefined}
              >
                {entry.label}
              </text>
              {inForce ? <text fg={palette.success}>current</text> : null}
              {roomy ? (
                <text wrapMode="word" fg={palette.inkFaint}>
                  {entry.detail}
                </text>
              ) : null}
            </box>
          );
        })}
      </box>
      <text wrapMode="none" fg={palette.inkFaint}>
        {renderHints(width - 2, PICKER_HINTS)}
      </text>
    </box>
  );
}
