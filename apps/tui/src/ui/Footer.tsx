//! The footer: the edge and the tray the window keeps under the printed transcript.
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
  FOOTER_MARGIN,
  FooterFrame,
  type FooterMode,
  footerPlan,
  inputSlotRows,
  pagePlan,
  Scroll,
  Slot,
  SlotRows,
  Spacer,
  useFooterRows,
  useWindow,
} from "../kit";
import type { Palette } from "../theme/palette";
import { Docked, Hints, hintRows } from "./footer-parts";
import { InputBlock } from "./InputBlock";
import { Detail } from "./StatusRow";
import { LiveTail } from "./Transcript";
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
  // A page that has printed a row keeps the rows it has: a footer that grew from
  // here would scroll the row out of the terminal's scrollback. Only a page with
  // nothing above the footer may take the whole window.
  const footerRows = useFooterRows();
  const draft = useStore(store, selectDraft);
  const menu = useStore(store, selectCommandMenu);
  const menuHighlight = useStore(store, selectMenuHighlight);
  const empty = useStore(store, selectEmpty);
  const surface = useStore(store, selectSurface);
  const selection = useStore(store, selectSelection);
  const rows = useStore(store, selectBrowserRows);
  const theme = useStore(store, selectTheme);
  // The browser is a page of its own: it replaces the tray while it is open, so
  // the composer and the band wait, and it plans inside the rows the footer has
  // now. A band, a longer draft, and a notice keep to the rows the footer has,
  // because none of them is worth a printed row.
  const mode: FooterMode = empty ? "welcome" : "session";
  const welcome = mode === "welcome";
  // The tray the page replaces: its own rows are what the browser may keep, and
  // they come from the plan rather than from the renderer, so one frame's height
  // never decides the next frame's.
  const tray = footerPlan(
    width,
    windowHeight,
    welcome ? composerSlotRows(draft.text) : inputSlotRows(draft.text),
    hintRows(menu, surface),
    mode,
    0,
    printedRows,
    printedRows === 0 ? windowHeight : footerRows,
  );
  const browser =
    surface === "sessions" ? browserPlan(width, windowHeight, rows.length, tray.rows) : null;
  const browserNote =
    surface === "sessions" && browser === null
      ? "This window is too short to list the sessions · esc closes it"
      : undefined;
  // The page keeps the window the tray had: a footer that changed height here
  // would scroll its region, and the rows above a shrink keep the paint they
  // were left with until a print reaches them.
  const layout =
    browser === null
      ? tray
      : pagePlan(width, windowHeight, browser.rows + DOCK_GAP_ROWS + FOOTER_MARGIN, tray.rows);
  const live =
    welcome || layout.liveRows === 0 ? null : (
      <SlotRows rows={layout.liveRows}>
        <Slot>
          <Scroll>
            <LiveTail store={store} palette={palette} />
            {/* A print leads with a blank row between two blocks, and the page
                the printer measured carries it: the edge stands one row lower
                than the text it holds, or the row the print is about to write
                is already on the page as the edge's first row. */}
            {printedRows > 0 ? <Spacer rows={1} /> : null}
          </Scroll>
        </Slot>
      </SlotRows>
    );
  return (
    <FooterFrame
      layout={layout}
      welcome={welcome}
      palette={palette}
      header={welcome ? <WelcomeHeader store={store} palette={palette} /> : undefined}
      live={live}
      page={
        browser === null
          ? undefined
          : {
              node: <Docked store={store} palette={palette} browser={browser} />,
              // The page node carries the blank row above the panel itself;
              // the page branch must not budget the same gap again or the
              // bottom margin falls off the window.
              rows: browser.rows + DOCK_GAP_ROWS,
            }
      }
    >
      {welcome && layout.liveRows > 0 ? (
        <SlotRows rows={layout.liveRows}>
          <Slot>
            <Welcome palette={palette} />
          </Slot>
        </SlotRows>
      ) : null}
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
      {/* A slot the plan spent nothing on paints nothing: the composer owns its
          own frame, and a frame with no rows of its own to stand in paints over
          the row the band and the detail line were given. */}
      {layout.inputRows === 0 ? null : (
        <SlotRows rows={layout.inputRows}>
          {welcome ? (
            <Columns count={1} limit={146} align="center">
              <WelcomeComposer store={store} palette={palette} />
            </Columns>
          ) : (
            <InputBlock store={store} palette={palette} />
          )}
        </SlotRows>
      )}
      {layout.detailRows === 0 ? null : (
        <SlotRows rows={layout.detailRows}>
          <Slot>
            {welcome ? (
              <Center>
                <Detail store={store} palette={palette} notice={browserNote} />
              </Center>
            ) : (
              <Detail store={store} palette={palette} notice={browserNote} />
            )}
          </Slot>
        </SlotRows>
      )}
    </FooterFrame>
  );
}
