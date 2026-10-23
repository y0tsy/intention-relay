//! The application surface: one frame, one keyboard, one live panel.
//!
//! The frame is the only accent border in the app: whichever surface owns the
//! keyboard is painted inside it, so a user is never typing into a screen that
//! is not on screen. The composer, the hint band, and the status line stay put
//! while the transcript scrolls between them.

import { decodePasteBytes, type ScrollBoxRenderable } from "@opentui/core";
import { useKeyboard, usePaste, useRenderer, useTerminalDimensions } from "@opentui/react";
import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import type { ThemePreference } from "../app/action";
import { isCommandLine, notice, resolveCommand, themeValue } from "../app/commands";
import { PromptHistory } from "../app/history";
import type { AppStore } from "../app/store";
import type { AppState } from "../app/state";
import { paletteFor, type Palette } from "../theme/palette";
import { Composer } from "./Composer";
import {
  EMPTY_DRAFT,
  type Draft,
  backspace,
  deleteForward,
  insertNewline,
  insertText,
  moveCaret,
  replaceDraft,
} from "./editor";
import { HintBand } from "./HintBand";
import { SessionBrowser } from "./SessionBrowser";
import { StatusRow, type StatusFacts } from "./StatusRow";
import { THEME_ENTRIES, ThemePicker } from "./ThemePicker";
import { Transcript } from "./Transcript";
import { Welcome } from "./Welcome";

/** Subscribes one component to the store's version. */
function useStoreState(store: AppStore): AppState {
  useSyncExternalStore(
    (listener) => store.subscribe(listener),
    () => store.getVersion(),
  );
  return store.state;
}

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

/** Runs a one-second clock, which is the resolution the status line reads. */
function useNow(active: boolean): number {
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  useEffect(() => {
    if (!active) {
      return;
    }
    const timer = setInterval(() => {
      setNow(Math.floor(Date.now() / 1000));
    }, 1_000);
    return () => {
      clearInterval(timer);
    };
  }, [active]);
  return now;
}

