//! How a slot row region spends one panel's rows.
//!
//! A band and a picker both take their rows from the region above the input: the
//! caller names the content rows it shows, and the pane's frame spends its own
//! two. The footer plans the slot with these handles, so no surface adds its
//! frame's chrome to its own rows by hand.

import { displayWidth } from "../text";
import { paneSlotRows } from "./Pane";

/** The columns the band's cursor marker takes, so every row aligns with it. */
const MARKER_COLUMNS = 2;

/** The one blank column between two band columns. */
const COLUMN_GAP = 1;

/** The widths of one band's columns; a `0` hides the column. */
export interface BandColumnWidths {
  readonly label: number;
  readonly description: number;
  readonly trailing: number;
}

/**
 * Returns the band's column widths for one content width.
 *
 * The label and the trailing cell keep the width of their widest value across
 * the whole registry, so the columns never jump while the reader types; the
 * description takes what they leave. A band too narrow for all three keeps the
 * label alone.
 */
export function bandColumnWidths(
  available: number,
  labels: readonly string[],
  trailing: readonly string[],
): BandColumnWidths {
  const label = widest(labels);
  const trailingCell = widest(trailing);
  const fixed = MARKER_COLUMNS + label + COLUMN_GAP + trailingCell;
  if (available > fixed + COLUMN_GAP) {
    return { label, description: available - fixed - COLUMN_GAP, trailing: trailingCell };
  }
  return { label: Math.max(0, available - MARKER_COLUMNS), description: 0, trailing: 0 };
}

/** Returns the widest display width of one cell's values. */
function widest(cells: readonly string[]): number {
  return cells.reduce((wide, cell) => Math.max(wide, displayWidth(cell)), 0);
}

/** The rows one menu band shows, at most: the cursor's window of suggestions. */
export const MAX_BAND_ROWS = 3;

/**
 * Returns the slot rows a menu band occupies for one ranked row count.
 *
 * A closed band takes no row; an open one takes its window of rows and, when
 * the list is longer, one overflow row that names the rest.
 */
export function bandSlotRows(count: number | null, max = MAX_BAND_ROWS): number {
  if (count === null || count === 0) {
    return 0;
  }
  return paneSlotRows(Math.min(count, max) + (count > max ? 1 : 0));
}

/** Returns the slot rows a picker panel occupies for its content rows. */
export function pickerSlotRows(contentRows: number): number {
  return paneSlotRows(contentRows);
}
