//! The numbers a layout brick hands its children.
//!
//! A brick that draws a frame, a margin, or a hanging marker subtracts what it
//! draws and provides the rest to its content. The content names what it paints,
//! never how many columns or rows it was given: the width is a handle, and the
//! brick that owns the chrome is the one that computes it.

import { useRenderer } from "@opentui/react";
import { createContext, type ReactNode, useContext } from "react";
import { useWindow } from "../window";

/** The columns a brick's content has, or `null` above the first provider. */
const ColumnsContext = createContext<number | null>(null);

/** The rows a footer slot has, or `null` outside a footer. */
const SlotRowsContext = createContext<number | null>(null);

/** The rows the transcript has printed above the footer. */
const PrintedRowsContext = createContext(0);

/** Provides the columns a region's content has. */
export function ContentColumns({ columns, children }: { columns: number; children?: ReactNode }) {
  return <ColumnsContext.Provider value={columns}>{children}</ColumnsContext.Provider>;
}

/** Provides the rows of one footer slot. */
export function SlotRows({ rows, children }: { rows: number; children?: ReactNode }) {
  return <SlotRowsContext.Provider value={rows}>{children}</SlotRowsContext.Provider>;
}

/** Provides the rows the transcript has printed. */
export function PrintedRows({ rows, children }: { rows: number; children?: ReactNode }) {
  return <PrintedRowsContext.Provider value={rows}>{children}</PrintedRowsContext.Provider>;
}

/**
 * Returns the columns the current content has.
 *
 * A component painted outside a brick reads the window's own width, which is
 * what a test renderer hands it; the application always sits inside a provider.
 */
export function useContentColumns(): number {
  const inherited = useContext(ColumnsContext);
  const { columns } = useWindow();
  return inherited ?? columns;
}

/**
 * Returns the columns the current content has without listening for a resize.
 *
 * A row of a printed block is painted once and keeps the width it was written
 * at, so it has no reason to follow the window; a block that did would add a
 * resize listener of its own for every block on the page.
 */
export function useStaticColumns(): number {
  const inherited = useContext(ColumnsContext);
  const renderer = useRenderer();
  return inherited ?? renderer.width;
}

/**
 * Returns the rows the footer takes now.
 *
 * The renderer's own height for the footer is the one the last frame asked for,
 * which is the ceiling the next frame plans against: a page that has printed a
 * row cannot grow its footer without scrolling that row out of the scrollback.
 */
export function useFooterRows(): number {
  const renderer = useRenderer();
  return renderer.footerHeight > 0 ? renderer.footerHeight : renderer.height;
}

/** Returns the rows of the footer slot the component sits in. */
export function useSlotRows(fallback = 0): number {
  return useContext(SlotRowsContext) ?? fallback;
}

/** Returns the rows the transcript has printed above the footer. */
export function usePrintedRows(): number {
  return useContext(PrintedRowsContext);
}
