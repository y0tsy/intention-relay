//! The marker every transcript block opens with.
//!
//! A block's glyph rides its first display row and every row after it starts at
//! the same text column, so a wrapped line lines up under the first line instead
//! of under the glyph. The indent is measured through the terminal's own width
//! helper, so a wide glyph never shifts the body it introduces.

import type { ReactNode } from "react";
import type { Palette } from "../../theme/palette";
import { MARKER_GAP } from "../glyphs";
import { displayWidth } from "../measure";

/** Returns the columns a marker's glyph and the shared gap take. */
export function markerColumns(glyph: string): number {
  return displayWidth(glyph) + MARKER_GAP;
}

/**
 * Renders a block whose marker rides the first row and whose body hangs under it.
 *
 * The body is laid out one column short of the block's width as a whole, so a
 * marked row is never wider than the block it belongs to.
 */
export function Marked({
  glyph,
  ink,
  palette,
  children,
}: {
  glyph: string;
  ink: keyof Palette;
  palette: Palette;
  children: ReactNode;
}) {
  return (
    <box flexDirection="row">
      <box width={markerColumns(glyph)}>
        <text fg={palette[ink]}>{`${glyph}${" ".repeat(MARKER_GAP)}`}</text>
      </box>
      <box flexGrow={1} flexShrink={1} flexDirection="column">
        {children}
      </box>
    </box>
  );
}
