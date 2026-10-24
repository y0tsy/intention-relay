//! The footer: the live band the window keeps under the printed transcript.
//!
//! The transcript is the terminal's scrollback, so the rows the application
//! paints are the ones that can still change: the hint band or the theme
//! picker's panel, the step in flight, the input block, and the notice line,
//! inside the window's rounded frame. A session with nothing committed yet fills
//! the frame with the welcome pane, so a launch is the panel the alt-screen
//! surface drew, cell for cell.
//!
//! The kernel plans the footer's rows and paints its frame; this module only
//! names what goes in each region. The footer tells the renderer how many rows
//! it took, and above them the terminal owns the rows the transcript printed.
//! The session browser is a layout row docked under the footer, and the footer
//! reflows into the rows the browser leaves.

import type { ThemePreference } from "../app/action";
import { type CommandMenu, MAX_COMMAND_ROWS } from "../app/menu";
import {
  selectBrowserFilter,
  selectBrowserRows,
  selectBrowserTab,
  selectCommandMenu,
  selectDraft,
  selectEmpty,
  selectMenuHighlight,
  selectSelection,
  selectSessionsError,
  selectSurface,
  selectTheme,
} from "../app/select";
import type { Surface } from "../app/state";
import type { AppStore } from "../app/store";
import {
  bandSlotRows,
  type BrowserLayout,
  browserPlan,
  browserPanelColumns,
  DOCK_GAP_ROWS,
  Dock,
  FooterFrame,
  type FooterMode,
  footerPlan,
  inputSlotRows,
  Scroll,
  Slot,
  SlotRows,
  useWindow,
} from "../kit";
import type { Palette } from "../theme/palette";
import { HintBand } from "./HintBand";
import { InputBlock } from "./InputBlock";
import { SessionBrowser } from "./SessionBrowser";
import { Detail } from "./StatusRow";
import { PICKER_ROWS, ThemePicker } from "./ThemePicker";
import { Live } from "./Transcript";
import { useNow } from "./use-now";
import { useStore } from "./use-store";
import { Welcome } from "./Welcome";

/**
 * Adds the rows the hint region asks for this frame.
 *
 * The picker's panel is fixed and takes the region whenever it is open; the
 * command band takes the rows its word ranks, and a body too short for the band
 * and the input gives the band up rather than the input the reader is typing
 * into.
 */
function hintRows(menu: CommandMenu | null, surface: Surface): number {
  return surface === "theme"
    ? PICKER_ROWS
    : bandSlotRows(menu?.rows.length ?? null, MAX_COMMAND_ROWS);
}

/** Renders the hint region: the picker's panel, or the band on the caret's word. */
function Hints({
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
function Docked({
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
      />
    </Dock>
  );
}

/** Renders the footer's part of the window's frame and reserves its rows. */
export function Footer({
  store,
  palette,
  printedRows,
}: {
  store: AppStore;
  palette: Palette;
  /** The rows the transcript has already printed above the footer. */
  printedRows: number;
}) {
  // The renderer's region is the footer, so the window's own size comes from the
  // terminal; a resize repaints through the hook.
  const { columns: width, rows: windowHeight } = useWindow();
  const draft = useStore(store, selectDraft);
  const menu = useStore(store, selectCommandMenu);
  const menuHighlight = useStore(store, selectMenuHighlight);
  const empty = useStore(store, selectEmpty);
  const surface = useStore(store, selectSurface);
  const selection = useStore(store, selectSelection);
  const rows = useStore(store, selectBrowserRows);
  const theme = useStore(store, selectTheme);
  const browser = surface === "sessions" ? browserPlan(width, windowHeight, rows.length) : null;
  const mode: FooterMode = empty ? "welcome" : "session";
  const layout = footerPlan(
    width,
    windowHeight,
    inputSlotRows(draft.text),
    hintRows(menu, surface),
    mode,
    browser === null ? 0 : DOCK_GAP_ROWS + browser.rows,
    printedRows,
  );
  const welcome = mode === "welcome";
  return (
    <FooterFrame
      layout={layout}
      welcome={welcome}
      palette={palette}
      below={browser === null ? null : <Docked store={store} palette={palette} browser={browser} />}
    >
      {layout.liveRows === 0 ? null : (
        <SlotRows rows={layout.liveRows}>
          <Slot>
            {welcome ? (
              <Welcome palette={palette} />
            ) : (
              <Scroll>
                <Live store={store} palette={palette} />
              </Scroll>
            )}
          </Slot>
        </SlotRows>
      )}
      {layout.hintRows === 0 ? null : (
        <SlotRows rows={layout.hintRows}>
          <Hints
            menu={menu}
            menuHighlight={menuHighlight}
            surface={surface}
            selection={selection}
            theme={theme}
            palette={palette}
          />
        </SlotRows>
      )}
      <SlotRows rows={layout.inputRows}>
        <InputBlock store={store} palette={palette} />
      </SlotRows>
      <SlotRows rows={layout.detailRows}>
        <Slot>
          <Detail store={store} palette={palette} />
        </Slot>
      </SlotRows>
    </FooterFrame>
  );
}
