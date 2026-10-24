//! The split-footer renderer: the process's one window.
//!
//! The window keeps the last rows of the screen for the footer and leaves the
//! rows above to the terminal, which is what lets the wheel, the scrollbar, and
//! a drag belong to the terminal. The renderer's own footer starts at the whole
//! window so the first frame paints the welcome, and every frame after it takes
//! the rows the footer reports.

import { type CliRenderer, createCliRenderer } from "@opentui/core";

/** Opens the one renderer this process paints with. */
export async function createSplitScreen(): Promise<CliRenderer> {
  // The window is a footer: the transcript is printed into the terminal's own
  // scrollback, so the wheel, the scrollbar, and a drag belong to the terminal
  // and the application never asks for the mouse. The terminal's saved lines are
  // never cleared: the rows the app printed are the session's history.
  const renderer = await createCliRenderer({
    exitOnCtrlC: false,
    useMouse: false,
    screenMode: "split-footer",
    footerHeight: 12,
    clearOnShutdown: false,
  });
  renderer.footerHeight = renderer.terminalHeight;
  return renderer;
}

/**
 * Leaves the terminal while keeping what it has printed.
 *
 * The frame's bottom edge is the window's last row, so the shell's prompt needs
 * a row of its own under it; the rows above stay in the terminal's history and
 * the saved lines are never dropped.
 */
export async function closeScreen(renderer: CliRenderer): Promise<void> {
  renderer.destroy();
  await renderer.closed;
  process.stdout.write("\n");
}
