//! The command band: the rows one word ranks, with the window around the cursor.
//!
//! The band is one framed region above the input. It owns its columns, its
//! window, and the row that reports the rows it did not show; a surface names
//! the rows and the palette roles it paints.

import { bandColumnWidths } from "./menu-band";
import { Pane, paneContentColumns } from "./Pane";
import { scrollWindow } from "./ScrollWindow";
import { Table, type TableCellStyle } from "./Table";
import { useContentColumns } from "./context";

/** One row the band ranks. */
export interface BandItem {
  /** A stable identity for the row. */
  readonly key: string;
  /** The first cell: a command name or an argument value. */
  readonly label: string;
  /** The one-line description. */
  readonly description: string;
  /** The command's category or the argument's name. */
  readonly trailing: string;
}

/** The palette roles the band paints. */
export interface BandPalette {
  readonly accent: string;
  readonly accentDeep: string;
  readonly headerBg: string;
  readonly rowSelectedBg: string;
  readonly ink: string;
  readonly inkMuted: string;
}

/** Renders the band around one highlighted row. */
export function Band({
  items,
  highlight,
  labels,
  trailing,
  title,
  noun,
  limit,
  marker,
  rowMarker,
  palette,
}: {
  items: readonly BandItem[];
  /** The row the reader is on; the band shows the window around it. */
  highlight: number;
  /** Every label the registry ranks, for the column's own width. */
  labels: readonly string[];
  /** Every trailing cell the registry ranks, for the column's own width. */
  trailing: readonly string[];
  title: string;
  /** The noun the overflow row names: `commands` or `values`. */
  noun: string;
  /** The rows the band shows at once. */
  limit: number;
  /** The cursor marker and the row marker, already spelled. */
  marker: string;
  rowMarker: string;
  palette: BandPalette;
}) {
  const window = scrollWindow({ count: items.length, visible: limit, selected: highlight });
  const shown = items.slice(window.start, window.end);
  const hidden = items.length - shown.length;
  const widths = bandColumnWidths(paneContentColumns(useContentColumns(), 2), labels, trailing);
  const columns = [
    { key: "label", width: widths.label, truncate: false },
    ...(widths.description > 0
      ? [{ key: "description", width: widths.description, truncate: false }]
      : []),
    ...(widths.trailing > 0 ? [{ key: "trailing", width: widths.trailing }] : []),
  ];
  return (
    <Pane
      title={title}
      titleColor={palette.accentDeep}
      borderColor={palette.accent}
      background={palette.headerBg}
      paddingLeft={1}
      paddingRight={1}
    >
      <Table
        columns={columns}
        marker={{ selected: marker, row: rowMarker }}
        markerGap={0}
        rows={shown.map((item, index) => ({
          key: item.key,
          selected: window.start + index === highlight,
          background: window.start + index === highlight ? palette.rowSelectedBg : palette.headerBg,
          cells: {
            label: item.label,
            description: item.description,
            trailing: item.trailing,
          },
        }))}
        renderCell={(column, cell, row): TableCellStyle =>
          cellStyle(column.key, row.selected === true, cell.text, palette)
        }
        renderMarker={(row) => ({
          ink: palette.accent,
          bold: row.selected === true,
        })}
      />
      {hidden > 0 ? (
        <text wrapMode="none" bg={palette.headerBg}>
          <span fg={palette.headerBg}>{rowMarker}</span>
          <span fg={palette.inkMuted}>{`… and ${hidden} more ${noun}`}</span>
        </text>
      ) : null}
    </Pane>
  );
}

/** Returns one cell's ink for the band's columns. */
function cellStyle(
  column: string,
  selected: boolean,
  _text: string,
  palette: BandPalette,
): TableCellStyle {
  switch (column) {
    case "label":
      return { ink: palette.accentDeep, bold: selected };
    case "description":
      return { ink: palette.ink };
    default:
      return { ink: palette.inkMuted };
  }
}
