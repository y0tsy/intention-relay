//! One fixed-width cell: the text, its padding, and its ink.
//!
//! A table column and a search bar both need a run that fills exactly the
//! columns it was given. The cell pads and cuts by display width, so a wide
//! glyph never shifts the column after it.

import { TextAttributes } from "@opentui/core";
import { cellText, cellTextRight } from "../text";

/** Renders one cell padded and cut to `width` columns. */
export function Cell({
  text,
  width,
  ink,
  background,
  bold = false,
  align = "left",
}: {
  text: string;
  width: number;
  ink?: string;
  background?: string;
  bold?: boolean;
  align?: "left" | "right";
}) {
  const content = align === "right" ? cellTextRight(text, width) : cellText(text, width);
  return (
    <text
      wrapMode="none"
      fg={ink}
      bg={background}
      attributes={bold ? TextAttributes.BOLD : undefined}
    >
      {content}
    </text>
  );
}
