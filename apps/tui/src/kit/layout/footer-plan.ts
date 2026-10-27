//! How the live footer spends a window: its frame, the step in flight, the
//! edge, the input, and the notice line.
//!
//! The transcript itself is printed into the terminal's own scrollback, so the
//! rows the window keeps for the application are the ones that can still
//! change. The budget is one pure function from a window to the rows of each
//! region: the surfaces beneath read the plan and paint, they never add a
//! frame's chrome to their own rows.

/** The columns and rows the window keeps around the footer. */
import { PANE_PADDING } from "./Pane";
import { COMPOSER_CHROME_ROWS, INPUT_CHROME_ROWS } from "./TextField";

/** The columns the page keeps on each side of the window. */
export const FOOTER_MARGIN = 1;

/**
 * The columns a session's frame takes on each side.
 *
 * The frame's rail is the column the transcript's rule runs down, so the tray
 * and the printed page above it meet on one vertical line.
 */
export const SESSION_MARGIN = 3;

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

/** The columns the printed transcript and the footer's content start at. */
export const CONTENT_INDENT = FOOTER_MARGIN + FRAME_EDGE + PANE_PADDING;

/** The columns the transcript's rule occupies, its one-column margin included. */
export const RULE_COLUMNS = 4;

/** The column between the rule and the text. */
export const RULE_GAP = 1;

/** The columns a row's rule and gap take before its text. */
export const TRANSCRIPT_INDENT = RULE_COLUMNS + RULE_GAP;

/** The columns the text keeps on the right, as the footer's input does. */
export const RIGHT_MARGIN = 4;

/**
 * The rows the edge keeps for the step in flight.
 *
 * The printer seals a row once a later row exists, so the last rows a stream
 * paints are the ones it may still rewrite: the edge holds them and they are
 * the rows the printer withholds. Two rows and not one, because a table's
 * delimiter or a setext underline arrives after the row it reinterprets.
 */
export const EDGE_ROWS = 2;

/** Returns the columns one transcript row's text has. */
export function transcriptColumns(width: number): number {
  return Math.max(0, width - TRANSCRIPT_INDENT - RIGHT_MARGIN);
}

/** What the footer's region holds this frame. */
export type FooterMode =
  /** No session is open: the welcome pane fills the region. */
  | "welcome"
  /** A session is open: the region keeps its edge for the step in flight. */
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
  /** The rows the step in flight's edge holds, or the welcome pane's rows. */
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
  /** The rows above the frame that belong to the page, not to the footer. */
  readonly slackRows: number;
}

/**
 * Returns the rows the input block keeps out of the rows it was offered.
 *
 * The block paints its own frame and the line the draft is typed on, so a block
 * with fewer rows than those two paints the frame and puts the line outside it.
 */
function fittedInput(rows: number, wantedRows: number, floor: number): number {
  const fitted = Math.min(wantedRows, rows);
  return fitted >= floor ? fitted : 0;
}

/**
 * Returns how the footer spends a window `terminalHeight` rows tall.
 *
 * `docked` is the rows a surface below the footer takes - the session browser and
 * the gap above it - so the footer leaves those rows to the surface.
 *
 * `printedRows` is how many rows the page has written above the footer: a
 * session's window reaches the terminal's last row and gives the rows a short
 * print left above it to the page, so the tray stays where the reader left it
 * and the edge meets the transcript on one line.
 *
 * `maxRows` is the ceiling the footer may take. The renderer grows a footer by
 * scrolling its region, and the rows such a scroll passes leave the scrollback
 * for good, so a page that has printed a row may not hand rows back: a panel
 * that wants more than the footer has is clipped instead.
 */
