//! How a print is laid out: the row it leads with, the rows it seals, and the
//! chunks it goes in.
//!
//! A print is one bounded fold of blocks, so the surface that paints it holds
//! one block's rows and never a whole burst of them. The transcript is the
//! terminal's scrollback and carries no frame of its own, so a print leads with
//! a blank row between two blocks of the same session and nothing else.
//!
//! The step in flight is printed as it streams: the page paints the text it has
//! so far, and every row a later row has superseded is final. The last rows a
//! stream paints may still be rewritten - a table's delimiter or a setext
//! underline arrives after the row it reinterprets - so the edge holds them
//! until the stream moves past them or the step commits.
//!
//! A block taller than the window is committed in chunks of the rows the window
//! can hold above the footer, so one native call never carries more rows than
//! the terminal can take.

import type { StepPrint } from "./print-types";

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

/**
 * Returns the rows of a stream that are final, its `edge` last rows excluded.
 *
 * A row is final once a later row exists: the stream only ever appends, so the
 * rows above the edge are the rows it can no longer rewrite.
 */
export function sealedRows(rows: number, edge: number): number {
  return Math.max(0, Math.trunc(rows) - Math.max(0, Math.trunc(edge)));
}

/**
 * Reports whether `after` is the same step as `before`, grown.
 *
 * A step only ever appends to its two channels, so a step whose reasoning or
 * answer no longer starts with the text already painted is another step: the
 * print seals what it painted and the new step opens a page of its own.
 */
export function continues(before: StepPrint, after: StepPrint): boolean {
  return after.reasoning.startsWith(before.reasoning) && after.text.startsWith(before.text);
}
