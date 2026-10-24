//! A committed user row: the reader's own message under its label.
//!
//! The transcript is a log, so the reader's turn is not a card: one labelled row
//! opens it and the message goes through the same markdown pipeline an answer
//! does. A code fence, a list, or a bold run renders inside the turn exactly as
//! it would in the answer it asked for.

import { MarkdownBlocks, markerText, Stack } from "../../kit";
import type { Palette } from "../../theme/palette";
import { USER_GLYPH, USER_LABEL } from "../glyphs";

/** Renders one committed user row. */
export function UserCard({ text, palette }: { text: string; palette: Palette }) {
  return (
    <Stack>
      <text fg={palette.userLabel}>{markerText(USER_GLYPH, USER_LABEL)}</text>
      <MarkdownBlocks source={text} palette={palette} ink={palette.ink} />
    </Stack>
  );
}
