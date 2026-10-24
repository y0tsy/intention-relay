//! The page a print paints: the transcript's rule, and the blocks it carries.

import { createPortal } from "@opentui/react";
import type { ScrollbackSurface } from "@opentui/core";
import type { ReactNode } from "react";
import { ContentColumns } from "../layout/context";
import { Surface } from "../layout/Surface";
import type { PrintJob, PrintPalette, PrintedBlock } from "./print-types";

/** The rows the page under a print is painted for before the surface measures it. */
const PAGE_ROWS = 400;

/** The columns the transcript's rule occupies, its one-column margin included. */
export const RULE_COLUMNS = 4;

/** The column between the rule and the text. */
const RULE_GAP = 1;

/** The columns a printed row's rule and gap take before its text. */
export const TRANSCRIPT_INDENT = RULE_COLUMNS + RULE_GAP;

/** The columns the text keeps on the right, as the footer's input does. */
const RIGHT_MARGIN = 4;

/** Returns the columns one printed row's text has. */
export function transcriptColumns(width: number): number {
  return Math.max(0, width - RULE_COLUMNS - RULE_GAP - RIGHT_MARGIN);
}

/** Paints the rule that runs beside every printed row. */
function Rule({ palette }: { palette: PrintPalette }) {
  return <box width={RULE_COLUMNS} border={["right"]} borderColor={palette.border} />;
}

/** Paints one measured print into the scrollback surface it belongs to. */
export function PrintPage<Block extends PrintedBlock>({
  surface,
  job,
  palette,
  renderBlock,
}: {
  surface: ScrollbackSurface;
  job: PrintJob<Block>;
  palette: PrintPalette;
  renderBlock: (block: Block) => ReactNode;
}) {
  // The transcript is the terminal's scrollback: the page is the terminal's own
  // background with one rule down its left side, and the rule reaches every row
  // the print carries so no gap in it marks where one print ended.
  const rows = job.rows > 0 ? job.rows : PAGE_ROWS;
  return createPortal(
    <box flexDirection="column" width={surface.width}>
      <box position="absolute" top={0} left={0} width={surface.width} height={rows}>
        <Surface rows={rows} columns={surface.width} color={palette.canvas} />
      </box>
      {job.lead.separator ? (
        <box flexDirection="row" height={1}>
          <Rule palette={palette} />
        </box>
      ) : null}
      <ContentColumns columns={transcriptColumns(surface.width)}>
        {job.blocks.map((block, index) => (
          <box key={block.key} flexDirection="row">
            <Rule palette={palette} />
            <box width={RULE_GAP} />
            <box flexDirection="column" flexGrow={1} paddingRight={RIGHT_MARGIN}>
              {index === 0 ? null : <box height={1} />}
              {renderBlock(block)}
            </box>
          </box>
        ))}
      </ContentColumns>
    </box>,
    surface.root,
    null,
  );
}
