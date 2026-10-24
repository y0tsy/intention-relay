//! The window an application paints into.
//!
//! The render region is the footer, so the window's own height comes from the
//! terminal itself: a surface that reads it here never has to know which part of
//! the terminal the renderer owns.

import { useRenderer, useTerminalDimensions } from "@opentui/react";

/** One terminal window: the columns and rows the application sees. */
export interface Window {
  /** The terminal's own width. */
  readonly columns: number;
  /** The terminal's own height, not the render region's. */
  readonly rows: number;
}

/** Returns the window the application paints into. */
export function useWindow(): Window {
  const { width } = useTerminalDimensions();
  const renderer = useRenderer();
  return { columns: width, rows: renderer.terminalHeight };
}
