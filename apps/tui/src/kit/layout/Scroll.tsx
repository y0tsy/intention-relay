//! The edge's scrollbox: the rows of the step in flight pinned to its last row.

import type { ReactNode } from "react";

/** Renders children in a scrollbox that stays on its last row. */
export function Scroll({ children }: { children?: ReactNode }) {
  return (
    // The edge is a preview of rows the printer is about to write, not a region
    // the reader scrolls: a bar in it would flicker as the stream moves past.
    <scrollbox
      flexGrow={1}
      flexBasis={0}
      stickyScroll
      stickyStart="bottom"
      scrollbarOptions={{ visible: false }}
    >
      {children}
    </scrollbox>
  );
}
