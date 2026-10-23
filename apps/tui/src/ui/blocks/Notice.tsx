//! A committed notice: daemon-authored content bound to the run.
//!
//! A notice is not model prose, so its text never goes through the markdown
//! pipeline and never reads as an answer. Its marker rides the first row, its
//! body keeps the notice wash and ink, and a notice is never re-classified: the
//! row carries free text and the block prints exactly that.

import type { Palette } from "../../theme/palette";
import { NOTICE_GLYPH } from "../glyphs";
import { Marked } from "./marker";

/** Renders one committed notice block. */
export function Notice({ text, palette }: { text: string; palette: Palette }) {
  return (
    <Marked glyph={NOTICE_GLYPH} ink="noticeInk" palette={palette}>
      <box backgroundColor={palette.noticeSurface}>
        <text wrapMode="word" fg={palette.noticeInk}>
          {text}
        </text>
      </box>
    </Marked>
  );
}
