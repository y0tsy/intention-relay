//! The render-free state machine every front end drives.
//!
//! The state owns the open session, the client's committed state of it, the
//! status of the one stream that observes it, and the input line itself. A front
//! end reads the state and mutates it only through [`AppState.update`], which
//! returns the client effects the transition asks for and never throws: a
//! failure becomes the error line instead of an error return, so no caller has
//! to recover from a transition.

import type {
  ErrorDto,
  MessageProjectionDto,
  RunProjectionDto,
  RunStatusDto,
  SendUserTurnOutcomeDto,
  SessionId,
  SessionSnapshotDto,
  SessionSummariesDto,
} from "../bindings";
import { SessionStreamState } from "../client/stream";
import { type Action, THEME_PREFERENCES, type ThemePreference } from "./action";
import { isCommandLine, notice, resolveCommand, themeValue } from "./commands";
import {
  type Draft,
  EMPTY_DRAFT,
  editDraft,
  insertNewline,
  insertText,
  replaceDraft,
} from "./editor";
import type { Effect } from "./effect";
import { PromptHistory } from "./history";

/** Which surface owns the keyboard. */
export type Surface = "chat" | "sessions" | "theme";

/** How far the one live channel of a session has got. */
export type StreamStatus = "inactive" | "live" | "failed" | "ended";

/**
 * The rows of a session this front end has not opened.
 *
 * The empty list is one value, not one per read: a view subscribes to the rows
 * and compares the value it was handed, so a selector that built a fresh empty
 * list on every call would repaint forever.
 */
const NO_ROWS: readonly MessageProjectionDto[] = [];

/** Every value one front end holds, with no terminal attached. */
export class AppState {
  readonly initialSession: SessionId | null;
  sessionId: SessionId | null = null;
  /**
   * The client's state of the open session, if any.
   *
   * The client owns the committed transcript - its rows, its one retention
   * bound, its run projection, and its transient text - so this front end holds
   * no second copy of any of it.
   */
  sessionState: SessionStreamState | null = null;
  /**
   * How many reconnects this front end has asked for the current stream.
   *
   * The budget is one reconnect per lost stream: the first loss asks for it and
   * a second loss inside the same recovery asks for nothing, so the front end
   * reports its typed failure instead of retrying in a loop. Only a frame the
   * state accepts restores it: a subscription that opened and delivered no
   * traffic leaves it spent.
   */
  reconnectsAsked = 0;
  stream: StreamStatus = "inactive";
  error: string | null = null;
  /**
   * The surface that owns the keyboard.
   *
   * Exactly one surface owns it at a time, and that ownership is what the accent
   * border marks, so a screen a user is typing into is always the one that is
   * painted.
   */
  surface: Surface = "chat";
  /** The session list the browser shows, once the daemon has answered. */
  sessions: SessionSummariesDto | null = null;
  /** The failure the session browser reports instead of a list. */
  sessionsError: string | null = null;
  /** The index the surface that owns the keyboard has highlighted. */
  selection = 0;
  /** The palette preference the configuration file declares. */
  theme: ThemePreference;
  /**
   * The prompt line the composer shows, and where its caret sits.
   *
   * The input line is the state's own: a keystroke reaches the draft through an
   * action, so typing never touches the committed rows a transcript lays out.
   */
  draft: Draft = EMPTY_DRAFT;
  /** The one-line notice the chat surface shows instead of an effect. */
  notice: string | null = null;
  /** The rows {@link AppState.transcript} handed out last, and their revision. */
  #rows: readonly MessageProjectionDto[] = NO_ROWS;
  #rowsSource: SessionStreamState | null = null;
  #rowsRevision = -1;
  /** The prompts this front end has submitted, for the arrow keys to walk. */
  readonly #history = new PromptHistory();

