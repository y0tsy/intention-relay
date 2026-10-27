//! The renderer's split-footer surface, behind the kernel's own handles.
//!
//! The renderer moves the surface where its output started whenever a print
//! lands or the footer resizes, and keeps the queue that decides when a frame
//! has taken the rows a print gave back. Both are private to the renderer, so
//! the casts live here and nowhere else: the printer asks a question, the
//! surface module answers it.

import type { CliRenderer } from "@opentui/core";

/** The renderer fields the split-footer printer reads. */
interface SplitRenderer {
  readonly terminalHeight: number;
  readonly footerHeight: number;
  readonly renderOffset?: number;
  readonly pendingNativeMode?: unknown;
  forceFullRepaintRequested?: boolean;
  setRenderOffset?: (offset: number) => void;
  resetSplitFooterForReplay?: (options: { clearSavedLines?: boolean }) => void;
}

/** Returns the renderer as the split-footer surface it is. */
function split(renderer: CliRenderer): SplitRenderer {
  return renderer as unknown as SplitRenderer;
}

/** The rows the footer plan asked the window to reserve in the last frame. */
let footerRows = 0;

/**
 * Publishes the rows the footer's plan asks the window to reserve.
 *
 * The renderer's own `footerHeight` reads the requested height, not the applied
 * one, so the printer cannot take it for the region's place. The plan is the
 * only source that knows how many rows the footer should have taken, and this
 * is where it is handed to the printer that waits for those rows.
 */
export function requestFooterRows(rows: number): void {
  footerRows = Math.max(0, Math.trunc(rows));
}

/** Returns the rows the footer's plan asked for last. */
export function requestedRows(): number {
  return footerRows;
}

/**
 * Puts the split surface back on the window's last row.
 *
 * A commit leaves the surface where its output started, so the region would sit
 * as many rows above the bottom as were just printed and the rows below it stay
 * the terminal's own background. The pinned offset is the one the renderer uses
 * for a frame it presents on its own.
 */
export function pinSurface(renderer: CliRenderer): void {
  const surface = split(renderer);
  const offset = Math.max(0, surface.terminalHeight - surface.footerHeight);
  try {
    // A surface that moves carries its paint to another row. The renderer
    // paints the cells that differ from the frame before, and those cells are
    // the old rows, so a move without a full repaint leaves the rows the
    // surface left behind with the paint they had.
    if (surface.renderOffset !== offset) {
      surface.forceFullRepaintRequested = true;
    }
    surface.setRenderOffset?.(offset);
  } catch {
    // A pin that lands while the terminal is busy dies quietly: the frame it
    // follows heals the surface on its own repaint.
  }
}

/**
 * Reports whether the surface stands where a footer of `rows` rows puts it.
 *
 * The renderer's `footerHeight` returns the height a pending frame asked for,
 * not the one it applied, and a shrink keeps the surface's old offset while it
 * scrolls the viewport instead. `renderOffset` is the one value the renderer
 * records once the region is in its place. A write before that is written above
 * a footer that still holds the rows, which scrolls the whole screen and leaves
 * the row the print stopped on as an unpainted gap.
 *
 * A renderer that keeps no offset in reach is one that cannot move it either:
 * the print proceeds rather than waiting for a signal that never comes.
 */
export function windowAtRows(renderer: CliRenderer, rows: number): boolean {
  const at = split(renderer);
  if ((at.pendingNativeMode ?? null) !== null) {
    return false;
  }
  if (typeof at.renderOffset !== "number") {
    return true;
  }
  const top = Math.max(0, Math.trunc(at.terminalHeight) - Math.max(0, Math.trunc(rows)));
  return at.renderOffset === top;
}

/**
 * Clears the window without touching the rows above it.
 *
 * `resetSplitFooterForReplay` is the renderer's own way back to a known screen:
 * it homes the cursor, erases what is on screen, and leaves the saved lines
 * alone, which is what a window whose rows have drifted needs.
 */
export function replayWindow(renderer: CliRenderer): void {
  try {
    split(renderer).resetSplitFooterForReplay?.({ clearSavedLines: false });
  } catch {
    // A window that cannot be replayed is repainted by the frames that follow.
  }
}
