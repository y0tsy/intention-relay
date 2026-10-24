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
}

/**
 * Returns the geometry the docked browser takes, or `null` while it does not fit.
 *
 * The panel is a layout row at the bottom of the window: it grows with the result
 * up to half the window, so the footer above it keeps the majority, and it never
 * opens at all when the footer would fall below its own floor.
 */
export function browserPlan(width: number, height: number, rowCount: number): BrowserLayout | null {
  if (width <= 2) {
    return null;
  }
  const chrome = 2 + BROWSER_CHROME_ROWS + BROWSER_FOOTER_ROWS;
  const cap = Math.max(MIN_PANEL_ROWS, Math.floor(height / 2));
  const dataCap = Math.max(1, cap - chrome);
  const dataRows = Math.min(Math.max(rowCount, 1), dataCap);
  const rows = Math.min(Math.max(chrome + dataRows, MIN_PANEL_ROWS), cap);
  if (height < rows + DOCK_GAP_ROWS + MIN_CHAT_ROWS) {
    return null;
  }
  return { rows, dataRows, contentColumns: width - BROWSER_CHROME_COLUMNS };
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
  return Math.max(0, windowColumns - FOOTER_MARGIN);
}
