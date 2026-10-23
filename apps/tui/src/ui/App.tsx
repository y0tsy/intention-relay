//! The application: one window, one keyboard, one panel.
//!
//! The frame is the canvas the terminal paints plus the chat panel on it, with a
//! one-cell margin so the window's own background stays visible around the panel.
//! The panel owns the transcript (or the welcome surface), the hint region, the
//! input block, and the notice line; a docked surface - the session browser -
//! takes rows from the bottom of the window and the panel shrinks above it, so no
//! surface ever paints over another.
//!
//! The app subscribes to what decides the frame's composition and to nothing that
//! moves with the stream: the panel, the input block, the status row, and the
//! transcript each subscribe to their own values.

import { useKeyboard, usePaste, useRenderer, useTerminalDimensions } from "@opentui/react";
import { useEffect, useState } from "react";
import type { ThemePreference } from "../app/action";
import { selectCommandMenu, selectSurface, selectTheme } from "../app/select";
import type { AppStore } from "../app/store";
import { type Palette, paletteFor } from "../theme/palette";
import { ChatPanel } from "./ChatPanel";
import { type CopyText, useCopyOnSelect } from "./copy";
import { keyIntent, pastedText } from "./keys";
import { useStore } from "./use-store";

/** Resolves the palette the app paints with. */
function usePalette(preference: ThemePreference): Palette {
  const renderer = useRenderer();
  const [terminal, setTerminal] = useState<"light" | "dark" | null>(null);
  useEffect(() => {
    if (preference !== "auto") {
      return;
    }
    let alive = true;
    // The terminal answers an OSC 11 query with its background colour; a terminal
    // that never answers leaves the dark palette in force.
    void renderer.waitForThemeMode(400).then((mode) => {
      if (alive && (mode === "light" || mode === "dark")) {
        setTerminal(mode);
      }
    });
    return () => {
      alive = false;
    };
  }, [preference, renderer]);
  const mode = preference === "auto" ? (terminal ?? "dark") : preference;
  return paletteFor(mode);
}

/**
 * The fullscreen application.
 *
 * `copy` names where a finished text selection goes; the running app leaves it
 * out and writes to the clipboard the terminal and the host offer.
 */
export function App({ store, copy }: { store: AppStore; copy?: CopyText }) {
  const surface = useStore(store, selectSurface);
  const theme = useStore(store, selectTheme);
  const menu = useStore(store, selectCommandMenu);
  const palette = usePalette(theme);
  const { width, height } = useTerminalDimensions();
  useCopyOnSelect(copy);

  useKeyboard((event) => {
    const intent = keyIntent(event, { surface, menu: menu !== null, height });
    switch (intent.kind) {
      case "ignore":
        return;
      case "interrupt":
        store.dispatch({ kind: "ctrl_c_pressed" });
        return;
      case "escape":
        store.dispatch({ kind: "escape_pressed" });
        return;
      case "submit":
        store.dispatch({ kind: "input_submitted" });
        return;
      case "newline":
        store.dispatch({ kind: "draft_newline" });
        return;
      case "accept_menu":
        store.dispatch({ kind: "menu_accepted" });
        return;
      case "edit":
        store.dispatch({ kind: "draft_edited", edit: intent.edit });
        return;
      case "recall":
        store.dispatch({ kind: "history_recalled", delta: intent.delta });
        return;
      case "scroll":
        // The transcript owns its own scroll offset, so a wheel notch moves the
        // one it holds and the panel keeps its rows.
        return;
      case "insert":
        store.dispatch({ kind: "draft_typed", text: intent.text });
        return;
      case "move_selection":
        store.dispatch({ kind: "selection_moved", delta: intent.delta });
        return;
      case "commit_selection":
        store.dispatch({ kind: "selection_committed" });
        return;
      case "close_surface":
        store.dispatch({ kind: "surface_closed" });
        return;
      case "expand_reasoning":
        store.dispatch({ kind: "reasoning_expanded", id: null });
        return;
      case "filter_typed":
        store.dispatch({ kind: "browser_filter_typed", text: intent.text });
        return;
      case "filter_backspace":
        store.dispatch({ kind: "browser_filter_backspace" });
        return;
      case "switch_tab":
        store.dispatch({ kind: "browser_tab_moved", delta: intent.delta });
        return;
      case "move_menu":
        store.dispatch({ kind: "menu_moved", delta: intent.delta });
        return;
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
    <box width="100%" height="100%" backgroundColor={palette.canvas}>
      <ChatPanel store={store} palette={palette} width={width} height={height} />
    </box>
  );
}
