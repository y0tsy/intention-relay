//! The sessions browser: one panel docked to the bottom of the chat window.
//!
//! The browser never paints over the transcript: it is a layout row at the bottom
//! of the window, and the chat panel above it reflows into the rows the panel
//! leaves. One blank canvas row separates the chat frame's bottom edge from the
//! panel's top border. The panel is its whole surface - a frame whose top border
//! carries the `Sessions` title, a tab radio row, a search bar, the session
//! table, and the hotkey legend as its own last inner row - so a tall window
//! never separates the panel from its legend.
//!
//! A narrow panel drops a column instead of squeezing every column into a
//! fragment, and a row the filter hides is a row the cursor cannot reach.

import { TextAttributes } from "@opentui/core";
import { BROWSER_TABS, type BrowserTab, relativeTime, tabLabel } from "../app/browser";
import type { SessionSummaryDto } from "../bindings";
import {
  browserColumnWidths,
  browserDataRows,
  Cell,
  Pane,
  Row,
  Rule,
  scrollWindow,
  Table,
  type TableCellStyle,
  useContentColumns,
  useSlotRows,
} from "../kit";
import type { Palette } from "../theme/palette";
import { ACTIVE_TAB, CURSOR_MARKER, INACTIVE_TAB, ROW_MARKER, SEARCH_PLACEHOLDER } from "./glyphs";

/** The keys the browser offers, as the legend spells them. */
export const BROWSER_HINTS = [
  { keys: "Enter", action: "select" },
  { keys: "Tab", action: "switch tab" },
  { keys: "Esc", action: "close" },
];

/** Returns the tab radio row. */
function TabRow({ tab, palette }: { tab: BrowserTab; palette: Palette }) {
  return (
    <text wrapMode="none" bg={palette.headerBg}>
      <span> </span>
      {BROWSER_TABS.map((entry) => {
        const active = entry === tab;
        return (
          <span key={entry}>
            {entry === BROWSER_TABS[0] ? null : <span fg={palette.inkFaint}>{"  |  "}</span>}
            <span fg={active ? palette.accent : palette.inkFaint}>
              {`${active ? ACTIVE_TAB : INACTIVE_TAB} `}
            </span>
            <span
              fg={active ? palette.ink : palette.inkMuted}
              attributes={active ? TextAttributes.BOLD : undefined}
            >
              {tabLabel(entry)}
            </span>
          </span>
        );
      })}
    </text>
  );
}

/** Returns the search bar, which shows the filter or its placeholder. */
function SearchBar({ filter, palette }: { filter: string; palette: Palette }) {
  const columns = useContentColumns();
  const empty = filter.length === 0;
  return (
    <Cell
      text={empty ? SEARCH_PLACEHOLDER : filter}
      width={columns}
      ink={empty ? palette.inkFaint : palette.ink}
      background={palette.inputBg}
    />
  );
}

/** Returns the footer: the legend, and the counter of the rows the window shows. */
function BrowserFooter({
  first,
  last,
  total,
  palette,
}: {
  first: number;
  last: number;
  total: number;
  palette: Palette;
}) {
  const counter = total === 0 ? "0 of 0" : `${first}-${last} of ${total}`;
  return (
    <Row background={palette.panel}>
      <Row grow={1}>
        {BROWSER_HINTS.map((hint, index) => (
          <text key={hint.keys} wrapMode="none">
            {index === 0 ? null : <span fg={palette.inkFaint}>{"  "}</span>}
            <span fg={palette.inkMuted}>{hint.keys}</span>
            <span fg={palette.inkFaint}>{` ${hint.action}`}</span>
          </text>
        ))}
      </Row>
      <text wrapMode="none" fg={palette.inkFaint}>
        {counter}
      </text>
    </Row>
  );
}

/** Renders the panel's content: its rows read their own columns from the pane. */
function BrowserBody({
  rows,
  error,
  selection,
  filter,
  tab,
  palette,
  now,
}: {
  rows: readonly SessionSummaryDto[];
  error: string | null;
  selection: number;
  filter: string;
  tab: BrowserTab;
  palette: Palette;
  now: number;
}) {
  const columns = useContentColumns();
  const widths = browserColumnWidths(columns);
  const dataRows = browserDataRows(useSlotRows(0));
  const window = scrollWindow({ count: rows.length, visible: dataRows, selected: selection });
  const shown = rows.slice(window.start, window.end);
  const tableColumns = [
    { key: "modified", width: widths.modified },
    { key: "created", width: widths.created },
    { key: "size", width: widths.size },
    { key: "title", width: widths.title },
    { key: "path", width: widths.path },
  ];
  const header = {
    key: "__header",
    background: palette.headerBg,
    cells: {
      modified: "Modified",
      created: "Created",
      size: "Size",
      title: "Title",
      path: "Path",
    },
  };
  const dataRowsPainted = shown.map((session, index) => ({
    key: session.session_id,
    selected: window.start + index === selection,
    background: window.start + index === selection ? palette.rowSelectedBg : palette.panel,
    cells: {
      modified: relativeTime(session.updated_at, now),
      created: relativeTime(session.created_at, now),
      size: String(session.message_count),
      title: session.session_id,
      path: session.workspace_root,
    },
  }));
  return (
    <>
      <box height={1} />
      <TabRow tab={tab} palette={palette} />
      <SearchBar filter={filter} palette={palette} />
      <Rule color={palette.inkFaint} background={palette.dividerTint} />
      <Table
        columns={tableColumns}
        marker={{ selected: CURSOR_MARKER, row: ROW_MARKER }}
        rows={[header]}
        renderCell={(): TableCellStyle => headerStyle(true, palette)}
        renderMarker={(row) => ({
          ink: palette.inkMuted,
          bold: row.key === "__header",
        })}
      />
      {error === null ? null : (
        <Cell text={error} width={columns} ink={palette.error} background={palette.stubBg} />
      )}
      {rows.length === 0 && error === null ? (
        <Cell
          text="no sessions yet; send a prompt to create one"
          width={columns}
          ink={palette.accentDeep}
          background={palette.stubBg}
        />
      ) : null}
      <Table
        columns={tableColumns}
        marker={{ selected: CURSOR_MARKER, row: ROW_MARKER }}
        rows={dataRowsPainted}
        renderCell={(column, _cell, row) =>
          sessionStyle(column.key, row.selected === true, palette)
        }
        renderMarker={(row) => ({
          ink: row.selected === true ? palette.ink : palette.inkMuted,
          bold: row.selected === true,
        })}
      />
      <box flexGrow={1} />
      <Rule color={palette.inkFaint} background={palette.dividerTint} />
      <BrowserFooter
        first={window.first}
        last={window.last}
        total={rows.length}
        palette={palette}
      />
    </>
  );
}

/** Renders the docked browser panel in the rows the window gave it. */
export function SessionBrowser({
  rows,
  error,
  selection,
  filter,
  tab,
  palette,
  now,
}: {
  rows: readonly SessionSummaryDto[];
  error: string | null;
  selection: number;
  filter: string;
  tab: BrowserTab;
  palette: Palette;
  now: number;
}) {
  const height = useSlotRows(0);
  return (
    <Pane
      height={height}
      title="Sessions"
      titleColor={palette.accentDeep}
      borderColor={palette.border}
      background={palette.panel}
      paddingLeft={2}
      paddingRight={2}
    >
      <BrowserBody
        rows={rows}
        error={error}
        selection={selection}
        filter={filter}
        tab={tab}
        palette={palette}
        now={now}
      />
    </Pane>
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
