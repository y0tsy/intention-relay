//! How the docked session browser spends the window's bottom rows.
//!
//! The browser is a layout row under the footer, never an overlay: the chat
//! panel above it reflows into the rows it leaves, and a window too short for
//! both keeps the chat and opens no browser. The panel grows with its result up
//! to half the window, so the footer above it keeps the majority.

import { FOOTER_MARGIN } from "./footer-plan";

/** The fewest rows the docked browser panel occupies, however small the result. */
export const MIN_PANEL_ROWS = 6;

/** The fewest rows the footer keeps above the docked browser and its gap. */
export const MIN_CHAT_ROWS = 11;

/** The blank canvas row the footer keeps above the docked browser. */
export const DOCK_GAP_ROWS = 1;

/** The browser body rows that are not session rows. */
export const BROWSER_CHROME_ROWS = 6;

/**
 * The body rows a compact browser keeps: its tab row and the rows the panel
 * closes with. A panel with no room for the filter and the column headings
 * shows the sessions themselves instead.
 */
export const COMPACT_BROWSER_ROWS = 3;

/** The rows the browser's footer occupies: its separator row, then the legend. */
export const BROWSER_FOOTER_ROWS = 2;

/** The browser columns its own frame and padding take. */
export const BROWSER_CHROME_COLUMNS = 6;

/** One docked browser's geometry inside a window. */
export interface BrowserLayout {
  /** The rows the docked panel occupies. */
  readonly rows: number;
  /** The session rows the panel has room for. */
  readonly dataRows: number;
  /** The columns the browser's own content has. */
  readonly contentColumns: number;
  /** Whether the panel drops its filter row and its column headings. */
  readonly compact: boolean;
}

/**
 * Returns the geometry the browser panel takes, or `null` while it does not fit.
 *
 * The panel is a page of its own: a blank row above it and the window's bottom
 * margin under it are the whole chrome around it, and it may take the footer's
 * rows while it is open, because the panel owns the keyboard and the composer
 * waits for it to close. `preferRows` is the room the footer has now; the panel
 * plans inside it and takes more only to reach its own floor, because a row it
 * grows over is a printed row on screen. A page with the whole window to itself
 * keeps half of it for the window it was opened from.
 */
export function browserPlan(
  width: number,
  height: number,
  rowCount: number,
  preferRows = height,
): BrowserLayout | null {
  if (width <= 2) {
    return null;
  }
  const fullChrome = 2 + BROWSER_CHROME_ROWS + BROWSER_FOOTER_ROWS;
  const compactChrome = 2 + COMPACT_BROWSER_ROWS;
  // The panel's own rows, without the row it stands under and the margin it
  // closes above.
  const room = Math.min(height, preferRows) - DOCK_GAP_ROWS - FOOTER_MARGIN;
  const compact = room < fullChrome + 2;
  const chrome = compact ? compactChrome : fullChrome;
  const floor = Math.max(compact ? 0 : MIN_PANEL_ROWS, chrome + 1);
  const fits = Math.max(room, Math.min(floor, height - DOCK_GAP_ROWS - FOOTER_MARGIN));
  if (fits < floor) {
    return null;
  }
  // Half the page keeps the row a taller window left for the tray it was opened
  // over, and the floor keeps the panel's own chrome and one session row.
  const cap = Math.max(floor, Math.min(fits, Math.floor((fits + 2) / 2)));
  const dataCap = Math.max(1, cap - chrome);
  const dataRows = Math.min(Math.max(rowCount, 1), dataCap);
  const rows = Math.min(Math.max(chrome + dataRows, floor), cap);
  if (height < rows + DOCK_GAP_ROWS + FOOTER_MARGIN) {
    return null;
  }
  return { rows, dataRows, contentColumns: width - BROWSER_CHROME_COLUMNS, compact };
}

/**
 * Returns the session rows one docked panel shows.
 *
 * The panel's own chrome is fixed, so the rows left after it are the rows the
 * table can fill; a panel at its floor still shows one row.
 */
export function browserDataRows(panelRows: number): number {
  return Math.max(1, panelRows - (2 + BROWSER_CHROME_ROWS + BROWSER_FOOTER_ROWS));
}

/** The outer columns the browser's panel occupies in one window. */
export function browserPanelColumns(windowColumns: number): number {
  return Math.max(0, windowColumns - FOOTER_MARGIN * 2);
}
