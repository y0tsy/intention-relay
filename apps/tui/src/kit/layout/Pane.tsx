//! A framed, padded region that tells its content how much room it has.
//!
//! A pane draws its frame and padding and provides its content columns to its
//! children, so the child never measures the chrome around it. A region too
//! narrow for a frame falls back to its bare rows instead of a broken box.

import type { BorderSides } from "@opentui/core";
import type { ReactNode } from "react";
import { ContentColumns, useContentColumns } from "./context";

/** The columns and rows the pane keeps around its content, on each side. */
export const PANE_PADDING = 1;

/** Returns the content columns of one pane with its own outer width. */
export function paneContentColumns(outer: number, border: number): number {
  return Math.max(0, outer - border - PANE_PADDING * 2);
}

/** Returns the slot rows one content pane keeps, frame included. */
export function paneSlotRows(contentRows: number): number {
  return 2 + contentRows;
}

/** Renders children in a rounded frame with padding, and provides their width. */
export function Pane({
  children,
  title,
  titleColor,
  titleAlignment = "left",
  border = true,
  borderColor,
  background,
  paddingLeft = PANE_PADDING,
  paddingRight = PANE_PADDING,
  columns,
  height,
  fallbackBare = false,
}: {
  children?: ReactNode;
  title?: string;
  titleColor?: string;
  titleAlignment?: "left" | "center" | "right";
  /** `true` draws every side; an array draws only those sides. */
  border?: boolean | readonly BorderSides[];
  borderColor?: string;
  background?: string;
  paddingLeft?: number;
  paddingRight?: number;
  /** The pane's own outer width; the parent's content width when omitted. */
  columns?: number;
  height?: number;
  /** Renders the children bare when the pane's own chrome does not fit. */
  fallbackBare?: boolean;
}) {
  const inherited = useContentColumns();
  const outer = columns ?? inherited;
  const chrome = borderStyleColumns(border) + paddingLeft + paddingRight;
  if (fallbackBare && outer - chrome < 1) {
    return <>{children}</>;
  }
  const content = Math.max(0, outer - chrome);
  const sides = border === true || border === false ? undefined : ([...border] as BorderSides[]);
  return (
    <box
      flexDirection="column"
      width={columns}
      height={height}
      borderStyle="rounded"
      border={border === true ? true : sides}
      borderColor={borderColor}
      backgroundColor={background}
      title={title}
      titleColor={titleColor}
      titleAlignment={titleAlignment}
      paddingLeft={paddingLeft}
      paddingRight={paddingRight}
    >
      <ContentColumns columns={content}>{children}</ContentColumns>
    </box>
  );
}

//! One column of the same indentation, kept beside the block it introduces.

/** Renders children indented, and tells them the columns that remain. */
export function Indent({
  columns,
  children,
  background,
}: {
  columns: number;
  children?: ReactNode;
  background?: string;
}) {
  const inherited = useContentColumns();
  const content = Math.max(0, inherited - columns);
  return (
    <box paddingLeft={columns} backgroundColor={background}>
      <ContentColumns columns={content}>{children}</ContentColumns>
    </box>
  );
}

/** Returns the border columns of one pane's own sides. */
function borderStyleColumns(border: boolean | readonly BorderSides[]): number {
  if (border === true) {
    return 2;
  }
  if (border === false) {
    return 0;
  }
  return border.includes("left") && border.includes("right")
    ? 2
    : border.includes("left") || border.includes("right")
      ? 1
      : 0;
}
