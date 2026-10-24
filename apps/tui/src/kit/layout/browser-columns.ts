//! The session table's column widths inside one content width.
//!
//! The purpose columns take a share of the flexible width each, so the table
//! stretches with the panel; the title column takes what they leave.

/** The columns the browser's table paints, in order. */
export interface BrowserColumns {
  /** The age of the last update. */
  readonly modified: number;
  /** The age of the session's creation. */
  readonly created: number;
  /** The rows the session holds. */
  readonly size: number;
  /** The session's own name. */
  readonly title: number;
  /** The workspace the session is bound to. */
  readonly path: number;
}

/** The columns one session row's marker occupies. */
const MARKER_WIDTH = 2;

/** The one column between two table columns. */
const COLUMN_GAP = 1;

/** The fewest columns each purpose column keeps. */
const MIN_MODIFIED = 8;
const MIN_CREATED = 7;
const MIN_SIZE = 4;
const MIN_PATH = 4;

/**
 * Returns the table's column widths for one content width.
 *
 * A narrow table never pushes a column negative: the title takes the rest, or
 * none, and the renderer clips what a column could not hold.
 */
export function browserColumnWidths(width: number): BrowserColumns {
  const flexible = Math.max(0, width - (MARKER_WIDTH + COLUMN_GAP * 4));
  const share = (percent: number): number => Math.floor((flexible * percent) / 100);
  const modified = Math.max(share(16), MIN_MODIFIED);
  const created = Math.max(share(10), MIN_CREATED);
  const size = Math.max(share(7), MIN_SIZE);
  const path = Math.max(share(24), MIN_PATH);
  return {
    modified,
    created,
    size,
    path,
    title: Math.max(0, flexible - (modified + created + size + path)),
  };
}
