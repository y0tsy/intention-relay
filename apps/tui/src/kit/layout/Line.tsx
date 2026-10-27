//! One row of content, laid out across its region.

import type { ReactNode } from "react";

/** Renders children on one row. */
export function Line({ children, background }: { children?: ReactNode; background?: string }) {
  return (
    <box flexDirection="row" height={1} backgroundColor={background}>
      {children}
    </box>
  );
}
