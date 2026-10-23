//! A committed user row: the reader's own message as a labelled card.
//!
//! The card is a frame on the palette's user wash whose label opens the top
//! border, and its content goes through the same markdown pipeline an answer
//! does: a user message is prose too, so a code fence, a list, or a bold run
//! renders inside the card exactly as it would in the answer it asked for.
//! A card too narrow for a frame falls back to its bare rows, so a narrow panel
//! shows the message instead of a broken box.

import type { Palette } from "../../theme/palette";
import { markerText, USER_GLYPH, USER_LABEL } from "../glyphs";
import { MarkdownBlocks } from "../Markdown";

/** The columns a card's frame and padding take: two borders and two paddings. */
export const CARD_CHROME_COLUMNS = 4;

/** Renders one committed user row. */
export function UserCard({
  text,
  palette,
  width,
}: {
  text: string;
  palette: Palette;
  width: number;
}) {
  if (width <= CARD_CHROME_COLUMNS) {
    return <MarkdownBlocks source={text} palette={palette} ink={palette.ink} />;
  }
  return (
    <box
      flexDirection="column"
      border
      borderStyle="rounded"
      borderColor={palette.userBorder}
      backgroundColor={palette.userSurface}
      title={markerText(USER_GLYPH, USER_LABEL)}
      titleColor={palette.userLabel}
      titleAlignment="left"
      paddingLeft={1}
      paddingRight={1}
    >
      <MarkdownBlocks source={text} palette={palette} ink={palette.ink} />
    </box>
  );
}
