//! The footer's geometry: one pure function from a window to the rows and
//! columns of each region of the live band under the transcript.
//!
//! The transcript itself is printed into the terminal's own scrollback, so what
//! the window keeps for the application is a footer: the hint band or the theme
//! picker's panel, the step in flight, the input block, and the notice line,
//! inside the same rounded frame the chat panel had. The frame sits on the
//! canvas with a one-cell margin above it, and the footer's last row is the
//! window's last row.
//!
//! The input block takes the rows its buffer needs, one per line beside its own
//! chrome, up to a share of the body; a buffer taller than its share windows its
//! lines instead of growing, so the step in flight keeps the rows the block does
//! not need. The hint region takes its rows exactly or not at all: a body that
//! cannot hold the whole band beside the input block would have had without it
//! shows none of it, because the region yields to the input the user is typing
//! into.
//!
//! The numbers are the ones the revue application computed by hand, kept as they
//! were: Yoga lays the footer out, but the row budget is decided here so a frame
//! is the same at every width and height.

/** The columns and rows the window keeps around the footer. */
export const PANEL_MARGIN = 1;

/** The columns the footer needs to say anything at all. */
export const MIN_COLUMNS = 8;

/** The rows and columns the footer's frame takes. */
export const PANEL_FRAME = 2;

/** The columns the footer pads its content with. */
export const PANEL_PADDING = 1;

/** The rows the notice or error line occupies under the input block. */
export const DETAIL_ROWS = 1;

/** The share of the footer's body the input block may show buffer lines for. */
export const INPUT_LINE_SHARE = 3;

/** The buffer lines the input block shows even in a body too short for its share. */
export const INPUT_MIN_LINES = 3;

/** The rows the input block's own chrome takes: its frame, badge, and status. */
export const INPUT_CHROME_ROWS = 4;

/** The rows the input block occupies for a one-line buffer. */
export const MIN_BLOCK_ROWS = INPUT_CHROME_ROWS + 1;

/** The rows the band above the input block keeps for the step in flight. */
export const LIVE_BAND_ROWS = 6;

/** The columns one vertical frame edge takes. */
const FRAME_EDGE = 1;

/** The columns the printed transcript and the footer's content start at. */
export const CONTENT_INDENT = PANEL_MARGIN + FRAME_EDGE + PANEL_PADDING;

/** What the footer's live region holds this frame. */
export type FooterMode =
  /** No session is open: the welcome pane fills the region. */
  | "welcome"
  /** A session is open: the region keeps its band for the step in flight. */
  | "session";

/** The geometry of the footer in one window. */
export interface FooterLayout {
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
}

/** How the welcome region spends its rows, top to bottom. */
export interface WelcomeLayout {
  /** The blank rows above the lockup. */
  readonly lead: number;
  /** The rows the lockup occupies. */
  readonly logo: number;
  /** The blank rows between the lockup and the overview. */
  readonly filler: number;
  /** The rows the overview occupies. */
  readonly overview: number;
}

/**
 * Returns how the footer spends a window `terminalHeight` rows tall.
 *
 * `docked` is the rows a surface below the footer takes - the session browser and
 * the gap above it - so the footer leaves those rows to the surface.
 */
export function footerLayout(
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
    columns: Math.max(0, width - PANEL_MARGIN * 2),
    rows: natural + slack,
    body: body + slack,
    chromeRows,
    liveRows: liveRows + slack,
    hintRows,
    inputRows,
    detailRows,
    contentColumns: Math.max(0, width - CONTENT_INDENT * 2),
  };
}

/** Returns how the welcome region spends `rows` rows. */
export function welcomeLayout(rows: number): WelcomeLayout {
  const overview = Math.min(rows, OVERVIEW_ROWS);
  const above = rows - overview;
  const lockup = Math.min(above, LOCKUP_ROWS);
  const lead = lockup >= LOCKUP_ROWS ? LOGO_LEAD_ROWS : 0;
  return { lead, logo: lockup - lead, filler: above - lockup, overview };
}

/** The rows the welcome's frame spends: its two edges and the margin above and below. */
export const WELCOME_CHROME_ROWS = 4;

/** The rows a session's frame spends: its bottom edge and the margin below it. */
export const SESSION_CHROME_ROWS = 2;

/** The rows above the welcome's lockup: margin, frame, padding. */
const LOGO_LEAD_ROWS = 1;

/** The rows the lockup itself occupies. */
const LOGO_ROWS = 1;

/** The rows the lockup area occupies when the region has room for it. */
const LOCKUP_ROWS = LOGO_LEAD_ROWS + LOGO_ROWS;

/** The rows the framed overview occupies: its top edge, its row, its bottom edge. */
const OVERVIEW_ROWS = 3;

/** Returns the rows the input block needs for a buffer with `lineBreaks` breaks. */
export function inputBlockRows(lineBreaks: number): number {
  return MIN_BLOCK_ROWS + Math.max(0, lineBreaks);
}

/** The fewest rows the docked browser panel occupies, however small the result. */
export const MIN_PANEL_ROWS = 6;

/** The fewest rows the footer keeps above the docked browser and its gap. */
export const MIN_CHAT_ROWS = 11;

/** The blank canvas row the footer keeps above the docked browser. */
export const PANEL_GAP_ROWS = 1;

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
export function browserLayout(
  width: number,
  height: number,
  rowCount: number,
): BrowserLayout | null {
  if (width <= PANEL_FRAME) {
    return null;
  }
  const chrome = PANEL_FRAME + BROWSER_CHROME_ROWS + BROWSER_FOOTER_ROWS;
  const cap = Math.max(MIN_PANEL_ROWS, Math.floor(height / 2));
  const dataCap = Math.max(1, cap - chrome);
  const dataRows = Math.min(Math.max(rowCount, 1), dataCap);
  const rows = Math.min(Math.max(chrome + dataRows, MIN_PANEL_ROWS), cap);
  if (height < rows + PANEL_GAP_ROWS + MIN_CHAT_ROWS) {
    return null;
  }
  return { rows, dataRows, contentColumns: width - BROWSER_CHROME_COLUMNS };
}