/** The fullscreen application. */
export function App({ store, onQuit }: { store: AppStore; onQuit: () => void }) {
  const state = useStoreState(store);
  const palette = usePalette(state.theme);
  const { width, height } = useTerminalDimensions();
  const [draft, setDraft] = useState<Draft>(EMPTY_DRAFT);
  const [noticeText, setNoticeText] = useState<string | null>(null);
  const history = useMemo(() => new PromptHistory(), []);
  const scroll = useRef<ScrollBoxRenderable | null>(null);
  const live = state.liveRun !== null;
  const now = useNow(live);

  const quit = useCallback(() => {
    store.dispose();
    onQuit();
  }, [store, onQuit]);

  /** Hands one submitted line to the grammar or to the session as a turn. */
  const submit = useCallback(() => {
    const text = draft.text;
    if (text.trim().length === 0) {
      return;
    }
    history.push(text);
    setDraft(EMPTY_DRAFT);
    setNoticeText(null);
    const line = text.trim();
    if (!isCommandLine(line)) {
      store.dispatch({ kind: "turn_submitted", content: text });
      return;
    }
    const resolution = resolveCommand(line.slice(1));
    if (resolution.kind !== "invocation") {
      setNoticeText(notice(resolution));
      return;
    }
    const { spec, words } = resolution.invocation;
    switch (spec.action) {
      case "quit":
        quit();
        return;
      case "new_session":
        store.dispatch({ kind: "new_session_requested" });
        return;
      case "open_session": {
        const sessionId = words[0];
        if (sessionId !== undefined) {
          store.dispatch({ kind: "session_open_requested", sessionId });
        }
        return;
      }
      case "list_sessions":
        store.dispatch({ kind: "sessions_requested" });
        return;
      case "choose_theme": {
        const word = words[0];
        if (word === undefined) {
          store.dispatch({ kind: "theme_requested" });
          return;
        }
        const preference = themeValue(word);
        if (preference === undefined) {
          setNoticeText(
            `unknown theme "${word}"; known themes: ${THEME_ENTRIES.map((entry) => entry.preference).join(" ")}`,
          );
          return;
        }
        store.dispatch({ kind: "theme_selected", preference });
      }
    }
  }, [draft, history, quit, store]);

  useKeyboard((event) => {
    if (event.eventType === "release") {
      return;
    }
    if (state.surface !== "chat") {
      switch (event.name) {
        case "up":
        case "k":
          store.dispatch({ kind: "selection_moved", delta: -1 });
          return;
        case "down":
        case "j":
          store.dispatch({ kind: "selection_moved", delta: 1 });
          return;
        case "return":
          store.dispatch({ kind: "selection_committed" });
          return;
        case "escape":
          store.dispatch({ kind: "surface_closed" });
          return;
        default:
          return;
      }
    }

    if (event.ctrl && event.name === "d") {
      quit();
      return;
    }
    if (event.name === "return") {
      // A modified return is a line break: a prompt may be several lines, and
      // the unmodified key is what submits one.
      if (event.shift || event.ctrl) {
        setDraft((current) => insertNewline(current));
        return;
      }
      submit();
      return;
    }
    if (event.ctrl && event.name === "j") {
      setDraft((current) => insertNewline(current));
      return;
    }
    if (event.name === "escape") {
      if (state.liveRun !== null) {
        store.dispatch({ kind: "cancel_run_requested" });
        return;
      }
      setDraft(EMPTY_DRAFT);
      setNoticeText(null);
      return;
    }
    if (event.name === "backspace") {
      setDraft((current) => backspace(current));
      return;
    }
    if (event.name === "delete") {
      setDraft((current) => deleteForward(current));
      return;
    }
    if (event.name === "left") {
      setDraft((current) => moveCaret(current, -1));
      return;
    }
    if (event.name === "right") {
      setDraft((current) => moveCaret(current, 1));
      return;
    }
    if (event.name === "up" || event.name === "down") {
      const recalled = history.navigate(event.name === "up" ? -1 : 1);
      setDraft(recalled === null ? EMPTY_DRAFT : replaceDraft(recalled));
      return;
    }
    if (event.name === "pageup" || event.name === "pagedown") {
      const step = Math.max(1, Math.floor(height / 2));
      scroll.current?.scrollBy({ x: 0, y: event.name === "pageup" ? -step : step });
      return;
    }
    const text = typed(event);
    if (text !== null) {
      setDraft((current) => insertText(current, text));
    }
  });

  usePaste((event) => {
    if (state.surface !== "chat") {
      return;
    }
    // A paste is data, never a gesture: it lands in the draft whole, and its
    // newlines are the prompt's own.
    setDraft((current) => insertText(current, decodePasteBytes(event.bytes)));
  });

  const inner = width - 2;
  const facts: StatusFacts = {
    sessionId: state.sessionId,
    runStatus: state.runStatus,
    startedAt: state.activeRun?.started_at ?? null,
    answering: state.provisionalText.length > 0,
    tokens:
      state.activeRun?.usage?.state === "reported" ? state.activeRun.usage.total_tokens : null,
    now,
  };
  const empty = state.transcript.length === 0 && state.provisionalText.length === 0;

  return (
    <box
      flexDirection="column"
      width="100%"
      height="100%"
      backgroundColor={palette.background}
      borderStyle="rounded"
      border
      borderColor={palette.accent}
      title="intention relay"
      titleAlignment="left"
    >
      {state.surface === "sessions" ? (
        <SessionBrowser
          summaries={state.sessions}
          error={state.sessionsError}
          selection={state.selection}
          palette={palette}
          width={inner}
          height={height - 2}
          now={now}
        />
      ) : state.surface === "theme" ? (
        <ThemePicker
          selection={state.selection}
          current={state.theme}
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
              stickyScroll
              stickyStart="bottom"
              contentOptions={{ paddingLeft: 1, paddingRight: 1 }}
            >
              <Transcript
                messages={state.transcript}
                provisionalReasoning={state.provisionalReasoning}
                provisionalText={state.provisionalText}
                palette={palette}
              />
            </scrollbox>
          )}
          {state.error === null ? null : (
            <box paddingLeft={1} paddingRight={1}>
              <text wrapMode="word" fg={palette.error}>
                {state.error}
              </text>
            </box>
          )}
          {noticeText === null ? null : (
            <box paddingLeft={1} paddingRight={1}>
              <text wrapMode="word" fg={palette.warning}>
                {noticeText}
              </text>
            </box>
          )}
          <HintBand width={inner} palette={palette} />
          <Composer draft={draft} palette={palette} focused />
          <StatusRow facts={facts} palette={palette} width={inner} />
        </box>
      )}
    </box>
  );
}

/**
 * Returns the text one key press types, or `null` for a key that is a gesture.
 *
 * A named key carries a multi-character name and never reaches the draft; a
 * single character is what a user typed, whatever the layout maps it to.
 */
export function typed(event: { name: string; ctrl: boolean; meta: boolean }): string | null {
  if (event.ctrl || event.meta) {
    return null;
  }
  if (event.name === "space") {
    return " ";
  }
  return [...event.name].length === 1 ? event.name : null;
}
