//! A committed answer: the step's own prose under its label.
//!
//! The label is one row of its own, so the answer's first line and every line
//! under it share the transcript's measure. The block is the same one the step
//! in flight paints, so a live answer does not jump when it commits.

import { MarkdownBlocks, markerText, Stack } from "../../kit";
import type { Palette } from "../../theme/palette";
import { ANSWER_GLYPH, ASSISTANT_LABEL } from "../glyphs";

/** Renders one committed answer block. */
export function Answer({ text, palette }: { text: string; palette: Palette }) {
  return (
    <Stack>
      <text fg={palette.inkMuted}>{markerText(ANSWER_GLYPH, ASSISTANT_LABEL)}</text>
      <MarkdownBlocks source={text} palette={palette} ink={palette.ink} />
    </Stack>
  );
}