  /**
   * Creates the state a front end starts from.
   *
   * `initialSession` is the session the caller selected, or the session the
   * client resolved for an explicit continuation; `null` opens nothing.
   * `initialTheme` is the preference the configuration file declared.
   */
  constructor(initialSession: SessionId | null, initialTheme: ThemePreference = "auto") {
    this.initialSession = initialSession;
    this.theme = initialTheme;
  }

  /** Applies one action and returns the client effects it asks for. */
  update(action: Action): readonly Effect[] {
    switch (action.kind) {
      case "connect_requested":
        return [{ kind: "connect" }];
      case "bootstrapped":
        return this.#applyBootstrapped();
      case "bootstrap_failed":
        return this.#applyFailure(action.error);
      case "session_snapshot_loaded":
        return this.#applySessionSnapshot(action.snapshot);
      case "session_snapshot_failed":
        return this.#applyFailure(action.error);
      case "session_created":
        return this.#applySessionCreated(action.sessionId);
      case "session_create_failed":
        return this.#applyFailure(action.error);
      case "session_stream_opened":
        return this.#applySessionStreamOpened(action.state);
      case "session_stream_failed":
        return this.#applySessionStreamFailed(action.error);
      case "session_stream_ended":
        return this.#applySessionStreamEnded();
      case "frame_received":
        return this.#applyFrame(action.state);
      case "turn_accepted":
        return this.#applyTurnAccepted(action.outcome);
      case "turn_failed":
        return this.#applyFailure(action.error);
      case "run_cancel_accepted":
        return [];
      case "run_cancel_failed":
      case "effect_failed":
        return this.#applyFailure(action.error);
      case "draft_typed":
        this.draft = insertText(this.draft, action.text);
        return [];
      case "draft_edited":
        this.draft = editDraft(this.draft, action.edit);
        return [];
      case "draft_newline":
        this.draft = insertNewline(this.draft);
        return [];
      case "draft_cleared":
        this.draft = EMPTY_DRAFT;
        this.notice = null;
        return [];
      case "history_recalled":
        return this.#applyHistoryRecalled(action.delta);
      case "notice_shown":
        this.notice = action.text;
        return [];
      case "input_submitted":
        return this.#applyInputSubmitted();
      case "quit_requested":
        return [{ kind: "quit" }];
      case "new_session_requested":
        return [{ kind: "create_session", firstTurn: null }];
      case "session_open_requested":
        return this.#applySessionOpenRequested(action.sessionId);
      case "cancel_run_requested":
        return this.#requestCancel();
      case "sessions_requested":
        return this.#applySessionsRequested();
      case "sessions_listed":
        this.sessions = action.summaries;
        this.sessionsError = null;
        this.selection = 0;
        return [];
      case "sessions_list_failed":
        this.sessions = null;
        this.sessionsError = `${action.error.code}: ${action.error.message}`;
        return [];
      case "selection_moved":
        return this.#applySelectionMoved(action.delta);
      case "selection_committed":
        return this.#applySelectionCommitted();
      case "surface_closed":
        return this.#applySurfaceClosed();
      case "theme_requested":
        return this.#applyThemeRequested();
      case "theme_selected":
        return this.#applyThemeSelected(action.preference);
    }
  }

  /** Returns the number of entries the surface that owns the keyboard offers. */
  get optionCount(): number {
    if (this.surface === "sessions") {
      return this.sessions?.sessions.length ?? 0;
    }
    if (this.surface === "theme") {
      return THEME_PREFERENCES.length;
    }
    return 0;
  }

  /** Returns the session the browser has highlighted. */
  get selectedSession(): SessionId | null {
    return this.sessions?.sessions[this.selection]?.session_id ?? null;
  }

  /** Returns the theme preference the picker has highlighted. */
  get selectedTheme(): ThemePreference | null {
    return THEME_PREFERENCES[this.selection] ?? null;
  }

