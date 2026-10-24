//! A committed answer: the step's own prose, opened by its marker.
//!
//! The marker rides the answer's first display row and the body hangs one indent
//! under it, so a wrapped answer lines up under its first line rather than under
//! the glyph. The block is the same one the step in flight paints, so a live
//! answer does not jump when it commits.

import { Hanging, MarkdownBlocks } from "../../kit";
import type { Palette } from "../../theme/palette";
import { ANSWER_GLYPH } from "../glyphs";

/** Renders one committed answer block. */
export function Answer({ text, palette }: { text: string; palette: Palette }) {
  return (
    <Hanging glyph={ANSWER_GLYPH} ink={palette.inkMuted}>
      <MarkdownBlocks source={text} palette={palette} ink={palette.ink} />
    </Hanging>
  );
}
