//! The footer: the live band the window keeps under the printed transcript.
//!
//! The transcript is the terminal's scrollback, so the rows the application
//! paints are the ones that can still change: the hint band or the theme
//! picker's panel, the step in flight, the input block, and the notice line,
//! inside one rounded frame. A session with nothing committed yet fills the
//! frame with the welcome pane instead, so a launch looks exactly like the
//! panel did.
//!
//! The geometry comes from [`footerLayout`], and the footer tells the renderer
//! how many rows it took: above them the terminal owns everything, below them
//! nothing exists. The session browser is a layout row docked under the footer,
//! and the footer reflows into the rows the browser leaves.

import { useRenderer } from "@opentui/react";
import { type RefObject, useLayoutEffect } from "react";
import type { ThemePreference } from "../app/action";
import { type CommandMenu, menuRows } from "../app/menu";
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
import type { Palette } from "../theme/palette";
import { Fill } from "./Fill";
import { HintBand } from "./HintBand";
import { InputBlock } from "./InputBlock";
import {
  type BrowserLayout,
  browserLayout,
  type FooterLayout,
  type FooterMode,
  footerLayout,
  inputBlockRows,
  MIN_COLUMNS,
  PANEL_GAP_ROWS,
  PANEL_MARGIN,
} from "./panel";
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
  return surface === "theme" ? PICKER_ROWS : menuRows(menu?.rows.length ?? null);
}

/** Renders the hint region: the picker's panel, or the band on the caret's word. */
function Hints({
  menu,
  menuHighlight,
  surface,
  selection,
  theme,
  palette,
  columns,
  rows,
}: {
  menu: CommandMenu | null;
  menuHighlight: number;
  surface: Surface;
  selection: number;
  theme: ThemePreference;
  palette: Palette;
  columns: number;
  rows: number;
}) {
  return (
    <box height={rows}>
      {surface === "theme" ? (
        <ThemePicker selection={selection} current={theme} palette={palette} />
      ) : menu === null ? null : (
        <HintBand menu={menu} highlight={menuHighlight} palette={palette} width={columns} />
      )}
    </box>
  );
}

/** Renders the docked session browser under the footer. */
function Docked({
  store,
  palette,
  browser,
  columns,
}: {
  store: AppStore;
  palette: Palette;
  browser: BrowserLayout;
  columns: number;
}) {
  const selection = useStore(store, selectSelection);
  const rows = useStore(store, selectBrowserRows);
  const filter = useStore(store, selectBrowserFilter);
  const tab = useStore(store, selectBrowserTab);
  const error = useStore(store, selectSessionsError);
  // The browser prints how long ago a session moved, so the clock runs while it
  // is on screen and nowhere else.
  const now = useNow(true);
  return (
    <>
      <box height={PANEL_GAP_ROWS} />
      <box flexDirection="row" height={browser.rows}>
        <box width={PANEL_MARGIN} />
        <box width={columns}>
          <SessionBrowser
            rows={rows}
            error={error}
            selection={selection}
            filter={filter}
            tab={tab}
            palette={palette}
            rows_={browser.rows}
            dataRows={browser.dataRows}
            contentColumns={browser.contentColumns}
            now={now}
          />
        </box>
      </box>
    </>
  );
}

/** Renders the footer and reserves exactly the rows it painted. */
export function Footer({
  store,
  palette,
  width,
  terminalHeight,
  printedRows,
  footerRows,
}: {
  store: AppStore;
  palette: Palette;
  width: number;
  terminalHeight: number;
  /** The rows the transcript has already printed above the footer. */
  printedRows: number;
  /** The printer reads this during its own effect, which runs after this one. */
  footerRows: RefObject<number>;
}) {
  const renderer = useRenderer();
  const draft = useStore(store, selectDraft);
  const menu = useStore(store, selectCommandMenu);
  const menuHighlight = useStore(store, selectMenuHighlight);
  const empty = useStore(store, selectEmpty);
  const surface = useStore(store, selectSurface);
  const selection = useStore(store, selectSelection);
  const rows = useStore(store, selectBrowserRows);
  const theme = useStore(store, selectTheme);
  const browser = surface === "sessions" ? browserLayout(width, terminalHeight, rows.length) : null;
  const mode: FooterMode = empty ? "welcome" : "session";
  const layout: FooterLayout = footerLayout(
    width,
    terminalHeight,
    inputBlockRows(draft.text.split("\n").length - 1),
    hintRows(menu, surface),
    mode,
    browser === null ? 0 : PANEL_GAP_ROWS + browser.rows,
    printedRows,
  );
  const height = layout.rows;
  // The rows above the footer are the terminal's; the renderer is told how many
  // the application took before it paints the frame that needs them.
  useLayoutEffect(() => {
    footerRows.current = height;
    renderer.footerHeight = height;
    // A print changes the rows the transcript leaves, so the region is a new one
    // on this commit and needs the frame that applies it.
    renderer.requestRender();
  }, [renderer, height, footerRows]);
  if (layout.columns < MIN_COLUMNS || layout.body < 1) {
    // A window too narrow to read anything in draws nothing rather than a
    // broken box: the footer's own arithmetic has already spent its body.
    return <box />;
  }
  return (
    <box flexDirection="column" height={height} backgroundColor={palette.canvas}>
      {/* The page is painted under every row of the region first: a row that
          shows no text is still a row of the page, not a hole through it. */}
      <box position="absolute" top={0} left={0} width={width} height={height}>
        <Fill rows={height} columns={width} palette={palette} />
      </box>
      {layout.liveRows === 0 ? null : (
        <box height={layout.liveRows} flexDirection="column">
          {mode === "welcome" ? (
            <Welcome rows={layout.liveRows} columns={layout.contentColumns} palette={palette} />
          ) : (
            <scrollbox flexGrow={1} flexBasis={0} stickyScroll stickyStart="bottom">
              <Live store={store} palette={palette} width={layout.contentColumns} />
            </scrollbox>
          )}
        </box>
      )}
      {layout.hintRows === 0 ? null : (
        <box height={layout.hintRows} paddingLeft={PANEL_MARGIN} paddingRight={PANEL_MARGIN}>
          <Hints
            menu={menu}
            menuHighlight={menuHighlight}
            surface={surface}
            selection={selection}
            theme={theme}
            palette={palette}
            columns={layout.columns}
            rows={layout.hintRows}
          />
        </box>
      )}
      <box flexDirection="row">
        <box width={PANEL_MARGIN} />
        {/* The block stretches inside its own width: a row that let it size to
            its content would shrink it to the prompt's own text. */}
        <box width={layout.columns}>
          <InputBlock
            store={store}
            palette={palette}
            rows={layout.inputRows}
            columns={layout.contentColumns}
          />
        </box>
      </box>
      <box height={layout.detailRows} paddingLeft={PANEL_MARGIN}>
        <Detail store={store} palette={palette} />
      </box>
      {browser === null ? null : (
        <Docked store={store} palette={palette} browser={browser} columns={width - PANEL_MARGIN} />
      )}
    </box>
  );
}
