//! The input's command hint band: the commands the first word selects, or the
//! values of the argument word the caret sits in.
//!
//! The band is one region of the chat panel directly above the input block, and
//! the transcript gives up exactly the rows the band takes: the panel's body is
//! transcript plus band plus input plus detail, so nothing is painted over
//! anything and the picker's panel, once it opens, spends the same region.
//!
//! The band is its own rounded frame on the header wash, titled `commands` while
//! it ranks the registry and `arguments` while it ranks one declared argument's
//! values. The highlighted row carries the cursor marker and the selected wash,
//! and the first column keeps the accent ink the input line's own prompt uses.

import type { CommandMenu, MenuRow } from "../app/menu";
import { MAX_COMMAND_ROWS, menuLabels, menuTrailing } from "../app/menu";
import {
  bandColumnWidths,
  Pane,
  paneContentColumns,
  scrollWindow,
  Table,
  type TableCellStyle,
  useContentColumns,
} from "../kit";
import type { Palette } from "../theme/palette";
import { CURSOR_MARKER, ROW_MARKER } from "./glyphs";

/** Renders the band the word the caret sits in opens. */
export function HintBand({
  menu,
  highlight,
  palette,
}: {
  menu: CommandMenu;
  /** The row the reader is on; the band shows the window around it. */
  highlight: number;
  palette: Palette;
}) {
  const window = scrollWindow({
    count: menu.rows.length,
    visible: MAX_COMMAND_ROWS,
    selected: highlight,
  });
  const shown = menu.rows.slice(window.start, window.end);
  const hidden = menu.rows.length - shown.length;
  const title = menu.kind === "command" ? "commands" : "arguments";
  const noun = menu.kind === "command" ? "commands" : "values";
  const widths = bandColumnWidths(
    paneContentColumns(useContentColumns(), 2),
    menuLabels(menu),
    menuTrailing(menu),
  );
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
        marker={{ selected: CURSOR_MARKER, row: ROW_MARKER }}
        markerGap={0}
        rows={shown.map((row, index) => bandRow(row, window.start + index === highlight, palette))}
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
          <span fg={palette.headerBg}>{ROW_MARKER}</span>
          <span fg={palette.inkMuted}>{`… and ${hidden} more ${noun}`}</span>
        </text>
      ) : null}
    </Pane>
  );
}

/** Returns the row shape the table reads. */
function bandRow(row: MenuRow, selected: boolean, palette: Palette) {
  return {
    key: row.label,
    selected,
    background: selected ? palette.rowSelectedBg : palette.headerBg,
    cells: {
      label: row.label,
      description: row.description,
      trailing: row.trailing,
    },
  };
}

/** Returns one cell's ink for the band's columns. */
function cellStyle(
  column: string,
  selected: boolean,
  _text: string,
  palette: Palette,
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
