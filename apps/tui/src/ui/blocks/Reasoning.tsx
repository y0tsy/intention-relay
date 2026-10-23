//! A reasoning block: what the model thought before it answered.
//!
//! The block opens with its own header, shows the first fifteen display rows of
//! the reasoning, and closes with the accent bar that marks the end of the
//! segment. A longer block bounds itself: the last row it paints is the expand
//! affordance, which names how many rows stay hidden and is what a click or
//! `Ctrl+E` reveals one chunk at a time. The rows the affordance counts are the
//! rows this block measured, so the number it prints is the number it hid.

import type { Palette } from "../../theme/palette";
import { markerText, REASONING_EXPAND, REASONING_GLYPH, REASONING_LABEL } from "../glyphs";
import { takeRows, uniqueKeys } from "./rows";

/** The display rows a reasoning block shows before it collapses. */
export const REASONING_VISIBLE_ROWS = 15;

/** Renders one reasoning block at one width. */
export function Reasoning({
  text,
  palette,
  width,
  columns,
  expansion,
}: {
  text: string;
  palette: Palette;
  width: number;
  /** The columns the body has: the block's width less its own indent. */
  columns: number;
  /** How many extra chunks of rows the reader revealed. */
  expansion: number;
}) {
  const { shown, hidden } = takeRows(text, columns, REASONING_VISIBLE_ROWS + expansion);
  return (
    <box flexDirection="column">
      <text fg={palette.reasoningHeader}>{markerText(REASONING_GLYPH, REASONING_LABEL)}</text>
      <box flexDirection="column" paddingLeft={2} backgroundColor={palette.panel}>
        {shown.map((row, index) => (
          <text key={uniqueKeys(shown)[index]} wrapMode="none" fg={palette.reasoningBody}>
            {row}
          </text>
        ))}
      </box>
      {hidden > 0 ? (
        <text wrapMode="none" fg={palette.reasoningMarker}>
          {`${markerText(REASONING_EXPAND, `… ${hidden} more reasoning lines hidden — click or Ctrl+E to reveal more`)}`}
        </text>
      ) : null}
      <text wrapMode="none" fg={palette.accent}>
        {"─".repeat(Math.max(1, width))}
      </text>
    </box>
  );
}
