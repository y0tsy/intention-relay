//! A committed user row: the reader's own message as a labelled card.
//!
//! The card is a frame on the palette's user wash whose label opens the top
//! border, and its content goes through the same markdown pipeline an answer
//! does: a user message is prose too, so a code fence, a list, or a bold run
//! renders inside the card exactly as it would in the answer it asked for.
//! A card too narrow for a frame falls back to its bare rows, so a narrow panel
//! shows the message instead of a broken box.

import { MarkdownBlocks, markerText, Pane } from "../../kit";
import type { Palette } from "../../theme/palette";
import { USER_GLYPH, USER_LABEL } from "../glyphs";

/** Renders one committed user row. */
export function UserCard({
  text,
  palette,
  width,
}: {
  text: string;
  palette: Palette;
  /** The card's own outer width; the parent's content width when omitted. */
  width?: number;
}) {
  return (
    <Pane
      title={markerText(USER_GLYPH, USER_LABEL)}
      titleColor={palette.userLabel}
      borderColor={palette.userBorder}
      background={palette.userSurface}
      paddingLeft={1}
      paddingRight={1}
      columns={width}
      fallbackBare
    >
      <MarkdownBlocks source={text} palette={palette} ink={palette.ink} />
    </Pane>
  );
}
