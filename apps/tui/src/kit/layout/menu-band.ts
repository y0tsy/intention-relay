//! How a slot row region spends one panel's rows.
//!
//! A band and a picker both take their rows from the region above the input: the
//! caller names the content rows it shows, and the pane's frame spends its own
//! two. The footer plans the slot with these handles, so no surface adds its
//! frame's chrome to its own rows by hand.

import { paneSlotRows } from "./Pane";

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
