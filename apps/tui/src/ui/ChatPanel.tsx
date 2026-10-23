//! The chat window: one rounded panel whose body holds the transcript - or the
//! welcome pane while no session is open - the hint region, the input block, and
//! the notice line, in that order.
//!
//! The panel is the only container the chat has, and its four inside regions sum
//! to its body exactly: the hint region is a band of this panel, never a surface
//! painted over another, and the input block and the notice line never move. The
//! rows and columns come from [`panelLayout`], so a frame is the same at every
//! window size and no child is squeezed into another's rows.
//!
//! The session browser is a layout row docked to the bottom of the window, and
//! the chat panel reflows into the rows the browser leaves: one blank canvas row
//! separates the chat frame's bottom edge from the browser's top border, and
//! the browser's own bottom edge is the window's last row. The browser never
//! paints over the transcript, and the chat never spends rows the browser owns.

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
import { HintBand } from "./HintBand";
import { InputBlock } from "./InputBlock";
import { browserLayout, inputBlockRows, PANEL_GAP_ROWS, PANEL_MARGIN, panelLayout } from "./panel";
import { SessionBrowser } from "./SessionBrowser";
import { Detail } from "./StatusRow";
import { PICKER_ROWS, ThemePicker } from "./ThemePicker";
import { Transcript } from "./Transcript";
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

/** Renders the chat window in the size it was given. */
export function ChatPanel({
  store,
  palette,
  width,
  height,
}: {
  store: AppStore;
  palette: Palette;
  width: number;
  height: number;
}) {
  const draft = useStore(store, selectDraft);
  const menu = useStore(store, selectCommandMenu);
  const menuHighlight = useStore(store, selectMenuHighlight);
  const empty = useStore(store, selectEmpty);
  const surface = useStore(store, selectSurface);
  const selection = useStore(store, selectSelection);
  const rows = useStore(store, selectBrowserRows);
  const filter = useStore(store, selectBrowserFilter);
  const tab = useStore(store, selectBrowserTab);
  const sessionsError = useStore(store, selectSessionsError);
  const theme = useStore(store, selectTheme);
  // The browser prints how long ago a session moved, so the clock runs while it
  // is on screen and nowhere else.
  const now = useNow(surface === "sessions");
  const breaks = draft.text.split("\n").length - 1;
  // The browser is a layout row at the bottom of the window, so the chat panel
  // reflows into the rows the panel leaves; the chat keeps the majority, the
  // one window never grows a second surface painted over another.
  const browser = surface === "sessions" ? browserLayout(width, height, rows.length) : null;
  const chatHeight = browser === null ? height : height - browser.rows - PANEL_GAP_ROWS;
  const layout = panelLayout(width, chatHeight, inputBlockRows(breaks), hintRows(menu, surface));
  if (layout.rows < 3) {
    // A window too small to frame anything draws nothing rather than a broken
    // box: the panel's own arithmetic has already spent its body.
    return <box />;
  }
  return (
    <box flexDirection="column" height={height} backgroundColor={palette.canvas}>
      <box height={PANEL_MARGIN} />
      <box flexDirection="row">
        <box width={PANEL_MARGIN} />
        <box
          width={layout.columns}
          height={layout.rows}
          flexDirection="column"
          borderStyle="rounded"
          border
          borderColor={palette.border}
          backgroundColor={palette.panel}
          paddingLeft={1}
          paddingRight={1}
        >
          <box height={layout.transcriptRows} flexDirection="column">
            {empty ? (
              <Welcome
                rows={layout.transcriptRows}
                columns={layout.contentColumns}
                palette={palette}
              />
            ) : (
              <scrollbox
                flexGrow={1}
                flexBasis={0}
                stickyScroll
                stickyStart="bottom"
                scrollbarOptions={{
                  trackOptions: {
                    foregroundColor: palette.scrollThumb,
                    backgroundColor: palette.scrollTrack,
                  },
                }}
              >
                {/* The scrollbar reserves one column, so the blocks lay out one
                    column short of the region and never sit under the thumb. */}
                <Transcript store={store} palette={palette} width={layout.contentColumns - 1} />
              </scrollbox>
            )}
          </box>
          {layout.hintRows === 0 ? null : (
            <box height={layout.hintRows}>
              {surface === "theme" ? (
                <ThemePicker selection={selection} current={theme} palette={palette} />
              ) : menu === null ? null : (
                <HintBand
                  menu={menu}
                  highlight={menuHighlight}
                  palette={palette}
                  width={layout.contentColumns}
                />
              )}
            </box>
          )}
          <InputBlock
            store={store}
            palette={palette}
            rows={layout.inputRows}
            columns={layout.contentColumns}
          />
          <box height={layout.detailRows}>
            <Detail store={store} palette={palette} />
          </box>
        </box>
      </box>
      {browser === null ? null : (
        <>
          <box height={PANEL_GAP_ROWS} />
          <box flexDirection="row" height={browser.rows}>
            <box width={PANEL_MARGIN} />
            <box width={width - PANEL_MARGIN}>
              <SessionBrowser
                rows={rows}
                error={sessionsError}
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
      )}
    </box>
  );
}
