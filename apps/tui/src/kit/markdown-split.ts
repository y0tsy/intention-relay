//! Splitting a streamed answer where it is still being written.
//!
//! An answer arrives a token at a time and its beginning never changes, so the
//! split lets a view parse the settled prefix once and re-parse only the open
//! tail on each frame. The split points are the blank lines a fence is not open
//! across: a half-written code block stays one block instead of being parsed as
//! prose and then re-parsed as code.

/** One streamed answer, split where it is still being written. */
export interface StreamSplit {
  /** The settled prefix, ending on the blank line that separates it. */
  readonly closed: string;
  /** The block still being written; empty once a blank line settled it. */
  readonly open: string;
}

/** Splits one streamed answer into its settled prefix and its open tail. */
export function splitBlocks(source: string): StreamSplit {
  const start = openStart(source);
  return { closed: source.slice(0, start), open: source.slice(start) };
}

/** Returns the index where the block still being written begins. */
function openStart(source: string): number {
  const fence = openFenceStart(source);
  if (fence !== null) {
    return fence;
  }
  // Without an open fence, everything after the last blank line is still being
  // written: a blank line is what settles a block.
  return lastBlankLineEnd(source) ?? 0;
}

/** Returns the index of the fence a stream has opened and not yet closed. */
function openFenceStart(source: string): number | null {
  let open: { readonly marker: string; readonly index: number } | null = null;
  let index = 0;
  for (const line of source.split("\n")) {
    const match = /^ {0,3}(`{3,}|~{3,})/.exec(line);
    if (match !== null) {
      const marker = (match[1] ?? "`").slice(0, 1);
      if (open === null) {
        open = { marker, index };
      } else if (open.marker === marker) {
        open = null;
      }
    }
    index += line.length + 1;
  }
  return open === null ? null : open.index;
}

/** Returns the index just past the last blank line, if there is one. */
function lastBlankLineEnd(source: string): number | null {
  const last = [...source.matchAll(/\n[ \t]*\n/g)].at(-1);
  return last === undefined ? null : (last.index ?? 0) + last[0].length;
}
