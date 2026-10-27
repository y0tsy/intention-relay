//! A row of equally wide columns, with each child told the width it owns.
//!
//! A surface names how many columns it wants and hands one child per column; the
//! brick divides the inherited content width, keeps the gaps, and provides each
//! child's own columns, so no surface divides a width by hand.

import { Children, type ReactNode } from "react";
import { ContentColumns, useContentColumns } from "./context";
import { Row } from "./Stack";

/** Renders children in equal columns with one blank gap between them. */
export function Columns({
  count,
  gap = 2,
  limit,
  align = "start",
  children,
}: {
  /** The number of columns the row offers. */
  count: number;
  /** The blank columns between two columns. */
  gap?: number;
  /** The widest one column may grow, however wide the region is. */
  limit?: number;
  /** Where the run sits when the limit leaves it narrower than the region. */
  align?: "start" | "center";
  children?: ReactNode;
}) {
  const inherited = useContentColumns();
  const slots = Math.max(1, count);
  const totalGap = gap * Math.max(0, slots - 1);
  const divided = Math.max(0, Math.floor((inherited - totalGap) / slots));
  const width = limit === undefined ? divided : Math.min(divided, limit);
  const items = Children.toArray(children);
  if (width < 1 || items.length === 0) {
    return null;
  }
  const spent = width * Math.min(slots, items.length) + gap * Math.max(0, items.length - 1);
  return (
    <Row gap={gap} justify={align === "center" && spent < inherited ? "center" : "flex-start"}>
      {items.slice(0, slots).map((child, index) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: the columns are a positional run of children.
        <box key={index} width={width}>
          <ContentColumns columns={width}>{child}</ContentColumns>
        </box>
      ))}
    </Row>
  );
}
