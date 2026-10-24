//! The two directions a surface stacks its parts in.
//!
//! These are the layout primitives a surface composes: a column, a row, and the
//! blank space between them. They carry no row arithmetic; a fixed spacer names
//! the rows it takes and a growing one takes what the column leaves.

import type { ReactNode } from "react";

/** Renders children in a column. */
export function Stack({
  children,
  gap = 0,
  height,
  width,
  grow = 0,
  shrink = 1,
  background,
  padding = 0,
  spaced = false,
}: {
  children?: ReactNode;
  gap?: number;
  /** Keeps one blank row between two children. */
  spaced?: boolean;
  height?: number;
  width?: number;
  grow?: number;
  shrink?: number;
  background?: string;
  padding?: number;
}) {
  return (
    <box
      flexDirection="column"
      gap={spaced ? Math.max(1, gap) : gap}
      height={height}
      width={width}
      flexGrow={grow}
      flexShrink={shrink}
      backgroundColor={background}
      padding={padding}
    >
      {children}
    </box>
  );
}

/** Renders children in a row. */
export function Row({
  children,
  gap = 0,
  height,
  width,
  grow = 0,
  shrink = 1,
  justify = "flex-start",
  align = "flex-start",
  background,
  padding = 0,
}: {
  children?: ReactNode;
  gap?: number;
  height?: number;
  width?: number;
  grow?: number;
  shrink?: number;
  justify?: "flex-start" | "center" | "flex-end" | "space-between";
  align?: "flex-start" | "center" | "flex-end" | "stretch";
  background?: string;
  padding?: number;
}) {
  return (
    <box
      flexDirection="row"
      gap={gap}
      height={height}
      width={width}
      flexGrow={grow}
      flexShrink={shrink}
      justifyContent={justify}
      alignItems={align}
      backgroundColor={background}
      padding={padding}
    >
      {children}
    </box>
  );
}

/** Renders blank space: a fixed number of rows, or the rows a column leaves. */
export function Spacer({
  rows = 0,
  columns = 0,
  grow = false,
}: {
  rows?: number;
  columns?: number;
  grow?: boolean;
}) {
  if (grow) {
    return <box flexGrow={1} flexBasis={0} />;
  }
  if (columns > 0) {
    return <box width={columns} />;
  }
  return <box height={rows} />;
}