export function footerPlan(
  width: number,
  terminalHeight: number,
  wantedInputRows: number,
  wantedHintRows: number,
  mode: FooterMode,
  docked = 0,
  printedRows = 0,
  maxRows = terminalHeight,
): FooterLayout {
  const available = Math.max(0, terminalHeight - docked);
  // The region the footer's own rows may use: a ceiling below the window's own
  // height trims the slots rather than letting them paint past the region.
  const ceiling = Math.max(0, Math.max(1, maxRows) - docked);
  const budget = Math.min(available, ceiling);
  // A welcome paints an open page: its header row and the blank row under the
  // content are the chrome. A session's frame starts in the rows the transcript
  // printed, so the footer owns only the frame's bottom edge and the blank row
  // under it.
  const chromeWanted = mode === "welcome" ? WELCOME_CHROME_ROWS : SESSION_CHROME_ROWS;
  // A region too short for the frame's own bottom edge and the margin under it
  // paints no slot at all: the frame needs a row of its own before it can hold
  // one, and a slot painted without it would stand outside the region.
  const short = budget > 0 && budget < chromeWanted;
  const chromeRows = budget > 0 && !short ? chromeWanted : 0;
  // The detail line is the last row the frame spends: a region with no room for
  // it keeps the frame and drops the line.
  const detailRows = !short && budget - chromeRows >= DETAIL_ROWS ? DETAIL_ROWS : 0;
  const room = short ? 0 : Math.max(0, budget - detailRows - chromeRows);
  // The cap bounds the buffer lines the block shows, never its own chrome: a
  // block taller than its share windows the lines instead of growing further.
  const share = Math.floor(room / INPUT_LINE_SHARE);
  const cap = Math.min(Math.max(share, INPUT_MIN_LINES) + INPUT_CHROME_ROWS, room);
  const wantedInput = Math.min(wantedInputRows, cap);
  // The band takes its rows exactly or not at all: a body too short for it beside
  // the input block keeps the input and shows no band. A footer held to the rows
  // it has gives the band them first, because the panel a band carries owns the
  // keyboard and the composer can wait for it to close.
  const bandFirst = ceiling < available;
  const hintRows =
    wantedHintRows > 0 &&
    (bandFirst ? wantedHintRows <= room : wantedHintRows + wantedInput <= room)
      ? wantedHintRows
      : 0;
  // The input block is all or nothing above its own frame and the line inside
  // it: a block given fewer rows paints its frame with the draft's line outside
  // it, so a body too short for both hides the composer and leaves its rows to
  // the band and the edge.
  const inputRows = fittedInput(
    bandFirst ? Math.max(0, room - hintRows) : room,
    wantedInput,
    // A session's block carries its label and its status row inside the frame; a
    // welcome's composer is a bare frame around the line.
    (mode === "welcome" ? COMPOSER_CHROME_ROWS : INPUT_CHROME_ROWS) + 1,
  );
  const free = Math.max(0, room - hintRows - inputRows);
  // The edge belongs to the window, not to the step: it keeps its rows so the
  // footer never moves while a run streams and commits. Only the welcome pane
  // takes the whole window, because nothing has been printed above it yet.
  const liveRows = mode === "welcome" ? free : Math.min(free, EDGE_ROWS);
  const body = chromeRows + detailRows + hintRows + liveRows + inputRows;
  const natural = Math.min(terminalHeight, Math.min(available, body) + docked);
  // The window reaches the terminal's last row, and the frame inside it keeps
  // only the rows it spends: the rows a short print left above are the page's,
  // so a session shows the same tray whatever the print reached. The edge's
  // first row stands where the print's next row goes, so the transcript and the
  // step in flight meet on one line.
  const reached = Math.min(printedRows, terminalHeight);
  const wanted = natural + Math.max(0, terminalHeight - reached - natural);
  const rows = Math.min(wanted, Math.max(1, maxRows));
  const slackRows = Math.max(0, rows - natural);
  const margin = mode === "welcome" ? FOOTER_MARGIN : SESSION_MARGIN;
  const indent =
    mode === "welcome" ? CONTENT_INDENT : CONTENT_INDENT + (SESSION_MARGIN - FOOTER_MARGIN);
  return {
    windowColumns: width,
    columns: Math.max(0, width - margin * 2),
    rows,
    body,
    chromeRows,
    liveRows,
    hintRows,
    inputRows,
    detailRows,
    slackRows,
    contentColumns: Math.max(0, width - indent * 2),
    dockedRows: docked,
  };
}

/**
 * Returns the window a page of its own owns.
 *
 * The session browser is that page: it replaces the tray while it is open, so
 * the footer holds nothing but the rows the page asks for and the transcript
 * above it keeps the rows a print left it. `rows` is the page and the rows it
 * closes with, and `keepRows` is the window the tray had: a page that keeps it
 * changes no height, and a footer that changes height scrolls its region and
 * leaves the rows above a shrink with the paint they had.
 */
export function pagePlan(
  width: number,
  terminalHeight: number,
  rows: number,
  keepRows = 0,
): FooterLayout {
  const region = Math.min(terminalHeight, Math.max(Math.max(0, rows), keepRows));
  return {
    windowColumns: width,
    columns: Math.max(0, width - FOOTER_MARGIN * 2),
    rows: region,
    body: 0,
    chromeRows: 0,
    liveRows: 0,
    hintRows: 0,
    inputRows: 0,
    detailRows: 0,
    slackRows: 0,
    contentColumns: Math.max(0, width - CONTENT_INDENT * 2),
    dockedRows: Math.max(0, region - SESSION_MARGIN),
  };
}

/** Returns the rows one chunk of a print may carry above the footer. */
export function printRoom(terminalHeight: number, footerHeight: number): number {
  return Math.max(1, Math.trunc(terminalHeight) - Math.trunc(footerHeight));
}

/** The rows the welcome's open page spends: its bar, its hairline, and the margin below. */
export const WELCOME_CHROME_ROWS = 3;

/** The rows a session's frame spends: its bottom edge and the margin below it. */
export const SESSION_CHROME_ROWS = 2;
