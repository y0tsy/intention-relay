//! How a print is laid out: the row it leads with and the chunks it goes in.
//!
//! A print is one block, so the surface that paints it holds one block's rows and
//! never a whole burst of them: a session that satisfies sixty rows in a second
//! prints sixty bounded jobs instead of one frame the terminal cannot take.
//!
//! Every job leads with the row that keeps the window's frame whole: the frame's
//! top edge on the first print of a session, the closing edge of the previous
//! session's frame plus a new top edge when the session changed, and a blank row
//! between two blocks of the same session.
//!
//! A block taller than the window is committed in chunks of the rows the window
//! can hold above the footer, so one native call never carries more rows than
//! the terminal can take.

/** The rows a print leads with, before the block's own rows. */
export interface Lead {
  /** The blank row between two blocks of the same session. */
  readonly separator: boolean;
  /** The top edge of a frame: the session's first print. */
  readonly top: boolean;
  /** The closing edge of the frame the previous session left open. */
  readonly closing: boolean;
}

/** Returns the row a print leads with. */
export function leadRow(printedSoFar: number, sessionChanged: boolean): Lead {
  return {
    separator: printedSoFar > 0 && !sessionChanged,
    top: printedSoFar === 0 || sessionChanged,
    closing: sessionChanged,
  };
}

/** Returns the rows `lead` occupies. */
export function leadRows(lead: Lead): number {
  return (lead.separator ? 1 : 0) + (lead.top ? 1 : 0) + (lead.closing ? 1 : 0);
}

/**
 * Returns the row counts one print of `rows` rows goes in.
 *
 * Every chunk fits `room` rows, and the last one carries the remainder, so a
 * block of a thousand rows reaches the terminal as a sequence of calls the
 * terminal can take rather than as one call it cannot.
 */
export function chunks(rows: number, room: number): number[] {
  if (rows < 1) {
    return [];
  }
  const size = Math.max(1, Math.trunc(room));
  const parts: number[] = [];
  for (let left = Math.trunc(rows); left > 0; left -= size) {
    parts.push(Math.min(size, left));
  }
  return parts;
}

/** Returns how many rows a chunk starting at `from` takes. */
export function chunkRows(from: number, rows: number, room: number): number {
  return Math.max(0, Math.min(Math.max(1, Math.trunc(room)), rows - from));
}
