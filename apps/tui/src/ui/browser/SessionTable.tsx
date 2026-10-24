//! The browser's table: its header row, and the window of session rows.

import { relativeTime } from "../../app/browser";
import { Table, type TableCellStyle } from "../../kit";
import type { Palette } from "../../theme/palette";
import { CURSOR_MARKER, ROW_MARKER } from "../glyphs";
import type { BrowserTable } from "./table";

/** Renders the table in the rows one browser window gave it. */
export function SessionTable({
  table,
  selection,
  palette,
  now,
}: {
  table: BrowserTable;
  selection: number;
  palette: Palette;
  now: number;
}) {
  const header = {
    key: "__header",
    background: palette.headerBg,
    cells: {
      modified: "Modified",
      created: "Created",
      size: "Msgs",
      title: "Session",
      path: "Path",
    },
  };
  const painted = table.shown.map((session, index) => {
    const position = table.start + index;
    const selected = position === selection;
    return {
      key: session.session_id,
      selected,
      background: selected
        ? palette.rowSelectedBg
        : position % 2 === 1
          ? palette.rowAltBg
          : palette.panel,
      cells: {
        modified: relativeTime(session.updated_at, now),
        created: relativeTime(session.created_at, now),
        size: String(session.message_count),
        title: session.session_id,
        path: session.workspace_root,
      },
    };
  });
  return (
    <>
      <Table
        columns={table.columns}
        marker={{ selected: CURSOR_MARKER, row: ROW_MARKER }}
        rows={[header]}
        renderCell={(): TableCellStyle => headerStyle(true, palette)}
        renderMarker={(row) => ({
          ink: palette.inkMuted,
          bold: row.key === "__header",
        })}
      />
      <Table
        columns={table.columns}
        marker={{ selected: CURSOR_MARKER, row: ROW_MARKER }}
        rows={painted}
        renderCell={(column, _cell, row) =>
          sessionStyle(column.key, row.selected === true, palette)
        }
        renderMarker={(row) => ({
          ink: row.selected === true ? palette.ink : palette.inkMuted,
          bold: row.selected === true,
        })}
      />
    </>
  );
}

/** Returns the header cell's ink. */
function headerStyle(header: boolean, palette: Palette): TableCellStyle {
  if (!header) {
    return { ink: palette.inkMuted };
  }
  return { ink: palette.inkMuted, bold: true };
}

/** Returns one session cell's ink. */
function sessionStyle(_column: string, selected: boolean, palette: Palette): TableCellStyle {
  return { ink: selected ? palette.ink : palette.inkMuted, bold: selected };
}
