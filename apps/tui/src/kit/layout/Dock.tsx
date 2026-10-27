//! One surface docked under the footer: its gap, its margin, and its own rows.

import type { ReactNode } from "react";
import { ContentColumns, SlotRows } from "./context";
import { DOCK_GAP_ROWS } from "./dock-plan";
import { FOOTER_MARGIN } from "./footer-plan";
import { Spacer, Row, Stack } from "./Stack";

/** Renders a panel docked under the footer, in the rows it was given. */
export function Dock({
  rows,
  columns,
  children,
}: {
  /** The rows the panel occupies, gap excluded. */
  rows: number;
  /** The panel's own outer columns. */
  columns: number;
  children?: ReactNode;
}) {
  return (
    <>
      <Spacer rows={DOCK_GAP_ROWS} />
      <Row height={rows}>
        <Spacer columns={FOOTER_MARGIN} />
        <SlotRows rows={rows}>
          {/* The panel is the width the dock gives it, not the window's: a
              content width taken from the window is one column wider than the
              frame, and the cell that fills it loses its last column. */}
          <ContentColumns columns={columns}>
            <Stack width={columns}>{children}</Stack>
          </ContentColumns>
        </SlotRows>
      </Row>
    </>
  );
}
