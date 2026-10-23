//! A committed answer: the step's own prose, opened by its marker.
//!
//! The marker rides the answer's first display row and the body hangs one indent
//! under it, so a wrapped answer lines up under its first line rather than under
//! the glyph. The block is the same one the step in flight paints, so a live
//! answer does not jump when it commits.

import type { Palette } from "../../theme/palette";
import { ANSWER_GLYPH } from "../glyphs";
import { MarkdownBlocks } from "../Markdown";
import { Marked } from "./marker";

/** Renders one committed answer block. */
export function Answer({ text, palette }: { text: string; palette: Palette }) {
  return (
    <Marked glyph={ANSWER_GLYPH} ink="inkMuted" palette={palette}>
      <MarkdownBlocks source={text} palette={palette} ink={palette.ink} />
    </Marked>
  );
}
