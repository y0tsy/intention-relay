//! Printing committed blocks and the step in flight into the terminal's own
//! scrollback.
//!
//! The hooks own when a print starts and how its chunks reach the terminal; the
//! page owns what the print looks like; this component only joins them. A block
//! and the step in flight reach the terminal through the same page, so a step
//! does not change appearance when it commits.

import type { ReactNode } from "react";
import { PrintPage } from "./PrintPage";
import type { PrintPalette, PrintedBlock, StepPrint } from "./print-types";
import { usePrintSession } from "./use-print-session";

/** Prints the blocks the window has not printed yet, and the step in flight. */
export function Printer<Block extends PrintedBlock>({
  session,
  blocks,
  step,
  palette,
  renderBlock,
  renderStream,
  onPrinted,
}: {
  /** The session the blocks belong to; a switch closes the frame above them. */
  session: string | null;
  /** The blocks waiting to print, in transcript order. */
  blocks: readonly Block[];
  /** The step in flight, or `null` when nothing streams. */
  step: StepPrint | null;
  palette: PrintPalette;
  renderBlock: (block: Block) => ReactNode;
  /** Paints one snapshot of the step in flight. */
  renderStream: (step: StepPrint) => ReactNode;
  /** Called with the rows a print takes, so the footer gives them back first. */
  onPrinted: (rows: number) => void;
}) {
  const { surface, job } = usePrintSession({ session, blocks, step, onPrinted });
  if (job === null) {
    return null;
  }
  // A step in flight paints the text it has now, not the snapshot the print
  // measured: the page and the edge above the footer read one source, so the
  // rows the edge holds back are exactly the rows the page has below its seal.
  const painted = job.open && step !== null ? step : job.step;
  return (
    <PrintPage surface={surface} separator={job.lead.separator} palette={palette}>
      {painted === null ? (
        <box flexDirection="column" gap={1}>
          {job.blocks.map((block) => (
            <box key={block.key}>{renderBlock(block)}</box>
          ))}
        </box>
      ) : (
        renderStream(painted)
      )}
    </PrintPage>
  );
}

export type { PrintPalette, PrintedBlock, StepPrint } from "./print-types";
