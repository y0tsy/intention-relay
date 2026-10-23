//! The application surface: one frame, one keyboard, one live panel.
//!
//! The frame is the only accent border in the app: whichever surface owns the
//! keyboard is painted inside it, so a user is never typing into a screen that
//! is not on screen. The composer, the hint band, and the status line stay put
//! while the transcript scrolls between them.
//!
//! The app subscribes to what decides the frame's composition - the surface, the
//! palette, whether the transcript is still empty, the two transient lines, and
//! whether a run is live - and to nothing that moves with the stream. The rows,
//! the tail, the composer, and the status line each subscribe to their own
//! values, so a streamed frame repaints the tail and a keystroke repaints the
//! composer.

import type { ScrollBoxRenderable } from "@opentui/core";
import { useKeyboard, usePaste, useRenderer, useTerminalDimensions } from "@opentui/react";
import { useEffect, useRef, useState } from "react";
import type { ThemePreference } from "../app/action";
import {
  selectEmpty,
  selectError,
  selectLive,
  selectNotice,
  selectSelection,
  selectSessions,
  selectSessionsError,
  selectSurface,
  selectTheme,
} from "../app/select";
import type { AppStore } from "../app/store";
import { paletteFor, type Palette } from "../theme/palette";
import { CommittedList } from "./CommittedList";
import { Composer } from "./Composer";
import { HintBand } from "./HintBand";
import { keyIntent, pastedText } from "./keys";
import { ProvisionalTail } from "./ProvisionalTail";
import { SessionBrowser } from "./SessionBrowser";
import { StatusLine } from "./StatusRow";
import { ThemePicker } from "./ThemePicker";
import { useNow } from "./use-now";
import { useStore } from "./use-store";
import { Welcome } from "./Welcome";

/** Resolves the palette the app paints with. */
function usePalette(preference: ThemePreference): Palette {
  const renderer = useRenderer();
  const [terminal, setTerminal] = useState<"light" | "dark" | null>(null);
  useEffect(() => {
    if (preference !== "auto") {
      return;
    }
    let alive = true;
    // The terminal answers an OSC 11 query with its background colour; a
    // terminal that never answers leaves the dark palette in force.
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

/** Renders one transient line under the transcript. */
function NoticeLine({ text, color }: { text: string; color: string }) {
  return (
    <box paddingLeft={1} paddingRight={1}>
      <text wrapMode="word" fg={color}>
        {text}
      </text>
    </box>
  );
}

/** The fullscreen application. */
export function App({ store }: { store: AppStore }) {
  const surface = useStore(store, selectSurface);
  const theme = useStore(store, selectTheme);
  const empty = useStore(store, selectEmpty);
  const error = useStore(store, selectError);
  const notice = useStore(store, selectNotice);
  const live = useStore(store, selectLive);
  const sessions = useStore(store, selectSessions);
  const sessionsError = useStore(store, selectSessionsError);
  const selection = useStore(store, selectSelection);
  // The browser prints how long ago a session moved, so the clock runs while it
  // is the surface on screen and nowhere else.
  const now = useNow(surface === "sessions");
  const palette = usePalette(theme);
  const { width, height } = useTerminalDimensions();
  const scroll = useRef<ScrollBoxRenderable | null>(null);

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
        scroll.current?.scrollBy({ x: 0, y: intent.delta });
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

  const inner = width - 2;
  return (
    <box
      flexDirection="column"
      width="100%"
      height="100%"
      backgroundColor={palette.canvas}
      borderStyle="rounded"
      border
      borderColor={palette.accent}
      title="intention relay"
      titleAlignment="left"
    >
      {surface === "sessions" ? (
        <SessionBrowser
          summaries={sessions}
          error={sessionsError}
          selection={selection}
          palette={palette}
          width={inner}
          height={height - 2}
          now={now}
        />
      ) : surface === "theme" ? (
        <ThemePicker
          selection={selection}
          current={theme}
          palette={palette}
          width={inner}
          height={height - 2}
        />
      ) : (
        <box flexDirection="column" flexGrow={1}>
          {empty ? (
            <Welcome
              width={inner}
              height={height - 5}
              workspace={store.workspaceRoot}
              palette={palette}
            />
          ) : (
            <scrollbox
              ref={scroll}
              flexGrow={1}
              // The scrollbox takes the rows the panels below it leave, never
              // the rows its content asks for: a long transcript scrolls
              // inside the frame instead of pushing the composer out of it.
              flexBasis={0}
              stickyScroll
              stickyStart="bottom"
              contentOptions={{ paddingLeft: 1, paddingRight: 1 }}
            >
              <box flexDirection="column" gap={1}>
                <CommittedList store={store} palette={palette} />
                <ProvisionalTail store={store} palette={palette} />
              </box>
            </scrollbox>
          )}
          {error === null ? null : <NoticeLine text={error} color={palette.error} />}
          {notice === null ? null : <NoticeLine text={notice} color={palette.warning} />}
          <HintBand width={inner} palette={palette} />
          <Composer store={store} palette={palette} focused />
          <StatusLine store={store} palette={palette} width={inner} />
        </box>
      )}
    </box>
  );
}
