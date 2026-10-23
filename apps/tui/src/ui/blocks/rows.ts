//! Laying plain text out into the display rows a block counts.
//!
//! A reasoning block and a tool preview are both measured before they are
//! painted: the block shows a bounded number of display rows and names the rows
//! it hid, so the count has to be the same one the terminal will paint. The
//! wrapper below is the transcript's one wrap rule - word wrap, with the
//! continuation held at the block's own text column - applied to a string that
//! is already indented.

import { displayWidth } from "../measure";

/**
 * Wraps one run of text into display rows at `columns`.
 *
 * A word wider than the column budget is cut rather than allowed to overflow,
 * because a display row the block did not count is a row the window loses.
 */
export function wrapRows(text: string, columns: number): string[] {
  const width = Math.max(1, columns);
  const rows: string[] = [];
  for (const line of text.split("\n")) {
    if (displayWidth(line) <= width) {
      rows.push(line);
      continue;
    }
    let row = "";
    for (const word of splitWords(line)) {
      const candidate = row + word;
      if (displayWidth(candidate) <= width) {
        row = candidate;
        continue;
      }
      if (row.trimEnd().length > 0) {
        rows.push(row.trimEnd());
      }
      row = " ".repeat(width);
      const trimmed = word.trimStart();
      if (displayWidth(trimmed) <= width) {
        row = trimmed;
        continue;
      }
      for (const piece of cutToWidth(trimmed, width)) {
        rows.push(piece);
        row = "";
      }
    }
    if (row.length > 0) {
      rows.push(row.trimEnd());
    }
  }
  return rows;
}

/**
 * Returns the first `visible` display rows of a run and how many it hid.
 *
 * A hidden row is a row the block does not paint, and the marker a bounded
 * block prints is the count this function returns.
 */
export function takeRows(
  text: string,
  columns: number,
  visible: number,
): { readonly shown: readonly string[]; readonly hidden: number } {
  const rows = wrapRows(text, columns);
  const shown = rows.slice(0, Math.max(0, visible));
  return { shown, hidden: rows.length - shown.length };
}

/** Splits one line into words that keep the blanks that followed them. */
function splitWords(line: string): string[] {
  return line.match(/\S+\s*|\s+/g) ?? [];
}

/** Cuts one word into pieces no wider than `columns`. */
function cutToWidth(word: string, columns: number): string[] {
  const pieces: string[] = [];
  let piece = "";
  for (const character of word) {
    if (displayWidth(piece + character) > columns) {
      pieces.push(piece);
      piece = character;
      continue;
    }
    piece += character;
  }
  if (piece.length > 0) {
    pieces.push(piece);
  }
  return pieces;
}

/** Returns one stable key per row, so a repeated row is still its own element. */
export function uniqueKeys(rows: readonly string[]): string[] {
  const seen = new Map<string, number>();
  return rows.map((row) => {
    const count = seen.get(row) ?? 0;
    seen.set(row, count + 1);
    return count === 0 ? row : `${row}#${count}`;
  });
}
