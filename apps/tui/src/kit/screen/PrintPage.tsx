//! The page a print paints: its frame, its lead, and the blocks it carries.

import { createPortal } from "@opentui/react";
import type { ScrollbackSurface } from "@opentui/core";
import type { ReactNode } from "react";
import { ContentColumns } from "../layout/context";
import { CONTENT_INDENT, FOOTER_MARGIN } from "../layout/footer-plan";
import { Surface } from "../layout/Surface";
import type { PrintJob, PrintPalette, PrintedBlock } from "./print-types";

/** The rows the page under a print is painted for before the surface measures it. */
const PAGE_ROWS = 400;

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
  // The print continues the panel the footer closes: its rows carry the frame's
  // sides, the first print of a session carries the frame's top edge, and a
  // session switch closes the frame above it first. The page under them is
  // painted a row at a time, so no row of the transcript is the terminal's own
  // background, and it is exactly as tall as the print.
  const rows = job.rows > 0 ? job.rows : PAGE_ROWS;
  const frameColumns = surface.width - FOOTER_MARGIN * 2;
  const innerColumns = frameColumns - 2;
  return createPortal(
    <box flexDirection="column" width={surface.width}>
      <box position="absolute" top={0} left={0} width={surface.width} height={rows}>
        <Surface rows={rows} columns={surface.width} color={palette.canvas} />
      </box>
      {job.lead.closing ? (
        <box flexDirection="row" height={1}>
          <box width={FOOTER_MARGIN} />
          <box
            width={frameColumns}
            borderStyle="rounded"
            border={["bottom"]}
            borderColor={palette.border}
          />
        </box>
      ) : null}
      <box flexDirection="row">
        <box width={FOOTER_MARGIN} />
        <box
          width={frameColumns}
          flexDirection="column"
          borderStyle="rounded"
          border={job.lead.top ? ["top", "left", "right"] : ["left", "right"]}
          borderColor={palette.border}
          paddingLeft={1}
          paddingRight={1}
        >
          <box position="absolute" top={0} left={0} width={innerColumns} height={rows}>
            <Surface rows={rows} columns={innerColumns} color={palette.panel} />
          </box>
          <ContentColumns columns={surface.width - CONTENT_INDENT * 2}>
            {job.lead.separator ? <box height={1} /> : null}
            {job.blocks.map((block, index) => (
              <box key={block.key} flexDirection="column">
                {index === 0 ? null : <box height={1} />}
                {renderBlock(block)}
              </box>
            ))}
          </ContentColumns>
        </box>
        <box width={FOOTER_MARGIN} />
      </box>
    </box>,
    surface.root,
    null,
  );
}
