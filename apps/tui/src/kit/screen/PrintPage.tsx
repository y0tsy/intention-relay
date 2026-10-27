//! The page a print paints: the transcript's rule, and the rows it carries.
//!
//! A printed row is the terminal's own: the page paints it once and the history
//! keeps it. The rule runs down the column the tray's rail sits in, so the page
//! and the tray meet on one vertical line, and the text keeps the columns the
//! page's own measure gives it. The page is painted in the terminal's colours,
//! not the window's: it is a row of the scrollback, not a region of the footer.

import type { ScrollbackSurface } from "@opentui/core";
import { createPortal } from "@opentui/react";
import type { ReactNode } from "react";
import { ContentColumns } from "../layout/context";
import { RULE_COLUMNS, RULE_GAP, RIGHT_MARGIN, transcriptColumns } from "../layout/footer-plan";
import type { PrintPalette } from "./print-types";

/** Paints the rule that runs beside every printed row. */
function Rule({ palette }: { palette: PrintPalette }) {
  return <box width={RULE_COLUMNS} border={["right"]} borderColor={palette.border} />;
}

/** Paints one print into the scrollback surface it belongs to. */
export function PrintPage({
  surface,
  separator,
  palette,
  children,
}: {
  /** The surface the rows are committed from. */
  surface: ScrollbackSurface;
  /** Whether the page leads with the blank row between two prints. */
  separator: boolean;
  palette: PrintPalette;
  /** The rows the print carries. */
  children?: ReactNode;
}) {
  // The page is the terminal's own width: the rows it carries wrap at the
  // columns the window's width gives them, so a row printed now and the row the
  // footer paints while it streams are the same row.
  return createPortal(
    <box flexDirection="column" width={surface.width} backgroundColor={palette.canvas}>
      {separator ? (
        <box flexDirection="row" height={1}>
          <Rule palette={palette} />
        </box>
      ) : null}
      <ContentColumns columns={transcriptColumns(surface.width)}>
        <box flexDirection="row">
          <Rule palette={palette} />
          <box width={RULE_GAP} />
          <box flexDirection="column" flexGrow={1} paddingRight={RIGHT_MARGIN}>
            {children}
          </box>
        </box>
      </ContentColumns>
    </box>,
    surface.root,
    null,
  );
}