  /**
   * Returns the committed transcript rows of the current session.
   *
   * The client mutates its rows in place, so the rows a view compares are copied
   * once per revision: the same read hands back the same list, and an appended
   * row hands back a new one.
   */
  get transcript(): readonly MessageProjectionDto[] {
    const state = this.sessionState;
    if (state === null) {
      return NO_ROWS;
    }
    if (this.#rowsSource !== state || this.#rowsRevision !== state.revision) {
      this.#rowsSource = state;
      this.#rowsRevision = state.revision;
      this.#rows = state.messages.slice();
    }
    return this.#rows;
  }

  /** Returns the last known run projection. */
  get activeRun(): RunProjectionDto | null {
    return this.sessionState?.run ?? null;
  }

  /** Returns the last known run lifecycle status. */
  get runStatus(): RunStatusDto | null {
    return this.sessionState?.status ?? null;
  }

  /** Returns the transient answer text of the model step in flight. */
  get provisionalText(): string {
    return this.sessionState?.provisionalText ?? "";
  }

  /** Returns the transient reasoning text of the model step in flight. */
  get provisionalReasoning(): string {
    return this.sessionState?.provisionalReasoning ?? "";
  }

  /**
   * Returns the run this front end currently observes as live.
   *
   * A run counts as live only while a live subscription observes it: the
   * projection outlives the stream when the daemon dies, and a front end that
   * trusted it would keep offering to stop a run nobody runs any more.
   */
  get liveRun(): RunProjectionDto | null {
    if (this.stream !== "live") {
      return null;
    }
    const run = this.activeRun;
    if (run === null || isTerminal(run.status)) {
      return null;
    }
    return run;
  }

