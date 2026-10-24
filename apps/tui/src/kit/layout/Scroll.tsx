//! The live region's scrollbox: the step in flight pinned to its last row.

import type { ReactNode } from "react";

/** Renders children in a scrollbox that stays on its last row. */
export function Scroll({ children }: { children?: ReactNode }) {
  return (
    <scrollbox flexGrow={1} flexBasis={0} stickyScroll stickyStart="bottom">
      {children}
    </scrollbox>
  );
}
