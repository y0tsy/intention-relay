//! The chat panel's geometry: one pure function from a window to the rows and
//! columns of each region inside the panel.
//!
//! The panel sits on the canvas with a one-cell margin, so the window's own
//! background stays visible around it. Inside its frame the body is spent
//! exactly on four regions - the transcript (or the welcome), the hint band or
//! the theme picker's panel, the input block, and the notice line - so no child
//! ever overflows the panel and nothing is painted over anything.
//!
//! The input block takes the rows its buffer needs, one per line beside its own
//! chrome, up to a share of the body; a buffer taller than its share windows its
//! lines instead of growing, so the transcript keeps the rows the block does not
//! need. The hint region takes its rows exactly or not at all: a body that cannot
//! hold the whole band beside the rows the input block would have had without it
//! shows none of it, because the region yields to the input the user is typing
//! into.
//!
//! The numbers are the ones the revue application computed by hand, kept as they
//! were: Yoga lays the panel out, but the row budget is decided here so a frame
//! is the same at every width and height.

/** The columns and rows the window keeps around the chat panel. */
export const PANEL_MARGIN = 1;

/** The rows and columns the panel's frame takes. */
export const PANEL_FRAME = 2;

/** The columns the panel pads its content with. */
export const PANEL_PADDING = 1;

/** The rows the notice or error line occupies under the input block. */
export const DETAIL_ROWS = 1;

/** The share of the panel's body the input block may show buffer lines for. */
export const INPUT_LINE_SHARE = 3;

/** The buffer lines the input block shows even in a body too short for its share. */
export const INPUT_MIN_LINES = 3;

/** The rows the input block's own chrome takes: its frame, badge, and status. */
export const INPUT_CHROME_ROWS = 4;

/** The rows the input block occupies for a one-line buffer. */
export const MIN_BLOCK_ROWS = INPUT_CHROME_ROWS + 1;

/** The rows above the transcript's first content row: margin, frame, padding, frame. */
export const TRANSCRIPT_TOP_ROWS = PANEL_MARGIN + 1 + PANEL_PADDING + 1;

/** The blank rows the welcome pane keeps above its lockup. */
const LOGO_LEAD_ROWS = 1;

/** The rows the lockup itself occupies. */
const LOGO_ROWS = 1;

/** The rows the lockup area occupies when the region has room for it. */
const LOCKUP_ROWS = LOGO_LEAD_ROWS + LOGO_ROWS;

/** The rows the framed overview occupies: its top edge, its row, its bottom edge. */
const OVERVIEW_ROWS = 3;

/** The geometry of the chat panel in one window. */
export interface PanelLayout {
  /** The columns the panel occupies. */
  readonly columns: number;
  /** The rows the panel occupies. */
  readonly rows: number;
  /** The rows inside the panel's frame. */
  readonly body: number;
  /** The rows the transcript pane occupies. */
  readonly transcriptRows: number;
  /** The rows above the input block: the band, or the picker's panel. */
  readonly hintRows: number;
  /** The rows the input block occupies. */
  readonly inputRows: number;
  /** The rows the notice or error line occupies. */
  readonly detailRows: number;
  /** The columns the panel's content has. */
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

/** Returns the geometry of the chat panel in one window. */
export function panelLayout(
  width: number,
  height: number,
  wantedInputRows: number,
  wantedHintRows: number,
): PanelLayout {
  const columns = Math.max(0, width - PANEL_MARGIN * 2);
  const rows = Math.max(0, height - PANEL_MARGIN * 2);
  const body = Math.max(0, rows - PANEL_FRAME);
  const detailRows = body > 0 ? DETAIL_ROWS : 0;
  const available = Math.max(0, body - detailRows);
  // The cap bounds the buffer lines the block shows, never its own chrome: a
  // block taller than its share windows the lines instead of growing further.
  const share = Math.floor(available / INPUT_LINE_SHARE);
  const lines = Math.max(share, INPUT_MIN_LINES);
  const cap = Math.min(lines + INPUT_CHROME_ROWS, available);
  const inputRows = Math.min(wantedInputRows, cap);
  // The band takes its rows exactly or not at all: a body too short for it beside
  // the input block keeps the input and shows no band.
  const hintRows =
    wantedHintRows > 0 && wantedHintRows + inputRows <= available ? wantedHintRows : 0;
  const transcriptRows = Math.max(0, available - hintRows - inputRows);
  return {
    columns,
    rows,
    body,
    transcriptRows,
    hintRows,
    inputRows,
    detailRows,
    contentColumns: Math.max(0, columns - (PANEL_FRAME + PANEL_PADDING * 2)),
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

/** Returns the rows the input block needs for a buffer with `lineBreaks` breaks. */
export function inputBlockRows(lineBreaks: number): number {
  return MIN_BLOCK_ROWS + Math.max(0, lineBreaks);
}

/** Reports whether a window is wide enough to draw a panel at all. */
export function panelFits(width: number, height: number): boolean {
  return width > PANEL_MARGIN * 2 + PANEL_FRAME && height > PANEL_MARGIN * 2 + PANEL_FRAME;
}

/** The fewest rows the docked browser panel occupies, however small the result. */
export const MIN_PANEL_ROWS = 6;

/** The fewest rows the chat panel keeps above the docked browser and its gap. */
export const MIN_CHAT_ROWS = 11;

/** The blank canvas row the chat panel keeps above the docked browser. */
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
 * up to half the window, so the chat above it keeps the majority, and it never
 * opens at all when the chat would fall below its own floor.
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
