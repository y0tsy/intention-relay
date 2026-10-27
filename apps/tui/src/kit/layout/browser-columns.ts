//! The session table's column widths, and the columns its legend has, inside one
//! content width.
//!
//! The purpose columns take a share of the flexible width each, so the table
//! stretches with the panel; the title column takes what they leave. The legend
//! and its counter share one row, and the counter keeps its own columns whatever
//! the legend needs.

import { displayWidth } from "../text";

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

/** The columns the browser's legend and its counter share, and what fits there. */
export interface BrowserLegend {
  /** The columns the legend's own row has, left of the counter. */
  readonly columns: number;
  /** Whether the text the legend was given fits in those columns. */
  readonly fits: boolean;
}

/** The columns one session row's marker occupies. */
const MARKER_WIDTH = 2;

/** The one column between the legend and the counter. */
const LEGEND_GAP = 1;

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

/**
 * Returns the columns the browser's legend has beside its counter, and whether
 * the legend's own text fits.
 *
 * The legend names the keys a reader can press and the counter says where they
 * are: a panel too narrow for both keeps the counter whole and cuts the legend
 * down to what fits, rather than painting one over the other.
 */
export function browserLegendColumns(
  columns: number,
  legend: string,
  counter: string,
): BrowserLegend {
  const left = Math.max(0, columns - displayWidth(counter) - LEGEND_GAP);
  return { columns: left, fits: displayWidth(legend) <= left };
}
