//! The footer's frame: the page under it, the window margin, the rounded frame,
//! and the rows it reserves for the renderer.
//!
//! The frame is the one region that must agree with the renderer: its height is
//! the footer height, and the renderer has to know it before the printer writes
//! the rows above. The effect lives here, beside the box it measures, so a
//! surface never talks to the renderer about geometry.

import { useRenderer } from "@opentui/react";
import type { ReactNode } from "react";
import { useLayoutEffect } from "react";
import { ContentColumns } from "./context";
import { FOOTER_MARGIN, FOOTER_MIN_COLUMNS, type FooterLayout } from "./footer-plan";
import { Surface } from "./Surface";

/** The palette roles the frame paints. */
export interface FooterPalette {
  readonly canvas: string;
  readonly panel: string;
  readonly border: string;
}

/** Renders the live footer over the canvas, and the surface below it. */
export function FooterFrame({
  layout,
  welcome,
  palette,
  below,
  children,
}: {
  layout: FooterLayout;
  /** The welcome paints its own top edge and the row above it. */
  welcome: boolean;
  palette: FooterPalette;
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
  const frameRows = Math.max(0, rows - layout.dockedRows - layout.chromeRows + (welcome ? 2 : 1));
  const innerRows = Math.max(0, frameRows - (welcome ? 2 : 1));
  return (
    <box flexDirection="column" height={rows} backgroundColor={palette.canvas}>
      {/* The page is painted under every row of the region first: a row that
          shows no text is still a row of the page, not a hole through it. */}
      <box position="absolute" top={0} left={0} width={layout.windowColumns} height={rows}>
        <Surface rows={rows} columns={layout.windowColumns} color={palette.canvas} />
      </box>
      {welcome ? <box height={FOOTER_MARGIN} /> : null}
      <box flexDirection="row" height={frameRows}>
        <box width={FOOTER_MARGIN} />
        <box
          width={layout.columns}
          height={frameRows}
          flexDirection="column"
          borderStyle="rounded"
          border={welcome ? true : ["left", "right", "bottom"]}
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
