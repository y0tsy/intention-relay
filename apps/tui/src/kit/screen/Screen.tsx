//! The screen: the footer, and the rows the printer has given back above it.
//!
//! The window paints one box. The footer comes first and reserves its rows
//! before the printer writes the rows that belong above them, so a block is
//! never committed against a window that has not shrunk for it yet. The rows the
//! printer takes are state here, not in the application: a surface reports a
//! print and the footer reads the total.

import { type ReactNode, useCallback, useState } from "react";

/** Renders the live footer and the printer in the window's own box. */
export function Screen({
  background,
  footer,
  printer,
}: {
  /** The wash of the rows no surface paints. */
  background: string;
  /** Renders the footer, given the rows the transcript has printed. */
  footer: (printedRows: number) => ReactNode;
  /** Renders the printer, given the callback that reports a print's rows. */
  printer: (onPrinted: (rows: number) => void) => ReactNode;
}) {
  // The rows the transcript has printed above the footer: the band takes the
  // window rows the print does not reach, so the empty part of the window sits
  // between the transcript and the input and no blank row lands in the history.
  const [printedRows, setPrintedRows] = useState(0);
  const addPrinted = useCallback((rows: number) => {
    setPrintedRows((before) => before + rows);
  }, []);
  return (
    <box width="100%" height="100%" backgroundColor={background}>
      {footer(printedRows)}
      {printer(addPrinted)}
    </box>
  );
}
