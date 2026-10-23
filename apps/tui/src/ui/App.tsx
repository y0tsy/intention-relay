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
import { selectLive, selectSurface, selectTheme } from "../app/select";
import type { AppStore } from "../app/store";
import { type Palette, paletteFor } from "../theme/palette";
import { ChatPanel } from "./ChatPanel";
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

/** The fullscreen application. */
export function App({ store }: { store: AppStore }) {
  const surface = useStore(store, selectSurface);
  const theme = useStore(store, selectTheme);
  const live = useStore(store, selectLive);
  const palette = usePalette(theme);
  const { width, height } = useTerminalDimensions();

  useKeyboard((event) => {
    const intent = keyIntent(event, { surface, live, height });
    switch (intent.kind) {
      case "ignore":
        return;
      case "quit":
        store.dispatch({ kind: "quit_requested" });
        return;
      case "submit":
        store.dispatch({ kind: "input_submitted" });
        return;
      case "newline":
        store.dispatch({ kind: "draft_newline" });
        return;
      case "stop_run":
        store.dispatch({ kind: "cancel_run_requested" });
        return;
      case "clear_draft":
        store.dispatch({ kind: "draft_cleared" });
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
    }
  });

  usePaste((event) => {
    if (surface !== "chat") {
      return;
    }
    // A paste is data, never a gesture: it lands in the draft whole, and its
    // newlines are the prompt's own.
    store.dispatch({ kind: "draft_typed", text: pastedText(event.bytes) });
  });

  return (
    <box flexDirection="column" width="100%" height="100%" backgroundColor={palette.canvas}>
      <box height={1} />
      <ChatPanel store={store} palette={palette} width={width} height={height} />
    </box>
  );
}
