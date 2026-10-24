//! The footer: the live band the window keeps under the printed transcript.
//!
//! The transcript is the terminal's scrollback, so the rows the application
//! paints are the ones that can still change: the hint band or the theme
//! picker's panel, the step in flight, the composer, and the notice line. A
//! launch paints the open welcome page with its own header; an open session
//! keeps the closing rounded frame around the step and the composer.
//!
//! The kernel plans the footer's rows and paints its surfaces; this module only
//! names what goes in each region. The footer tells the renderer how many rows
//! it took, and above them the terminal owns the rows the transcript printed.
//! The session browser is a layout row docked under the footer, and the footer
//! reflows into the rows the browser leaves.

import {
  selectBrowserRows,
  selectCommandMenu,
  selectDraft,
  selectEmpty,
  selectMenuHighlight,
  selectSelection,
  selectSurface,
  selectTheme,
} from "../app/select";
import type { AppStore } from "../app/store";
import {
  browserPlan,
  Center,
  Columns,
  composerSlotRows,
  DOCK_GAP_ROWS,
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
import { Docked, Hints, hintRows } from "./footer-parts";
import { InputBlock } from "./InputBlock";
import { Detail } from "./StatusRow";
import { Live } from "./Transcript";
import { useStore } from "./use-store";
import { Welcome, WelcomeHeader } from "./Welcome";
import { WelcomeComposer } from "./WelcomeComposer";

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
  const welcome = mode === "welcome";
  const layout = footerPlan(
    width,
    windowHeight,
    welcome ? composerSlotRows(draft.text) : inputSlotRows(draft.text),
    hintRows(menu, surface),
    mode,
    browser === null ? 0 : DOCK_GAP_ROWS + browser.rows,
    printedRows,
  );
  return (
    <FooterFrame
      layout={layout}
      welcome={welcome}
      palette={palette}
      header={welcome ? <WelcomeHeader store={store} palette={palette} /> : undefined}
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
        {welcome ? (
          <Columns count={1} limit={146} align="center">
            <WelcomeComposer store={store} palette={palette} />
          </Columns>
        ) : (
          <InputBlock store={store} palette={palette} />
        )}
      </SlotRows>
      <SlotRows rows={layout.detailRows}>
        <Slot>
          {welcome ? (
            <Center>
              <Detail store={store} palette={palette} />
            </Center>
          ) : (
            <Detail store={store} palette={palette} />
          )}
        </Slot>
      </SlotRows>
    </FooterFrame>
  );
}
