//! What the application writes to the terminal before the renderer takes it.
//!
//! The window owns the last `rows` rows of the screen and the transcript lives
//! in the terminal's own scrollback, so whatever the shell printed above the
//! launch has to move out of the window's way without being erased: a session
//! that starts with the output still on screen paints the panel over it and the
//! text is gone for good.
//!
//! `pushScreen` reserves the window instead. A run of newlines scrolls the
//! visible screen into the terminal's history, the cursor returns to the top of
//! the freed rows, and the erase that follows clears only what is on screen
//! (`ED`, `CSI J`). The history keeps the old output, the window is empty, and
//! the beige page has nothing showing through it.
//!
//! `ED` is the only erase used here: `ED 3` (`\x1B[3J`) drops the saved lines
//! and would take the shell's output with it, and `ED 2` (`\x1B[2J`) without the
//! cursor move leaves the cursor on a row that is about to be cleared.

/** Returns the rows the window takes, never fewer than one. */
export function windowRows(rows: number | undefined): number {
  return Math.max(1, Math.trunc(rows ?? 0) || MIN_WINDOW_ROWS);
}

/** The rows the application assumes when the terminal does not say. */
const MIN_WINDOW_ROWS = 24;

/**
 * Returns the bytes that clear the window and keep the screen's history.
 *
 * `rows` newlines push the visible screen into the history, `CSI A` returns the
 * cursor to the top of the rows that entering them opened, and `CSI J` erases
 * from the cursor down: the window, and nothing above it.
 */
export function pushScreen(rows: number | undefined): string {
  const count = windowRows(rows);
  return `${"\n".repeat(count)}\x1B[${count}A\x1B[J`;
}
