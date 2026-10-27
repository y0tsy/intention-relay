//! A blank area painted with the application's own page colour.
//!
//! A terminal owns the cells an application does not draw, and this one writes
//! only the cells that carry a glyph: a row that draws nothing keeps whatever
//! the terminal had behind it, which on a themed page is a hole. The page holds
//! its rows with a non-breaking space, drawn in the page colour so the row reads
//! as empty and paints as the page.
//!
//! Selecting the transcript copies the page along the rows: a blank line in the
//! copy is a row of non-breaking spaces, which renders as a blank line anywhere
//! and strips like whitespace.

/** The glyph a blank cell holds: a space the renderer writes and keeps. */
const BLANK = "\u00a0";

/** Renders `rows` full-width rows of the page's colour. */
export function Surface({
  rows,
  columns,
  color,
}: {
  rows: number;
  columns: number;
  color: string;
}) {
  if (rows < 1 || columns < 1) {
    return null;
  }
  const row = BLANK.repeat(columns);
  return (
    <text fg={color} bg={color}>
      {Array.from({ length: rows }, () => row).join("\n")}
    </text>
  );
}
