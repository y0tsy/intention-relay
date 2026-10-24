//! How a print is laid out: the row it leads with and the chunks it goes in.
//!
//! A print is one bounded fold of blocks, so the surface that paints it holds
//! one block's rows and never a whole burst of them. The transcript is the
//! terminal's scrollback and carries no frame of its own, so a print leads with
//! a blank row between two blocks of the same session and nothing else.
//!
//! A block taller than the window is committed in chunks of the rows the window
//! can hold above the footer, so one native call never carries more rows than
//! the terminal can take.

/** The rows a print leads with, before the block's own rows. */
export interface Lead {
  /** The blank row between two blocks of the same session. */
  readonly separator: boolean;
}

/** Returns the row a print leads with. */
export function leadRow(printedSoFar: number): Lead {
  return { separator: printedSoFar > 0 };
}

/** Returns how many rows a chunk starting at `from` takes. */
export function chunkRows(from: number, rows: number, room: number): number {
  return Math.max(0, Math.min(Math.max(1, Math.trunc(room)), rows - from));
}
