//! The application: one window, one keyboard, one live footer.
//!
//! The frame is the canvas the terminal paints plus the footer on it, with a
//! one-cell margin so the window's own background stays visible above the frame.
//! Everything the footer does not repaint belongs to the terminal: the printed
//! transcript scrolls with the terminal's own history, so the wheel, the
//! scrollbar, and a drag over the text are the terminal's, never the
//! application's.
//!
//! The app subscribes to what decides the frame's composition and to nothing that
//! moves with the stream: the footer, the input block, the status row, and the
//! step in flight each subscribe to their own values.

import { useKeyboard, usePaste } from "@opentui/react";
import { selectCommandMenu, selectSurface, selectTheme } from "../app/select";
import type { AppStore } from "../app/store";
import { Screen, useTerminalTheme } from "../kit";
import { paletteFor } from "../theme/palette";
import { Footer } from "./Footer";
import { intentAction, keyIntent } from "../app/keys";
import { pastedText } from "./paste";
import { Printer } from "./Printer";
import { useStore } from "./use-store";

/** The application: the printed transcript, then the footer that owns the keys. */
export function App({ store }: { store: AppStore }) {
  const surface = useStore(store, selectSurface);
  const theme = useStore(store, selectTheme);
  const menu = useStore(store, selectCommandMenu);
  // A terminal that never answers the theme query leaves the dark palette in
  // force.
  const terminalTheme = useTerminalTheme(theme === "auto");
  const palette = paletteFor(theme === "auto" ? (terminalTheme ?? "dark") : theme);

  useKeyboard((event) => {
    const action = intentAction(keyIntent(event, { surface, menu: menu !== null }));
    if (action !== null) {
      store.dispatch(action);
    }
  });

  usePaste((event) => {
    if (surface !== "chat") {
      return;
    }
    // A paste is data, never a gesture: it lands in the draft whole, and its
    // newlines are the prompt's own.
    store.dispatch({ kind: "draft_pasted", text: pastedText(event.bytes) });
  });

  return (
    <Screen
      background={palette.canvas}
      footer={(printedRows) => <Footer store={store} palette={palette} printedRows={printedRows} />}
      printer={(onPrinted) => <Printer store={store} palette={palette} onPrinted={onPrinted} />}
    />
  );
}
