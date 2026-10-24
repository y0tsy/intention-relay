//! One display width for every surface.
//!
//! A terminal measures text in columns, and a wide character takes two. The
//! kernel measures every wrap point, clip, and pad with the same helper, so a
//! row a block counts is the row the renderer paints.

import { measureEditText } from "@opentui/core";

/** Returns the display columns a string occupies. */
export function displayWidth(text: string): number {
  return measureEditText(text, { widthMethod: "unicode" });
}

/** Returns one marker's glyph and the text it opens, separated by the gap. */
export function markerText(glyph: string, text: string, gap = 1): string {
  return `${glyph}${" ".repeat(Math.max(0, gap))}${text}`;
}

/** Returns one cell padded to its column, never cut. */
export function pad(text: string, columns: number): string {
  const width = displayWidth(text);
  return width >= columns ? text : `${text}${" ".repeat(columns - width)}`;
}

/**
 * Returns one cell cut to its column, with an ellipsis when it was cut.
 *
 * The cut counts display cells, not code points: a wide glyph is dropped whole
 * rather than allowed to overflow the cell it was measured for.
 */
export function truncate(text: string, columns: number): string {
  if (columns <= 0) {
    return "";
  }
  if (displayWidth(text) <= columns) {
    return text;
  }
  let kept = "";
  for (const character of text) {
    if (displayWidth(kept + character) > columns - 1) {
      break;
    }
    kept += character;
  }
  return `${kept}…`;
}

/** Returns one cell cut to its column and padded to it on the left. */
export function cellText(text: string, columns: number): string {
  return pad(truncate(text, columns), columns);
}

/** Returns one cell cut to its column and padded to it on the right. */
export function cellTextRight(text: string, columns: number): string {
  const kept = truncate(text, columns);
  return `${" ".repeat(Math.max(0, columns - displayWidth(kept)))}${kept}`;
}

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

/** Returns the rows a buffer's text takes, one per line. */
export function bufferLines(text: string): string[] {
  return text.split("\n");
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
