//! One bounded run of text: the rows a block paints and the rows it hid.
//!
//! A reasoning block and a tool preview both show a bounded number of display
//! rows and name the rows they hid. The wrap, the bound, and the count are one
//! primitive, so a block never measures its own text.

import type { ReactNode } from "react";
import { displayWidth, takeRows } from "../text";
import { ContentColumns, useContentColumns } from "./context";

/**
 * Renders `text` wrapped to the content width, bounded to `rows` rows.
 *
 * An `indent` keeps the body in a column of its own and the note that names the
 * hidden rows at the block's own first column, which is how a marked block
 * closes under its marker rather than under its body.
 */
export function Lines({
  text,
  rows,
  ink,
  rail,
  indent = 0,
  background,
  overflow,
}: {
  text: string;
  rows: number;
  ink?: string;
  /** A one-column run every row opens with, its own gap included. */
  rail?: string;
  /** The blank columns the body keeps at its left; the note keeps none. */
  indent?: number;
  /** The wash behind the indented body. */
  background?: string;
  /** Renders the note that names the rows the bound hid. */
  overflow?: (hidden: number) => ReactNode;
}) {
  const columns = useContentColumns();
  // The indent moves the body's first column; the run still wraps to the
  // block's own width, so a marked block uses the columns it was given.
  const bodyColumns = Math.max(1, columns);
  const railWidth = rail === undefined ? 0 : displayWidth(rail) + 1;
  const width = Math.max(1, bodyColumns - railWidth);
  const { shown, hidden } = takeRows(text, width, rows);
  const body = (
    <box flexDirection="column">
      {shown.map((row, index) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: the rows are a positional run of lines.
        <text key={index} wrapMode="none" fg={ink}>
          {rail === undefined ? row : `${rail} ${row}`}
        </text>
      ))}
    </box>
  );
  return (
    <box flexDirection="column">
      {indent === 0 && background === undefined ? (
        body
      ) : (
        <box paddingLeft={indent} backgroundColor={background}>
          <ContentColumns columns={bodyColumns}>{body}</ContentColumns>
        </box>
      )}
      {hidden > 0 && overflow !== undefined ? overflow(hidden) : null}
    </box>
  );
}
