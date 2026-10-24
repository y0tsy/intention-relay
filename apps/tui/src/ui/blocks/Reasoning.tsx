//! A reasoning block: what the model thought before it answered.
//!
//! The block opens with its own header, shows the first fifteen display rows of
//! the reasoning, and closes with the accent bar that marks the end of the
//! segment. A longer block bounds itself: the last row it paints is the expand
//! affordance, which names how many rows stay hidden and is what a click or
//! `Ctrl+E` reveals one chunk at a time. The rows the affordance counts are the
//! rows this block measured, so the number it prints is the number it hid.

import { TextAttributes } from "@opentui/core";
import { Lines, markerText, Stack } from "../../kit";
import type { Palette } from "../../theme/palette";
import { REASONING_EXPAND, REASONING_GLYPH, REASONING_LABEL } from "../glyphs";

/** The display rows a reasoning block shows before it collapses. */
export const REASONING_VISIBLE_ROWS = 0;

/** Renders one reasoning block at the columns its parent gives it. */
export function Reasoning({
  text,
  palette,
  expansion,
}: {
  text: string;
  palette: Palette;
  /** How many extra chunks of rows the reader revealed. */
  expansion: number;
}) {
  return (
    <Stack>
      <text attributes={TextAttributes.BOLD}>
        <span fg={palette.accent}>{REASONING_GLYPH}</span>
        <span fg={palette.reasoningHeader}>{` ${REASONING_LABEL}`}</span>
      </text>
      <Lines
        text={text}
        rows={REASONING_VISIBLE_ROWS + expansion}
        ink={palette.reasoningBody}
        indent={2}
        overflow={(hidden) => (
          <text wrapMode="none" fg={palette.reasoningMarker}>
            {markerText(REASONING_EXPAND, `… ${hidden} more · click or Ctrl+E`)}
          </text>
        )}
      />
    </Stack>
  );
}
