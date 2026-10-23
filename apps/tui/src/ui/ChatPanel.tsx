//! The chat panel: one rounded frame whose body holds the transcript - or the
//! welcome pane while no session is open - the hint region, the input block, and
//! the notice line, in that order.
//!
//! The panel is the only container the chat has, and its four inside regions sum
//! to its body exactly: the hint region is a band of this panel, never a surface
//! painted over another, and the input block and the notice line never move. The
//! rows and columns come from [`panelLayout`], so a frame is the same at every
//! window size and no child is squeezed into another's rows.

import { type CommandMenu, menuRows } from "../app/menu";
import {
  selectCommandMenu,
  selectDraft,
  selectEmpty,
  selectSelection,
  selectSessions,
  selectSessionsError,
  selectSurface,
  selectTheme,
} from "../app/select";
import type { Surface } from "../app/state";
import type { AppStore } from "../app/store";
import type { Palette } from "../theme/palette";
import { HintBand } from "./HintBand";
import { InputBlock } from "./InputBlock";
import { inputBlockRows, panelLayout } from "./panel";
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

/** Renders the chat panel in the window it was given. */
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
  const empty = useStore(store, selectEmpty);
  const surface = useStore(store, selectSurface);
  const sessions = useStore(store, selectSessions);
  const sessionsError = useStore(store, selectSessionsError);
  const selection = useStore(store, selectSelection);
  const theme = useStore(store, selectTheme);
  // The browser prints how long ago a session moved, so the clock runs while it
  // is on screen and nowhere else.
  const now = useNow(surface === "sessions");
  const breaks = draft.text.split("\n").length - 1;
  const layout = panelLayout(width, height, inputBlockRows(breaks), hintRows(menu, surface));
  if (layout.rows < 3) {
    // A window too small to frame anything draws nothing rather than a broken
    // box: the panel's own arithmetic has already spent its body.
    return <box />;
  }
  return (
    <box flexDirection="row">
      <box width={1} />
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
          {surface === "sessions" ? (
            <SessionBrowser
              summaries={sessions}
              error={sessionsError}
              selection={selection}
              palette={palette}
              width={layout.contentColumns}
              height={layout.transcriptRows}
              now={now}
            />
          ) : empty ? (
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
              <HintBand menu={menu} palette={palette} width={layout.contentColumns} />
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
  );
}
