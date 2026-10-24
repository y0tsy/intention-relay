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

import type { BrowserTab } from "../app/browser";
import type { SessionSummaryDto } from "../bindings";
import { Cell, Pane, Rule, Spacer } from "../kit";
import type { Palette } from "../theme/palette";
import { BrowserFooter, SearchBar, TabRow } from "./browser/chrome";
import { SessionTable } from "./browser/SessionTable";
import { useBrowserTable } from "./browser/table";

export { BROWSER_HINTS } from "./browser/chrome";

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
  const table = useBrowserTable(rows, selection);
  return (
    <>
      <Spacer rows={1} />
      <TabRow tab={tab} palette={palette} />
      <SearchBar filter={filter} palette={palette} />
      <Rule color={palette.inkFaint} background={palette.dividerTint} />
      <SessionTable table={table} selection={selection} palette={palette} now={now} />
      {error === null ? null : (
        <Cell text={error} ink={palette.error} background={palette.stubBg} />
      )}
      {rows.length === 0 && error === null ? (
        <Cell
          text="no sessions yet; send a prompt to create one"
          ink={palette.accentDeep}
          background={palette.stubBg}
        />
      ) : null}
      <Spacer grow />
      <Rule color={palette.inkFaint} background={palette.dividerTint} />
      <BrowserFooter first={table.first} last={table.last} total={rows.length} palette={palette} />
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
  return (
    <Pane
      fill
      wide
      title="Sessions"
      titleColor={palette.accentDeep}
      borderColor={palette.border}
      background={palette.panel}
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
