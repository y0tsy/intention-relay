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
import { useLayoutEffect } from "react";
import { ContentColumns } from "./context";
import {
  CONTENT_INDENT,
  FOOTER_MARGIN,
  FOOTER_MIN_COLUMNS,
  type FooterLayout,
  SESSION_MARGIN,
} from "./footer-plan";
import { Surface } from "./Surface";

/** The palette roles the frame paints. */
export interface FooterPalette {
  readonly canvas: string;
  readonly panel: string;
  readonly border: string;
  readonly headerBg: string;
}

/** Renders the live footer over the canvas, and the surface below it. */
export function FooterFrame({
  layout,
  welcome,
  palette,
  header,
  live,
  below,
  children,
}: {
  layout: FooterLayout;
  /** A welcome paints the open page; a session keeps the closing frame. */
  welcome: boolean;
  palette: FooterPalette;
  /** The welcome's top bar, painted across the page above the content. */
  header?: ReactNode;
  /** The step in flight, painted on the page above a session's frame. */
  live?: ReactNode;
  /** A surface docked under the frame, before the window's bottom margin. */
  below?: ReactNode;
  children?: ReactNode;
}) {
  const renderer = useRenderer();
  const rows = layout.rows;
  useLayoutEffect(() => {
    renderer.footerHeight = rows;
    // A print changes the rows the transcript leaves, so the region is a new one
    // on this commit and needs the frame that applies it.
    renderer.requestRender();
  }, [renderer, rows]);
  if (layout.columns < FOOTER_MIN_COLUMNS || layout.body < 1) {
    // A window too narrow to read anything in draws nothing rather than a broken
    // box: the plan has already spent its body.
    return <box />;
  }
  if (welcome) {
    const headerRows = 1;
    const contentRows = Math.max(0, rows - layout.dockedRows - headerRows - FOOTER_MARGIN);
    return (
      <box flexDirection="column" height={rows} backgroundColor={palette.canvas}>
        <box position="absolute" top={0} left={0} width={layout.windowColumns} height={rows}>
          <Surface rows={rows} columns={layout.windowColumns} color={palette.canvas} />
        </box>
        <box height={headerRows} backgroundColor={palette.headerBg}>
          <box
            position="absolute"
            top={0}
            left={0}
            width={layout.windowColumns}
            height={headerRows}
          >
            <Surface rows={headerRows} columns={layout.windowColumns} color={palette.headerBg} />
          </box>
          <box flexDirection="row" height={headerRows}>
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
        {below}
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
    <box flexDirection="column" height={rows} backgroundColor={palette.canvas}>
      {/* The page is painted under every row of the region first: a row that
          shows no text is still a row of the page, not a hole through it. */}
      <box position="absolute" top={0} left={0} width={layout.windowColumns} height={rows}>
        <Surface rows={rows} columns={layout.windowColumns} color={palette.canvas} />
      </box>
      <box height={layout.slackRows} />
      {/* The step in flight is the next block of the transcript, not a row of
          the tray: it paints on the page above the frame, at the columns a
          printed row's text has, and the frame below it stays the tray's height. */}
      {layout.liveRows < 1 ? null : (
        <box flexDirection="row" height={layout.liveRows}>
          <box width={SESSION_MARGIN + 2} />
          <box flexGrow={1}>
            <ContentColumns columns={layout.contentColumns}>{live}</ContentColumns>
          </box>
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
      {below}
      <box height={FOOTER_MARGIN} />
    </box>
  );
}
