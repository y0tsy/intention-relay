//! A reasoning block: what the model thought before it answered.
//!
//! The block opens with its own header on the muted ink the block wears, and the
//! transcript paints the reasoning whole: a printed row is a record, and a
//! record that keeps a part of the turn out of the log is not the log. The step
//! in flight paints this same block at the same width, so the rows it streams
//! are the rows it keeps.

import { TextAttributes } from "@opentui/core";
import { Lines, markerText, Stack } from "../../kit";
import type { Palette } from "../../theme/palette";
import { REASONING_GLYPH, REASONING_LABEL } from "../glyphs";

/** Renders one reasoning block at the columns its parent gives it. */
export function Reasoning({ text, palette }: { text: string; palette: Palette }) {
  return (
    <Stack>
      <text fg={palette.reasoningHeader} attributes={TextAttributes.BOLD}>
        {markerText(REASONING_GLYPH, REASONING_LABEL)}
      </text>
      <Lines text={text} ink={palette.reasoningBody} indent={2} />
    </Stack>
  );
}
