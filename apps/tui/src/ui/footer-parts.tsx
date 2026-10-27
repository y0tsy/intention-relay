//! The footer's parts: the hint region and the docked session browser.
//!
//! Both parts own the subscriptions they paint, so a menu keystroke or a browser
//! clock tick repaints its region without touching the composer or the live
//! step.

import type { ThemePreference } from "../app/action";
import { type CommandMenu, MAX_COMMAND_ROWS } from "../app/menu";
import {
  selectBrowserFilter,
  selectBrowserRows,
  selectBrowserTab,
  selectSelection,
  selectSessionsError,
} from "../app/select";
import type { Surface } from "../app/state";
import type { AppStore } from "../app/store";
import { bandSlotRows, type BrowserLayout, browserPanelColumns, Dock, useWindow } from "../kit";
import type { Palette } from "../theme/palette";
import { HintBand } from "./HintBand";
import { SessionBrowser } from "./SessionBrowser";
import { PICKER_ROWS, ThemePicker } from "./ThemePicker";
import { useNow } from "./use-now";
import { useStore } from "./use-store";

/**
 * Adds the rows the hint region asks for this frame.
 *
 * The picker's panel is fixed and takes the region whenever it is open; the
 * command band takes the rows its word ranks, and a body too short for the band
 * and the input gives the band up rather than the input the reader is typing
 * into.
 */
export function hintRows(menu: CommandMenu | null, surface: Surface): number {
  return surface === "theme"
    ? PICKER_ROWS
    : bandSlotRows(menu?.rows.length ?? null, MAX_COMMAND_ROWS);
}

/** Renders the hint region: the picker's panel, or the band on the caret's word. */
export function Hints({
  menu,
  menuHighlight,
  surface,
  selection,
  theme,
  palette,
}: {
  menu: CommandMenu | null;
  menuHighlight: number;
  surface: Surface;
  selection: number;
  theme: ThemePreference;
  palette: Palette;
}) {
  return surface === "theme" ? (
    <ThemePicker selection={selection} current={theme} palette={palette} />
  ) : menu === null ? null : (
    <HintBand menu={menu} highlight={menuHighlight} palette={palette} />
  );
}

/** Renders the docked session browser under the footer. */
export function Docked({
  store,
  palette,
  browser,
}: {
  store: AppStore;
  palette: Palette;
  browser: BrowserLayout;
}) {
  const { columns: width } = useWindow();
  const selection = useStore(store, selectSelection);
  const rows = useStore(store, selectBrowserRows);
  const filter = useStore(store, selectBrowserFilter);
  const tab = useStore(store, selectBrowserTab);
  const error = useStore(store, selectSessionsError);
  // The browser prints how long ago a session moved, so the clock runs while it
  // is on screen and nowhere else.
  const now = useNow(true);
  return (
    <Dock rows={browser.rows} columns={browserPanelColumns(width)}>
      <SessionBrowser
        rows={rows}
        error={error}
        selection={selection}
        filter={filter}
        tab={tab}
        palette={palette}
        now={now}
        compact={browser.compact}
      />
    </Dock>
  );
}
