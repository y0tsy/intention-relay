//! The window of rows a list shows around the row the reader is on.
//!
//! A band, a picker, and the session table all keep the selected row visible and
//! report the rows the window counts. One function decides where the window
//! starts; no surface recomputes it.

/** One list's visible row window. */
export interface ScrollWindow {
  /** The index of the first visible row. */
  readonly start: number;
  /** The index one past the last visible row. */
  readonly end: number;
  /** The one-based number of the first visible row, or `0` for an empty list. */
  readonly first: number;
  /** The one-based number of the last visible row, or `0` for an empty list. */
  readonly last: number;
}

/** Returns the window of `visible` rows that keeps `selected` in view. */
export function scrollWindow({
  count,
  visible,
  selected,
}: {
  count: number;
  visible: number;
  selected: number;
}): ScrollWindow {
  const shown = Math.max(1, Math.min(visible, count));
  const start = Math.max(0, Math.min(selected - shown + 1, count - shown));
  return {
    start,
    end: start + shown,
    first: count === 0 ? 0 : start + 1,
    last: count === 0 ? 0 : start + shown,
  };
}
