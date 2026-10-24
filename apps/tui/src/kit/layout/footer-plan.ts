//! How the live footer spends a window: its frame, the step in flight, the
//! band, the input, and the notice line.
//!
//! The transcript itself is printed into the terminal's own scrollback, so the
//! rows the window keeps for the application are the ones that can still
//! change. The budget is one pure function from a window to the rows of each
//! region: the surfaces beneath read the plan and paint, they never add a
//! frame's chrome to their own rows.

/** The columns and rows the window keeps around the footer. */
import { PANE_PADDING } from "./Pane";
import { INPUT_CHROME_ROWS } from "./TextField";

export const FOOTER_MARGIN = 1;

/** The columns the footer needs to say anything at all. */
export const FOOTER_MIN_COLUMNS = 8;

/** The columns one vertical frame edge takes. */
const FRAME_EDGE = 1;

/** The rows the notice or error line occupies under the input block. */
export const DETAIL_ROWS = 1;

/** The share of the footer's body the input block may show buffer lines for. */
const INPUT_LINE_SHARE = 3;

/** The buffer lines the input block shows even in a body too short for its share. */
const INPUT_MIN_LINES = 3;

/** The rows the band above the input block keeps for the step in flight. */
export const LIVE_BAND_ROWS = 6;

/** The columns the printed transcript and the footer's content start at. */
export const CONTENT_INDENT = FOOTER_MARGIN + FRAME_EDGE + PANE_PADDING;

/** What the footer's live region holds this frame. */
export type FooterMode =
  /** No session is open: the welcome pane fills the region. */
  | "welcome"
  /** A session is open: the region keeps its band for the step in flight. */
  | "session";

/** The geometry of the footer in one window. */
export interface FooterLayout {
  /** The window's own columns, the page the footer paints. */
  readonly windowColumns: number;
  /** The columns the footer's frame occupies. */
  readonly columns: number;
  /** The rows the footer occupies, which is what the window is asked to reserve. */
  readonly rows: number;
  /** The rows the footer spends: its frame, its margin, and everything inside. */
  readonly body: number;
  /** The rows the frame's border and the window's margin take of the body. */
  readonly chromeRows: number;
  /** The rows of the step in flight, or of the welcome pane. */
  readonly liveRows: number;
  /** The rows above the input block: the band, or the picker's panel. */
  readonly hintRows: number;
  /** The rows the input block occupies. */
  readonly inputRows: number;
  /** The rows the notice or error line occupies. */
  readonly detailRows: number;
  /** The columns the footer's content has. */
  readonly contentColumns: number;
  /** The rows a surface below the footer takes, docked browser and gap included. */
  readonly dockedRows: number;
}

/**
 * Returns how the footer spends a window `terminalHeight` rows tall.
 *
 * `docked` is the rows a surface below the footer takes - the session browser and
 * the gap above it - so the footer leaves those rows to the surface.
 */
export function footerPlan(
  width: number,
  terminalHeight: number,
  wantedInputRows: number,
  wantedHintRows: number,
  mode: FooterMode,
  docked = 0,
  printedRows = 0,
): FooterLayout {
  const available = Math.max(0, terminalHeight - docked);
  const detailRows = available > 0 ? DETAIL_ROWS : 0;
  // The welcome paints its frame from its own first row, so it also owns the
  // blank row above the frame and the frame's top edge. A session's frame starts
  // in the rows the transcript printed, so the footer owns only the frame's
  // bottom edge and the blank row under it.
  const chromeRows =
    available > 0 ? (mode === "welcome" ? WELCOME_CHROME_ROWS : SESSION_CHROME_ROWS) : 0;
  const room = Math.max(0, available - detailRows - chromeRows);
  // The cap bounds the buffer lines the block shows, never its own chrome: a
  // block taller than its share windows the lines instead of growing further.
  const share = Math.floor(room / INPUT_LINE_SHARE);
  const cap = Math.min(Math.max(share, INPUT_MIN_LINES) + INPUT_CHROME_ROWS, room);
  const inputRows = Math.min(wantedInputRows, cap);
  // The band takes its rows exactly or not at all: a body too short for it beside
  // the input block keeps the input and shows no band.
  const hintRows = wantedHintRows > 0 && wantedHintRows + inputRows <= room ? wantedHintRows : 0;
  const band = Math.max(0, room - hintRows - inputRows);
  // The band belongs to the window, not to the step: it keeps its rows so the
  // footer never moves while a run streams and commits. Only the welcome pane
  // takes the whole window, because nothing has been printed above it yet.
  const liveRows = mode === "welcome" ? band : Math.min(band, LIVE_BAND_ROWS);
  const body = chromeRows + detailRows + hintRows + liveRows + inputRows;
  const natural = Math.min(terminalHeight, Math.min(available, body) + docked);
  // The rows the print has not reached are the footer's: the band takes them, so
  // the empty part of the window sits between the transcript and the input, and
  // a short session never pads its own history with blank rows.
  const reached = Math.min(printedRows, terminalHeight);
  const slack = Math.max(0, terminalHeight - reached - natural);
  return {
    windowColumns: width,
    columns: Math.max(0, width - FOOTER_MARGIN * 2),
    rows: natural + slack,
    body: body + slack,
    chromeRows,
    liveRows: liveRows + slack,
    hintRows,
    inputRows,
    detailRows,
    contentColumns: Math.max(0, width - CONTENT_INDENT * 2),
    dockedRows: docked,
  };
}

/** Returns the rows one chunk of a print may carry above the footer. */
export function printRoom(terminalHeight: number, footerHeight: number): number {
  return Math.max(1, Math.trunc(terminalHeight) - Math.trunc(footerHeight));
}

/** The rows the welcome's frame spends: its two edges and the margin above and below. */
export const WELCOME_CHROME_ROWS = 4;

/** The rows a session's frame spends: its bottom edge and the margin below it. */
export const SESSION_CHROME_ROWS = 2;
