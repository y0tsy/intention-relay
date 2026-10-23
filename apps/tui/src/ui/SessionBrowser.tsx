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
import {
  BROWSER_TABS,
  type BrowserTab,
  browserColumns,
  relativeTime,
  tabLabel,
} from "../app/browser";
import type { SessionSummaryDto } from "../bindings";
import type { Palette } from "../theme/palette";
import { ACTIVE_TAB, CURSOR_MARKER, INACTIVE_TAB, ROW_MARKER, SEARCH_PLACEHOLDER } from "./glyphs";
import { displayWidth } from "./measure";

/** The keys the browser offers, as the legend spells them. */
export const BROWSER_HINTS = [
  { keys: "Enter", action: "select" },
  { keys: "Tab", action: "switch tab" },
  { keys: "Esc", action: "close" },
];

/** Returns one cell padded to its column. */
function pad(text: string, columns: number): string {
  return `${text}${" ".repeat(Math.max(0, columns - displayWidth(text)))}`;
}

/** Returns one cell cut to its column. */
function truncate(text: string, columns: number): string {
  const characters = [...text];
  return displayWidth(text) <= columns
    ? text
    : `${characters.slice(0, Math.max(0, columns - 1)).join("")}…`;
}

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
function SearchBar({
  filter,
  columns,
  palette,
}: {
  filter: string;
  columns: number;
  palette: Palette;
}) {
  const empty = filter.length === 0;
  return (
    <text wrapMode="none" bg={palette.inputBg} fg={empty ? palette.inkFaint : palette.ink}>
      {pad(empty ? SEARCH_PLACEHOLDER : filter, columns)}
    </text>
  );
}

/** Returns one divider band across the panel's content. */
function Divider({ columns, palette }: { columns: number; palette: Palette }) {
  return (
    <text wrapMode="none" bg={palette.dividerTint} fg={palette.inkFaint}>
      {"─".repeat(Math.max(1, columns))}
    </text>
  );
}

/** Returns the table's column header. */
function HeaderRow({
  columns,
  palette,
}: {
  columns: ReturnType<typeof browserColumns>;
  palette: Palette;
}) {
  const line = [
    ROW_MARKER,
    pad("Modified", columns.modified),
    pad("Created", columns.created),
    pad("Size", columns.size),
    pad("Title", columns.title),
    pad("Path", columns.path),
  ].join(" ");
  return (
    <text
      wrapMode="none"
      bg={palette.headerBg}
      fg={palette.inkMuted}
      attributes={TextAttributes.BOLD}
    >
      {line}
    </text>
  );
}

/** Returns one session row. */
function SessionRow({
  session,
  selected,
  columns,
  now,
  palette,
}: {
  session: SessionSummaryDto;
  selected: boolean;
  columns: ReturnType<typeof browserColumns>;
  now: number;
  palette: Palette;
}) {
  const line = [
    selected ? CURSOR_MARKER : ROW_MARKER,
    pad(truncate(relativeTime(session.updated_at, now), columns.modified), columns.modified),
    pad(truncate(relativeTime(session.created_at, now), columns.created), columns.created),
    pad(String(session.message_count), columns.size),
    pad(truncate(session.session_id, columns.title), columns.title),
    pad(truncate(session.workspace_root, columns.path), columns.path),
  ].join(" ");
  return (
    <text
      wrapMode="none"
      bg={selected ? palette.rowSelectedBg : palette.panel}
      fg={selected ? palette.ink : palette.inkMuted}
      attributes={selected ? TextAttributes.BOLD : undefined}
    >
      {line}
    </text>
  );
}

/** Returns the footer: the legend, and the counter of the rows the window shows. */
function Footer({
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
    <box flexDirection="row" backgroundColor={palette.panel}>
      <box flexGrow={1} flexDirection="row">
        {BROWSER_HINTS.map((hint, index) => (
          <text key={hint.keys} wrapMode="none">
            {index === 0 ? null : <span fg={palette.inkFaint}>{"  "}</span>}
            <span fg={palette.inkMuted}>{hint.keys}</span>
            <span fg={palette.inkFaint}>{` ${hint.action}`}</span>
          </text>
        ))}
      </box>
      <text wrapMode="none" fg={palette.inkFaint}>
        {counter}
      </text>
    </box>
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
  rows_,
  dataRows,
  contentColumns,
  now,
}: {
  rows: readonly SessionSummaryDto[];
  error: string | null;
  selection: number;
  filter: string;
  tab: BrowserTab;
  palette: Palette;
  rows_: number;
  dataRows: number;
  contentColumns: number;
  now: number;
}) {
  const columns = browserColumns(contentColumns);
  const visible = Math.max(1, Math.min(dataRows, rows.length));
  const offset = Math.max(0, Math.min(selection - visible + 1, rows.length - visible));
  const window = rows.slice(offset, offset + visible);
  return (
    <box
      height={rows_}
      width="100%"
      flexDirection="column"
      border
      borderStyle="rounded"
      borderColor={palette.border}
      backgroundColor={palette.panel}
      title="Sessions"
      titleColor={palette.accentDeep}
      titleAlignment="left"
      paddingLeft={2}
      paddingRight={2}
    >
      <box height={1} />
      <TabRow tab={tab} palette={palette} />
      <SearchBar filter={filter} columns={contentColumns} palette={palette} />
      <Divider columns={contentColumns} palette={palette} />
      <HeaderRow columns={columns} palette={palette} />
      {error === null ? null : (
        <text wrapMode="none" bg={palette.stubBg} fg={palette.error}>
          {truncate(error, contentColumns)}
        </text>
      )}
      {rows.length === 0 && error === null ? (
        <text wrapMode="none" bg={palette.stubBg} fg={palette.accentDeep}>
          {pad(
            truncate("no sessions yet; send a prompt to create one", contentColumns),
            contentColumns,
          )}
        </text>
      ) : null}
      {window.map((session, index) => (
        <SessionRow
          key={session.session_id}
          session={session}
          selected={offset + index === selection}
          columns={columns}
          now={now}
          palette={palette}
        />
      ))}
      <box flexGrow={1} />
      <Divider columns={contentColumns} palette={palette} />
      <Footer
        first={rows.length === 0 ? 0 : offset + 1}
        last={offset + visible}
        total={rows.length}
        palette={palette}
      />
    </box>
  );
}
