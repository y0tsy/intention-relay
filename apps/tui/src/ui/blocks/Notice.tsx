//! A committed notice: daemon-authored content bound to the run.
//!
//! A notice is not model prose, so its text never goes through the markdown
//! pipeline and never reads as an answer. Its marker rides the first row, its
//! body keeps the notice wash and ink, and a notice is never re-classified: the
//! row carries free text and the block prints exactly that.

import { Hanging } from "../../kit";
import type { Palette } from "../../theme/palette";
import { NOTICE_GLYPH } from "../glyphs";

/** Renders one committed notice block. */
export function Notice({ text, palette }: { text: string; palette: Palette }) {
  return (
    <Hanging glyph={NOTICE_GLYPH} ink={palette.noticeInk}>
      <box backgroundColor={palette.noticeSurface}>
        <text wrapMode="word" fg={palette.noticeInk}>
          {text}
        </text>
      </box>
    </Hanging>
  );
}
