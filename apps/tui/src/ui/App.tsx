//! The application surface: one frame, one keyboard, one live panel.
//!
//! The frame is the only accent border in the app: whichever surface owns the
//! keyboard is painted inside it, so a user is never typing into a screen that
//! is not on screen. The composer, the hint band, and the status line stay put
//! while the transcript scrolls between them.

import type { ScrollBoxRenderable } from "@opentui/core";
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
  editDraft,
  insertNewline,
  insertText,
  replaceDraft,
} from "./editor";
import { HintBand } from "./HintBand";
import { keyIntent, pastedText } from "./keys";
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
    const intent = keyIntent(event, {
      surface: state.surface,
      live: state.liveRun !== null,
      height,
    });
    switch (intent.kind) {
      case "ignore":
        return;
      case "quit":
        quit();
        return;
      case "submit":
        submit();
        return;
      case "newline":
        setDraft((current) => insertNewline(current));
        return;
      case "stop_run":
        store.dispatch({ kind: "cancel_run_requested" });
        return;
      case "clear_draft":
        setDraft(EMPTY_DRAFT);
        setNoticeText(null);
        return;
      case "edit":
        setDraft((current) => editDraft(current, intent.edit));
        return;
      case "recall": {
        const recalled = history.navigate(intent.delta);
        setDraft(recalled === null ? EMPTY_DRAFT : replaceDraft(recalled));
        return;
      }
      case "scroll":
        scroll.current?.scrollBy({ x: 0, y: intent.delta });
        return;
      case "insert":
        setDraft((current) => insertText(current, intent.text));
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
    if (state.surface !== "chat") {
      return;
    }
    // A paste is data, never a gesture: it lands in the draft whole, and its
    // newlines are the prompt's own.
    setDraft((current) => insertText(current, pastedText(event.bytes)));
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
