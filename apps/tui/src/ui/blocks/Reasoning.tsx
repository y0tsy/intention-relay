//! A reasoning block: what the model thought before it answered.
//!
//! The block opens with its own header on the muted ink the block wears, and the
//! transcript prints the reasoning whole: a printed row is a record, and a record
//! that keeps a part of the turn out of the log is not the log. The step in flight
//! is bounded instead, because the footer's band is: it names the rows it hid and
//! `Ctrl+E` reveals them one chunk at a time.

import { TextAttributes } from "@opentui/core";
import { Lines, markerText, Stack } from "../../kit";
import type { Palette } from "../../theme/palette";
import { REASONING_EXPAND, REASONING_GLYPH, REASONING_LABEL } from "../glyphs";

/** The display rows the step in flight shows before it collapses. */
export const REASONING_VISIBLE_ROWS = 4;

/** More rows than any printed reasoning block paints. */
const REASONING_WHOLE = 100_000;

/** Renders one reasoning block at the columns its parent gives it. */
export function Reasoning({
  text,
  palette,
  expansion = 0,
  printed = false,
}: {
  text: string;
  palette: Palette;
  /** How many extra chunks of rows the reader revealed. */
  expansion?: number;
  /** Whether the block is a printed row, which keeps everything it was given. */
  printed?: boolean;
}) {
  return (
    <Stack>
      <text fg={palette.reasoningHeader} attributes={TextAttributes.BOLD}>
        {markerText(REASONING_GLYPH, REASONING_LABEL)}
      </text>
      <Lines
        text={text}
        rows={printed ? REASONING_WHOLE : REASONING_VISIBLE_ROWS + expansion}
        ink={palette.reasoningBody}
        indent={2}
        overflow={
          printed
            ? undefined
            : (hidden) => (
                <text wrapMode="none" fg={palette.reasoningMarker}>
                  {markerText(REASONING_EXPAND, `… ${hidden} more · click or Ctrl+E`)}
                </text>
              )
        }
      />
    </Stack>
  );
}
