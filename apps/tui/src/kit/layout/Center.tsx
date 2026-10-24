//! Centring a run of rows without measuring it.
//!
//! The layout engine already knows the width; a centred row is the engine's
//! `justify-content` and nothing else. A surface never pads a line by hand.

import type { ReactNode } from "react";
import { Row } from "./Stack";

/** Renders a one-row run centred across its region. */
export function Center({ children, rows = 1 }: { children?: ReactNode; rows?: number }) {
  return (
    <Row height={rows} justify="center">
      {children}
    </Row>
  );
}
