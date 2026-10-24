//! One bounded run of text: the rows a block paints and the rows it hid.
//!
//! A reasoning block and a tool preview both show a bounded number of display
//! rows and name the rows they hid. The wrap, the bound, and the count are one
//! primitive, so a block never measures its own text.

import type { ReactNode } from "react";
import { displayWidth, takeRows } from "../text";
import { useContentColumns } from "./context";

/** Renders `text` wrapped to the content width, bounded to `rows` rows. */
export function Lines({
  text,
  rows,
  ink,
  rail,
  overflow,
}: {
  text: string;
  rows: number;
  ink?: string;
  /** A one-column run every row opens with, its own gap included. */
  rail?: string;
  /** Renders the note that names the rows the bound hid. */
  overflow?: (hidden: number) => ReactNode;
}) {
  const columns = useContentColumns();
  const railWidth = rail === undefined ? 0 : displayWidth(rail) + 1;
  const width = Math.max(1, columns - railWidth);
  const { shown, hidden } = takeRows(text, width, rows);
  return (
    <box flexDirection="column">
      {shown.map((row, index) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: the rows are a positional run of lines.
        <text key={index} wrapMode="none" fg={ink}>
          {rail === undefined ? row : `${rail} ${row}`}
        </text>
      ))}
      {hidden > 0 && overflow !== undefined ? overflow(hidden) : null}
    </box>
  );
}
