//! Printing finished blocks into the terminal's own scrollback.
//!
//! The hooks own when a print starts and how its chunks reach the terminal; the
//! page owns what the print looks like; this component only joins them. A block
//! reaches the terminal through a scrollback surface, so the printed rows are
//! the very component the live tail used and a step does not change appearance
//! when it commits.

import type { ReactNode } from "react";
import { PrintPage } from "./PrintPage";
import type { PrintPalette, PrintedBlock } from "./print-types";
import { usePrintSession } from "./use-print-session";

/** Prints the committed blocks the window has not printed yet. */
export function Printer<Block extends PrintedBlock>({
  session,
  blocks,
  palette,
  renderBlock,
  onPrinted,
}: {
  /** The session the blocks belong to; a switch closes the frame above them. */
  session: string | null;
  /** The blocks waiting to print, in transcript order. */
  blocks: readonly Block[];
  palette: PrintPalette;
  renderBlock: (block: Block) => ReactNode;
  /** Called with the rows a print takes, so the footer gives them back first. */
  onPrinted: (rows: number) => void;
}) {
  const { surface, job } = usePrintSession({ session, blocks, onPrinted });
  if (job === null) {
    return null;
  }
  return <PrintPage surface={surface} job={job} palette={palette} renderBlock={renderBlock} />;
}

export type { PrintPalette, PrintedBlock } from "./print-types";
