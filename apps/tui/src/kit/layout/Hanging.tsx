//! The marker a transcript block opens with.
//!
//! A block's glyph rides its first display row and every row after it starts at
//! the same text column, so a wrapped line lines up under the first line instead
//! of under the glyph. The brick owns the marker's columns, so the body never
//! measures them.

import type { ReactNode } from "react";
import { displayWidth } from "../text";
import { ContentColumns, useContentColumns } from "./context";

/** Returns the columns a marker's glyph and the gap take. */
export function markerColumns(glyph: string, gap = 1): number {
  return displayWidth(glyph) + gap;
}

/**
 * Renders a block whose marker rides the first row and whose body hangs under it.
 */
export function Hanging({
  glyph,
  ink,
  children,
  gap = 1,
}: {
  glyph: string;
  ink: string;
  children?: ReactNode;
  gap?: number;
}) {
  const columns = useContentColumns();
  const marker = markerColumns(glyph, gap);
  return (
    <box flexDirection="row">
      <box width={marker}>
        <text fg={ink}>{`${glyph}${" ".repeat(gap)}`}</text>
      </box>
      <box flexGrow={1} flexShrink={1} flexDirection="column">
        <ContentColumns columns={Math.max(1, columns - marker)}>{children}</ContentColumns>
      </box>
    </box>
  );
}