  /** Records a ready daemon and asks for the selected session. */
  #applyBootstrapped(): readonly Effect[] {
    this.error = null;
    const sessionId = this.sessionId ?? this.initialSession;
    return sessionId === null ? [] : [{ kind: "open_session", sessionId }];
  }

  /** Shows one session snapshot and opens the one subscription it needs. */
  #applySessionSnapshot(snapshot: SessionSnapshotDto): readonly Effect[] {
    const read = SessionStreamState.fromSnapshot(snapshot);
    this.#mergeTranscript(read);
    this.sessionId = snapshot.session_id;
    this.stream = "inactive";
    this.error = null;
    // Opening a session records no notice: the notice line stays free for real
    // transient events.
    return this.#ensureSessionStream();
  }

  /** Opens the session the daemon just created. */
  #applySessionCreated(sessionId: SessionId): readonly Effect[] {
    this.error = null;
    return [{ kind: "open_session", sessionId }];
  }

  /**
   * Merges one read of a session's durable transcript into the client state.
   *
   * Rows are matched by durable identity, so a read of the session this front
   * end already holds joins the rows it has; a read of another session seeds the
   * state, because a front end holds exactly one session's rows.
   */
  #mergeTranscript(read: SessionStreamState): void {
    if (this.sessionId !== read.sessionId || this.sessionState === null) {
      this.sessionState = read;
      return;
    }
    this.sessionState.reconcile(read);
  }

  /**
   * Takes the client's committed state of a freshly opened subscription.
   *
   * An accepted subscription proves only that a stream exists: the reconnect
   * budget a loss spent stays spent until a frame the state accepts shows the
   * stream is carrying the session's traffic.
   */
  #applySessionStreamOpened(state: SessionStreamState): readonly Effect[] {
    this.sessionState = state;
    this.stream = "live";
    this.error = null;
    return [];
  }

  /**
   * Records a subscription that failed to open or stay live.
   *
   * The stream is this front end's one live channel, so its loss is not a dead
   * end: the state ends the observation the stream justified and asks for the
   * one reconnect this loss is owed.
   */
  #applySessionStreamFailed(error: ErrorDto): readonly Effect[] {
    this.stream = "failed";
    this.#endStreamObservation();
    return [...this.#applyFailure(error), ...this.#reconnectOnce()];
  }

  /** Records a stream the daemon closed: the same loss, the same one reconnect. */
  #applySessionStreamEnded(): readonly Effect[] {
    this.stream = "ended";
    this.#endStreamObservation();
    return this.#reconnectOnce();
  }

  /** Ends the observation the lost stream justified. */
  #endStreamObservation(): void {
    this.sessionState?.endStream();
  }

  /**
   * Returns the one reconnect this front end asks for after a lost stream.
   *
   * The loss stays visible: a typed failure keeps its error line, and the stream
   * status keeps naming the loss.
   */
  #reconnectOnce(): readonly Effect[] {
    if (this.reconnectsAsked > 0) {
      return [];
    }
    this.reconnectsAsked += 1;
    return [{ kind: "connect" }];
  }

  /**
   * Applies one run-stream frame the client already derived.
   *
   * The client applies every frame to the one session state it exposes, so the
   * state carried beside the frame is the derivation's own result: this front
   * end holds it instead of replaying the frame into a copy of its own.
   */
  #applyFrame(state: SessionStreamState): readonly Effect[] {
    if (this.stream !== "live") {
      // A frame for a subscription this front end no longer holds is stale;
      // dropping it keeps the committed rows authoritative.
      return [];
    }
    this.sessionState = state;
    // A frame the client accepted is the evidence that this stream carries the
    // session's traffic: the one reconnect a later loss is owed is restored
    // here, and never by a subscription that only opened.
    this.reconnectsAsked = 0;
    return [];
  }

  /**
   * Records the turn the daemon accepted and keeps the session stream live.
   *
   * The run a started turn opens reaches this state through the session's own
   * stream, so nothing new is subscribed here: a stream that is already live
   * carries the run's status, rows, and text, and only a lost one is
   * re-established.
   */
  #applyTurnAccepted(outcome: SendUserTurnOutcomeDto): readonly Effect[] {
    this.error = null;
    if (this.sessionId === null) {
      return [];
    }
    switch (outcome.kind) {
      case "started":
      case "pending":
        return this.#ensureSessionStream();
    }
  }

  /**
   * Recalls one earlier prompt into the input line.
   *
   * A walk that comes back to the live draft leaves what the user had typed
   * alone: the draft is the fallback, so an accidental arrow key never spends a
   * half-written prompt.
   */
  #applyHistoryRecalled(delta: number): readonly Effect[] {
    const recalled = this.#history.navigate(delta);
    if (recalled !== null) {
      this.draft = replaceDraft(recalled);
    }
    return [];
  }

  /**
   * Submits the line the input line holds.
   *
   * The draft is the state's own, so what a submitted line means is decided
   * here: a prompt becomes a turn, a slash line runs the transition its
   * registered command names, and a line that resolves to nothing becomes the
   * notice that says why.
   */
  #applyInputSubmitted(): readonly Effect[] {
    const text = this.draft.text;
    if (text.trim().length === 0) {
      return [];
    }
    this.#history.push(text);
    this.draft = EMPTY_DRAFT;
    this.notice = null;
    const line = text.trim();
    if (!isCommandLine(line)) {
      return this.#submitTurn(text);
    }
    const resolution = resolveCommand(line.slice(1));
    if (resolution.kind !== "invocation") {
      this.notice = notice(resolution);
      return [];
    }
    const { spec, words } = resolution.invocation;
    switch (spec.action) {
      case "quit":
        return [{ kind: "quit" }];
      case "new_session":
        return [{ kind: "create_session", firstTurn: null }];
      case "open_session": {
        const sessionId = words[0];
        return sessionId === undefined ? [] : this.#applySessionOpenRequested(sessionId);
      }
      case "list_sessions":
        return this.#applySessionsRequested();
      case "choose_theme": {
        const word = words[0];
        if (word === undefined) {
          return this.#applyThemeRequested();
        }
        const preference = themeValue(word);
        if (preference === undefined) {
          this.notice = `unknown theme "${word}"; known themes: ${THEME_PREFERENCES.join(" ")}`;
          return [];
        }
        return this.#applyThemeSelected(preference);
      }
    }
  }

  /**
   * Sends one prompt the caller supplied as data.
   *
   * With no session open the prompt creates the session it needs as that
   * session's first turn, in one command.
   */
  #submitTurn(content: string): readonly Effect[] {
    this.error = null;
    if (this.sessionId === null) {
      return [{ kind: "create_session", firstTurn: content }];
    }
    return [{ kind: "send_turn", sessionId: this.sessionId, content }];
  }

  /** Requests that the live run stop for good. */
  #requestCancel(): readonly Effect[] {
    const run = this.liveRun;
    if (run === null || this.sessionId === null) {
      return [];
    }
    return [{ kind: "cancel_run", sessionId: this.sessionId, runId: run.run_id }];
  }

  /**
   * Opens the session a user named.
   *
   * The keyboard goes back to the transcript from this moment, before the read
   * the open asks for has even been sent.
   */
  #applySessionOpenRequested(sessionId: SessionId): readonly Effect[] {
    this.surface = "chat";
    this.selection = 0;
    return [{ kind: "open_session", sessionId }];
  }

  /**
   * Opens the session browser and asks the client for the current list.
   *
   * The surface owns the keyboard from this moment, before the answer arrives:
   * a screen a user can type into is always a screen that is painted.
   */
  #applySessionsRequested(): readonly Effect[] {
    this.surface = "sessions";
    this.sessions = null;
    this.sessionsError = null;
    this.selection = 0;
    return [{ kind: "list_sessions" }];
  }

  /** Moves the highlight within the entries the surface offers. */
  #applySelectionMoved(delta: number): readonly Effect[] {
    const count = this.optionCount;
    if (this.surface === "chat" || count === 0) {
      return [];
    }
    // Wrapping keeps one key moving through every entry, so a list longer than
    // the panel is reachable without a second gesture.
    this.selection = (((this.selection + delta) % count) + count) % count;
    return [];
  }

  /** Commits the highlight: open the session it names, or apply the theme it names. */
  #applySelectionCommitted(): readonly Effect[] {
    switch (this.surface) {
      case "chat":
        return [];
      case "sessions": {
        const sessionId = this.selectedSession;
        if (sessionId === null) {
          return [];
        }
        this.surface = "chat";
        return [{ kind: "open_session", sessionId }];
      }
      case "theme": {
        const preference = this.selectedTheme;
        if (preference === null) {
          return [];
        }
        return this.#applyThemeSelected(preference);
      }
    }
  }

  /** Dismisses the surface that owns the keyboard without committing anything. */
  #applySurfaceClosed(): readonly Effect[] {
    this.surface = "chat";
    this.selection = 0;
    return [];
  }

  /** Opens the theme picker on the preference in force. */
  #applyThemeRequested(): readonly Effect[] {
    this.surface = "theme";
    this.selection = Math.max(0, THEME_PREFERENCES.indexOf(this.theme));
    return [];
  }

  /** Applies one theme preference and writes it back to the configuration file. */
  #applyThemeSelected(preference: ThemePreference): readonly Effect[] {
    this.theme = preference;
    this.surface = "chat";
    this.selection = 0;
    return [{ kind: "persist_theme", preference }];
  }

  /**
   * Returns the subscription that makes the open session live, if any.
   *
   * A front end follows the session it has open, not one run: the run a turn
   * starts - its own or another window's - reaches the state as a status frame,
   * so a stream that is already live needs nothing.
   */
  #ensureSessionStream(): readonly Effect[] {
    if (this.sessionId === null || this.stream === "live") {
      return [];
    }
    return [{ kind: "subscribe", sessionId: this.sessionId }];
  }

  /**
   * Records one typed failure as the error line.
   *
   * The line is the error type's own one-line form, so a failure reads the same
   * in every front end.
   */
  #applyFailure(error: ErrorDto): readonly Effect[] {
    this.error = `${error.code}: ${error.message}`;
    return [];
  }
}

/** Reports whether one run status is terminal. */
function isTerminal(status: RunStatusDto): boolean {
  return status === "completed" || status === "failed" || status === "interrupted";
}
