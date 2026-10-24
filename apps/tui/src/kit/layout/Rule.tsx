//! A horizontal rule across the region it sits in.
//!
//! The default is a one-row box with only its top edge: the renderer paints the
//! line at whatever width the layout gives it. A caller that needs the rule's
//! own wash, or a rule inside a run that is not a box, names the columns and the
//! brick repeats the glyph inside itself.

/** Renders one horizontal rule across the available width. */
export function Rule({
  color,
  columns,
  background,
}: {
  color: string;
  /** The rule's own columns; the layout's width when omitted. */
  columns?: number;
  /** The wash behind the rule, when it carries one. */
  background?: string;
}) {
  if (columns !== undefined) {
    return (
      <text wrapMode="none" fg={color} bg={background}>
        {"─".repeat(Math.max(1, columns))}
      </text>
    );
  }
  return <box height={1} borderStyle="single" border={["top"]} borderColor={color} />;
}
