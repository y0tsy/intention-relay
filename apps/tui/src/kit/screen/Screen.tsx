//! The screen: the footer, and the printer that writes above it.
//!
//! The window paints one box. The footer keeps the rows it needs, the terminal
//! keeps the rows the transcript printed above them, and the printer writes the
//! rows a block becomes into the terminal's own scrollback. The footer's height
//! follows that print: the rows above the tray are the page's until a print
//! takes them.

import { useState, type ReactNode } from "react";

/** Renders the live footer and the printer in the window's own box. */
export function Screen({
  background,
  footer,
  printer,
}: {
  /** The wash of the rows no surface paints. */
  background: string;
  /** The footer: the edge, the tray, and the rows they take. */
  footer: (printedRows: number) => ReactNode;
  /** The printer: the rows the transcript writes into the terminal. */
  printer: (onPrinted: (rows: number) => void) => ReactNode;
}) {
  // The rows the page has printed. The footer and the printer share it: a print
  // takes its rows out of the window before it writes them, so the tray's top
  // edge never moves under the row being printed.
  const [printedRows, setPrintedRows] = useState(0);
  const addPrinted = (rows: number): void => setPrintedRows((before) => before + rows);
  return (
    <box width="100%" height="100%" backgroundColor={background}>
      {footer(printedRows)}
      {printer(addPrinted)}
    </box>
  );
}
