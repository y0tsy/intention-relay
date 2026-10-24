//! A table of padded runs: one marker, one gap, and the columns that fit.
//!
//! A band and the session table both read as one line per row, the marker first
//! and one blank column between the columns. The brick owns the widths: fit
//! columns take what their rows ask, the last growing column takes the rest, and
//! a narrow table drops a column whole instead of squeezing every column into a
//! fragment. A caller styles the ink; it never measures it.

import { TextAttributes } from "@opentui/core";
import { cellText, cellTextRight, displayWidth, pad } from "../text";
import { useContentColumns } from "./context";

/** One column of the table. */
export interface TableColumn {
  /** The key the caller reads its cell by. */
  readonly key: string;
  /** The column's width; its rows' widest cell when omitted. */
  readonly width?: number;
  /** Takes the columns the other columns leave, the last of them first. */
  readonly grow?: boolean;
  /** Lower values hide first when the table is narrow; `0` never hides. */
  readonly hideOrder?: number;
  /** Cuts an overflowing cell with an ellipsis; `false` only pads it. */
  readonly truncate?: boolean;
  readonly align?: "left" | "right";
}

/** One row of the table. */
export interface TableRow {
  /** The row's own marker, or the selected one when `selected`. */
  readonly marker?: string;
  readonly selected?: boolean;
  /** The wash behind the whole row. */
  readonly background?: string;
  readonly cells: Readonly<Record<string, string>>;
}

/** One cell the table measured. */
export interface TableCell {
  /** The cell's own text, unpadded. */
  readonly text: string;
  /** The columns the cell occupies, padding included. */
  readonly width: number;
}

/** The ink one cell paints. */
export interface TableCellStyle {
  readonly ink?: string;
  readonly bold?: boolean;
}

/** Renders one table row per input row, aligned on the columns that fit. */
export function Table({
  columns,
  rows,
  marker,
  markerGap = 1,
  gap = 1,
  width,
  renderCell,
  renderMarker,
}: {
  columns: readonly TableColumn[];
  rows: ReadonlyArray<TableRow & { key: string }>;
  /** The marker the rows share: its selected and unselected spellings. */
  marker?: { readonly selected: string; readonly row: string };
  /** The blank columns between the marker and the first column. */
  markerGap?: number;
  /** The blank columns between two columns. */
  gap?: number;
  /** The table's own width; the content width when omitted. */
  width?: number;
  renderCell: (
    column: TableColumn,
    cell: { text: string; width: number },
    row: TableRow & { key: string },
  ) => TableCellStyle;
  renderMarker?: (row: TableRow & { key: string }) => TableCellStyle;
}) {
  const inherited = useContentColumns();
  const available = width ?? inherited;
  const markerWidth =
    marker === undefined ? 0 : Math.max(displayWidth(marker.selected), displayWidth(marker.row));
  const plan = columnWidths(available, columns, rows, markerWidth, markerGap, gap);
  const visible = columns.filter((column) => !plan.hidden.has(column.key));
  return (
    <box flexDirection="column">
      {rows.map((row) => {
        const mark =
          marker === undefined
            ? null
            : (row.marker ?? (row.selected === true ? marker.selected : marker.row));
        const markerStyle = renderMarker === undefined ? {} : renderMarker(row);
        return (
          <text key={row.key} wrapMode="none" bg={row.background}>
            {mark === null ? null : (
              <span
                fg={markerStyle.ink}
                attributes={markerStyle.bold === true ? TextAttributes.BOLD : undefined}
              >
                {`${cellText(mark, markerWidth)}${" ".repeat(markerGap)}`}
              </span>
            )}
            {visible.map((column, index) => {
              const cellWidth = plan.widths.get(column.key) ?? 0;
              const text = row.cells[column.key] ?? "";
              const style = renderCell(column, { text, width: cellWidth }, row);
              const content =
                column.truncate === false
                  ? pad(text, cellWidth)
                  : column.align === "right"
                    ? cellTextRight(text, cellWidth)
                    : cellText(text, cellWidth);
              return (
                <span
                  key={column.key}
                  fg={style.ink}
                  attributes={style.bold === true ? TextAttributes.BOLD : undefined}
                >
                  {`${index === 0 ? "" : " ".repeat(gap)}${content}`}
                </span>
              );
            })}
          </text>
        );
      })}
    </box>
  );
}

/** The widths one table spends. */
interface ColumnPlan {
  /** The width of every visible column. */
  readonly widths: ReadonlyMap<string, number>;
  /** The keys the table dropped for being too narrow. */
  readonly hidden: ReadonlySet<string>;
}

/** Returns the widths one table spends in `available` columns. */
function columnWidths(
  available: number,
  columns: readonly TableColumn[],
  rows: ReadonlyArray<TableRow & { key: string }>,
  markerWidth: number,
  markerGap: number,
  gap: number,
): ColumnPlan {
  const room = Math.max(0, available);
  const hidden = new Set<string>();
  let visible = [...columns];
  const spent = (): number =>
    Math.max(
      0,
      (markerWidth > 0 ? markerWidth + markerGap : 0) +
        gap * Math.max(0, visible.length - 1) +
        intrinsicSum(visible, rows),
    );
  // A narrow table drops a column whole instead of squeezing every column into
  // a fragment: the lowest hide order goes first.
  while (visible.length > 0 && spent() > room) {
    const order = [...visible]
      .filter((column) => (column.hideOrder ?? 0) > 0)
      .sort((left, right) => (left.hideOrder ?? 0) - (right.hideOrder ?? 0))
      .at(0);
    if (order === undefined) {
      break;
    }
    hidden.add(order.key);
    visible = visible.filter((column) => column.key !== order.key);
  }
  const widths = new Map<string, number>();
  for (const column of visible) {
    widths.set(column.key, column.width ?? intrinsicWidth(column, rows));
  }
  // The last growing column takes what the table leaves.
  const spentNow =
    Math.max(
      0,
      (markerWidth > 0 ? markerWidth + markerGap : 0) + gap * Math.max(0, visible.length - 1),
    ) + [...visible].reduce((sum, column) => sum + (widths.get(column.key) ?? 0), 0);
  const leftover = Math.max(0, room - spentNow);
  if (leftover > 0) {
    const grower = [...visible].reverse().find((column) => column.grow === true);
    if (grower !== undefined) {
      widths.set(grower.key, (widths.get(grower.key) ?? 0) + leftover);
    }
  }
  return { widths, hidden };
}

/** Returns the columns one set of visible columns asks for. */
function intrinsicSum(
  visible: readonly TableColumn[],
  rows: ReadonlyArray<TableRow & { key: string }>,
): number {
  return visible.reduce((sum, column) => sum + (column.width ?? intrinsicWidth(column, rows)), 0);
}

/** Returns the columns one column asks for: its own width, or its widest row. */
function intrinsicWidth(
  column: TableColumn,
  rows: ReadonlyArray<TableRow & { key: string }>,
): number {
  if (column.width !== undefined) {
    return column.width;
  }
  return rows.reduce(
    (widest, row) => Math.max(widest, displayWidth(row.cells[column.key] ?? "")),
    0,
  );
}
