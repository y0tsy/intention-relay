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
  readonly externalOutputQueue?: { readonly size: number };
  setRenderOffset?: (offset: number) => void;
  resetSplitFooterForReplay?: (options: { clearSavedLines?: boolean }) => void;
}

/** Returns the renderer as the split-footer surface it is. */
function split(renderer: CliRenderer): SplitRenderer {
  return renderer as unknown as SplitRenderer;
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
    surface.setRenderOffset?.(offset);
  } catch {
    // A pin that lands while the terminal is busy dies quietly: the frame it
    // follows heals the surface on its own repaint.
  }
}

/** Returns the surface's row, or null when the renderer keeps it out of reach. */
export function readOffset(renderer: CliRenderer): number | null {
  const at = split(renderer).renderOffset;
  return typeof at === "number" ? at : null;
}

/**
 * Reports whether the window has taken the rows a print gave it back.
 *
 * The renderer applies a footer's new rows on a frame of its own, and only when
 * the split output queue has drained: a print written before that frame is
 * written above a footer that still holds the rows, which scrolls the whole
 * screen and leaves a copy of the footer the scroll moved.
 */
export function windowSettled(renderer: CliRenderer): boolean {
  const at = split(renderer);
  return (at.pendingNativeMode ?? null) === null && (at.externalOutputQueue?.size ?? 0) === 0;
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
