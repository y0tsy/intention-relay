//! One fixed-width cell: the text, its padding, and its ink.
//!
//! A table column and a search bar both need a run that fills exactly the
//! columns it was given. The cell pads and cuts by display width, so a wide
//! glyph never shifts the column after it.

import { TextAttributes } from "@opentui/core";
import { cellText, cellTextRight } from "../text";
import { useContentColumns } from "./context";

/** Renders one cell padded and cut to its column. */
export function Cell({
  text,
  width,
  ink,
  background,
  bold = false,
  align = "left",
}: {
  text: string;
  /** The cell's own columns; the parent's content width when omitted. */
  width?: number;
  ink?: string;
  background?: string;
  bold?: boolean;
  align?: "left" | "right";
}) {
  const inherited = useContentColumns();
  const columns = width ?? inherited;
  const content = align === "right" ? cellTextRight(text, columns) : cellText(text, columns);
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
