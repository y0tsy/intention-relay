//! The footer's frame: the page under it, the window margin, and the rows it
//! reserves for the renderer.
//!
//! A launch paints an open page: one header row, the welcome region, and the
//! composer on the page itself. An open session keeps the rounded frame that
//! closes under the input, because the transcript above it is the terminal's.
//! The frame is the one region that must agree with the renderer: its height is
//! the footer height, and the renderer has to know it before the printer writes
//! the rows above. The effect lives here, beside the box it measures, so a
//! surface never talks to the renderer about geometry.

import { useRenderer } from "@opentui/react";
import type { ReactNode } from "react";
import { ContentColumns } from "./context";
import {
  CONTENT_INDENT,
  FOOTER_MARGIN,
  FOOTER_MIN_COLUMNS,
  type FooterLayout,
  RULE_GAP,
  SESSION_MARGIN,
  transcriptColumns,
} from "./footer-plan";
import { Surface } from "./Surface";

/** The palette roles the frame paints. */
export interface FooterPalette {
  readonly canvas: string;
  readonly panel: string;
  readonly border: string;
  readonly headerBg: string;
}

/** Renders the live footer over the canvas, and the page it closes above. */
export function FooterFrame({
  layout,
  welcome,
  palette,
  header,
  live,
  page,
  children,
}: {
  layout: FooterLayout;
  /** A welcome paints the open page; a session keeps the closing frame. */
  welcome: boolean;
  palette: FooterPalette;
  /** The welcome's top bar, painted across the page above the content. */
  header?: ReactNode;
  /** The step in flight's unsealed edge, painted above a session's frame. */
  live?: ReactNode;
  /** A page that owns the whole footer: the panel, and the rows it paints. */
  page?: { readonly node: ReactNode; readonly rows: number };
  children?: ReactNode;
}) {
  const renderer = useRenderer();
  const rows = layout.rows;
  // The region is set as the frame is built, not after it: the renderer paints
  // the rows this frame spends, and a region applied after the frame stands the
  // tray too high, leaving the row above it with the paint it had. Setting it
  // applies the region and asks for the repaint itself.
  if (renderer.footerHeight !== rows) {
    renderer.footerHeight = rows;
    renderer.requestRender();
  }
  if (page !== undefined) {
    // A page of its own: the panel stands under one blank row and closes the
    // window above its margin. Nothing of the tray is painted, because the
    // panel owns the keyboard.
    return (
      <box flexDirection="column" height={rows} backgroundColor={palette.canvas} overflow="hidden">
        <box position="absolute" top={0} left={0} width={layout.windowColumns} height={rows}>
          <Surface rows={rows} columns={layout.windowColumns} color={palette.canvas} />
        </box>
        <box height={Math.max(0, rows - page.rows - FOOTER_MARGIN)} />
        <box height={page.rows}>{page.node}</box>
        <box height={FOOTER_MARGIN} />
      </box>
    );
  }
  if (layout.columns < FOOTER_MIN_COLUMNS || layout.body < 1) {
    // A window too narrow or too short to read anything in draws nothing rather
    // than a broken box: the region is the page and nothing of the tray.
    return <box width="100%" height={rows} backgroundColor={palette.canvas} />;
  }
  if (welcome) {
    // The bar and the hairline that closes it: a page's top edge, not a row of
    // its content.
    const headerRows = 2;
    const contentRows = Math.max(0, rows - layout.dockedRows - headerRows - FOOTER_MARGIN);
    return (
      <box flexDirection="column" height={rows} backgroundColor={palette.canvas} overflow="hidden">
        <box position="absolute" top={0} left={0} width={layout.windowColumns} height={rows}>
          <Surface rows={rows} columns={layout.windowColumns} color={palette.canvas} />
        </box>
        <box
          height={headerRows}
          backgroundColor={palette.headerBg}
          border={["bottom"]}
          borderColor={palette.border}
        >
          <box position="absolute" top={0} left={0} width={layout.windowColumns} height={1}>
            <Surface rows={1} columns={layout.windowColumns} color={palette.headerBg} />
          </box>
          <box flexDirection="row" height={1}>
            <box width={CONTENT_INDENT} />
            <box flexGrow={1}>
              <ContentColumns columns={layout.contentColumns}>{header}</ContentColumns>
            </box>
            <box width={CONTENT_INDENT} />
          </box>
        </box>
        <box flexDirection="row" height={contentRows}>
          <box width={CONTENT_INDENT} />
          <box flexDirection="column" flexGrow={1}>
            <ContentColumns columns={layout.contentColumns}>{children}</ContentColumns>
          </box>
          <box width={CONTENT_INDENT} />
        </box>
        <box height={FOOTER_MARGIN} />
      </box>
    );
  }
  const frameRows = Math.max(
    0,
    rows - layout.dockedRows - layout.chromeRows + 1 - layout.slackRows - layout.liveRows,
  );
  const innerRows = Math.max(0, frameRows - 1);
  return (
    // The region is the footer: a surface that wants more rows than the footer
    // has shows the rows it got rather than painting over the page above.
    <box flexDirection="column" height={rows} backgroundColor={palette.canvas} overflow="hidden">
      {/* The page is painted under every row of the region first: a row that
          shows no text is still a row of the page, not a hole through it. */}
      <box position="absolute" top={0} left={0} width={layout.windowColumns} height={rows}>
        <Surface rows={rows} columns={layout.windowColumns} color={palette.canvas} />
      </box>
      {/* The step in flight is the next rows of the transcript, not a row of
          the tray: its edge paints on the page above the frame, at the columns a
          printed row's text has, with the page's rail beside it. The rail is the
          column the tray's own rail sits in, so the frame closes from the first
          printed row into the tray without a gap. It stands directly under the
          last printed row: the page's empty middle belongs below it, or a block
          the printer is still writing would be split by blank rows. */}
      {layout.liveRows < 1 ? null : (
        <box flexDirection="row" height={layout.liveRows}>
          <box width={SESSION_MARGIN} />
          <box
            width={RULE_GAP}
            height={layout.liveRows}
            border={["right"]}
            borderColor={palette.border}
          />
          <box width={RULE_GAP} />
          <box flexGrow={1}>
            <ContentColumns columns={transcriptColumns(layout.windowColumns)}>
              {live}
            </ContentColumns>
          </box>
        </box>
      )}
      {/* The page's empty middle carries the same rail as the rows it stands
          between: the rule runs from the first printed row into the tray without
          a break, and a row a print left empty is a row of the page, not a hole
          through it. It sits under the edge, so the edge's first line is always
          the line the next print writes to. */}
      {layout.slackRows < 1 ? null : (
        <box flexDirection="row" height={layout.slackRows}>
          <box width={SESSION_MARGIN} />
          <box
            width={RULE_GAP}
            height={layout.slackRows}
            border={["right"]}
            borderColor={palette.border}
          />
        </box>
      )}
      <box flexDirection="row" height={frameRows}>
        <box width={SESSION_MARGIN} />
        <box
          width={layout.columns}
          height={frameRows}
          flexDirection="column"
          borderStyle="rounded"
          border={["left", "right", "bottom"]}
          borderColor={palette.border}
          backgroundColor={palette.panel}
          paddingLeft={1}
          paddingRight={1}
        >
          <box position="absolute" top={0} left={0} width={layout.columns} height={frameRows}>
            <Surface rows={innerRows} columns={layout.columns} color={palette.panel} />
          </box>
          <ContentColumns columns={layout.contentColumns}>{children}</ContentColumns>
        </box>
      </box>
      <box height={FOOTER_MARGIN} />
    </box>
  );
}
