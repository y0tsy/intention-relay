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
  RunModeDto,
  RunProjectionDto,
  RunStatusDto,
  SendUserTurnOutcomeDto,
  SessionId,
  SessionSummariesDto,
  SessionSummaryDto,
  WorkspaceRootDto,
} from "../bindings";
import { type Action, isClientReport, THEME_PREFERENCES, type ThemePreference } from "./action";
import { type Arm, EXIT_ARM_NOTICE, stopArmNotice } from "./arm";
import type { SessionStreamState } from "../client/stream";
import type { BrowserTab } from "./browser";
import type { Draft } from "./editor";
import type { Effect } from "./effect";
import { type CommandMenu, menuCompletesNothing } from "./menu";
import { Composer } from "./composer";
import { resolveSubmission } from "./submit";
import type { RunPhase } from "./run";
import { ReasoningExpansions } from "./reasoning";
import { Session, type ConnectionStatus, type StreamStatus } from "./session";
import { RunWatch } from "./run-watch";
import { BrowserState } from "./browser-state";

/** Which surface owns the keyboard. */
export type Surface = "chat" | "sessions" | "theme";

export type { ConnectionStatus, StreamStatus } from "./session";

/** Every value one front end holds, with no terminal attached. */
export class AppState {
  /** The open session, its stream, and the transcript cache. */
  readonly #session: Session;
  /**
   * The surface that owns the keyboard.
   *
   * Exactly one surface owns it at a time, and that ownership is what the accent
   * border marks, so a screen a user is typing into is always the one that is
   * painted.
   */
  surface: Surface = "chat";
  /** The browser's own list, tab, and filter. */
  readonly #browser = new BrowserState();
  /** The index the surface that owns the keyboard has highlighted. */
  selection = 0;
  /** The draft line, its history, and its command band. */
  readonly #composer = new Composer();
  /** The palette preference the configuration file declares. */
  theme: ThemePreference;
  /** The one-line notice the chat surface shows instead of an effect. */
  notice: string | null = null;
  /** The phase, start, and elapsed reading of the turn in flight. */
  readonly #watch = new RunWatch();
  /** The chunks each reasoning block has revealed. */
  readonly #reasoning = new ReasoningExpansions();

  /**
   * Creates the state a front end starts from.
   *
   * `initialSession` is the session the caller selected, or the session the
   * client resolved for an explicit continuation; `null` opens nothing.
   * `initialTheme` is the preference the configuration file declared.
   */
  constructor(initialSession: SessionId | null, initialTheme: ThemePreference = "auto") {
    this.#session = new Session(initialSession);
    this.theme = initialTheme;
  }

  /** Applies one action and returns the client effects it asks for. */
  update(action: Action): readonly Effect[] {
    // The arm is consecutive over the user's own actions: only a press of the
    // key it belongs to keeps it standing, and a client report - a live run's
    // frames, the front end's elapsed readings - arrives between two presses
    // without being an action of the user's at all.
    if (!isClientReport(action) && !this.#armedIsPressedBy(action)) {
      this.#composer.armed = null;
    }
    switch (action.kind) {
      case "connect_requested":
        this.#session.connection = "connecting";
        return [{ kind: "connect" }];
      case "bootstrapped":
        return this.#session.bootstrapped();
      case "bootstrap_failed":
        return this.#session.bootstrapFailed(action.error);
      case "session_snapshot_loaded":
        return this.#session.snapshot(action.snapshot);
      case "session_snapshot_failed":
        return this.#applyFailure(action.error);
      case "session_created":
        return this.#session.created(action.sessionId);
      case "session_create_failed":
        return this.#applyFailure(action.error);
      case "session_stream_opened":
        return this.#session.streamOpened(action.state);
      case "session_stream_failed":
        return this.#session.streamFailed(action.error);
      case "session_stream_ended":
        return this.#session.streamEnded();
      case "frame_received":
        return this.#applyFrame(action.state);
      case "turn_accepted":
        return this.#applyTurnAccepted(action.outcome);
      case "turn_failed":
        return this.#applyFailure(action.error);
      case "elapsed_reported":
        this.#watch.reportElapsed(action.millis, this.activeRun?.open_tool_calls ?? 0);
        return [];
      case "reasoning_expanded":
        this.#applyReasoningExpanded(action.id);
        return [];
      case "run_cancel_accepted":
        return [];
      case "run_cancel_failed":
      case "effect_failed":
        return this.#applyFailure(action.error);
      case "draft_typed":
        this.#composer.type(action.text);
        return [];
      case "draft_pasted":
        this.#composer.paste(action.text);
        return [];
      case "draft_edited":
        this.#composer.edit(action.edit);
        return [];
      case "draft_newline":
        this.#composer.newline();
        return [];
      case "escape_pressed":
        return this.#applyEscape();
      case "ctrl_c_pressed":
        return this.#applyCtrlC();
      case "history_recalled":
        this.#composer.recall(action.delta);
        return [];
      case "notice_shown":
        this.notice = action.text;
        return [];
      case "input_submitted":
        return this.#applyInputSubmitted();
      case "new_session_requested":
        return [{ kind: "create_session", firstTurn: null }];
      case "session_open_requested":
        return this.#applySessionOpenRequested(action.sessionId);
      case "cancel_run_requested":
        return this.#requestCancel();
      case "sessions_requested":
        return this.#applySessionsRequested();
      case "sessions_listed":
        this.#browser.listed(action.summaries);
        this.selection = 0;
        return [];
      case "sessions_list_failed":
        this.#browser.failed(`${action.error.code}: ${action.error.message}`);
        this.selection = 0;
        return [];
      case "selection_moved":
        return this.#applySelectionMoved(action.delta);
      case "selection_committed":
        return this.#applySelectionCommitted();
      case "browser_filter_typed":
        this.#browser.typeFilter(action.text);
        this.selection = 0;
        return [];
      case "browser_filter_backspace":
        this.#browser.backspaceFilter();
        this.selection = 0;
        return [];
      case "browser_tab_moved":
        this.#browser.moveTab(action.delta);
        this.selection = 0;
        return [];
      case "menu_moved":
        this.#composer.moveMenu(action.delta);
        return [];
      case "menu_accepted":
        this.#composer.acceptMenu();
        return [];
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
      return this.browserRows.length;
    }
    if (this.surface === "theme") {
      return THEME_PREFERENCES.length;
    }
    return 0;
  }

  /** Returns the row of the command band the reader is on. */
  get menuHighlight(): number {
    return this.#composer.menuHighlight;
  }

  /** Returns the session list the browser shows, once the daemon answered. */
  get sessions(): SessionSummariesDto | null {
    return this.#browser.summaries;
  }

  /** Returns the failure the session browser reports instead of a list. */
  get sessionsError(): string | null {
    return this.#browser.failure;
  }

  /** Returns the browser's tab. */
  get browserTab(): BrowserTab {
    return this.#browser.tab;
  }

  /** Returns the word the browser filters its rows by. */
  get browserFilter(): string {
    return this.#browser.filter;
  }

  /** Returns the rows the browser's tab and filter show. */
  get browserRows(): readonly SessionSummaryDto[] {
    return this.#browser.rows(this.workspaceRoot ?? "");
  }

  /** Returns the session the browser has highlighted. */
  get selectedSession(): SessionId | null {
    return this.#browser.rows(this.workspaceRoot ?? "")[this.selection]?.session_id ?? null;
  }

  /** Returns the theme preference the picker has highlighted. */
  get selectedTheme(): ThemePreference | null {
    return THEME_PREFERENCES[this.selection] ?? null;
  }

  /** Returns the session the caller selected, or `null` before one opens. */
  get initialSession(): SessionId | null {
    return this.#session.initialSession;
  }

  /** Returns the open session's identity, or `null` before one is open. */
  get sessionId(): SessionId | null {
    return this.#session.sessionId;
  }

  /** Sets the open session's identity. */
  set sessionId(value: SessionId | null) {
    this.#session.sessionId = value;
  }

  /** Returns the client state of the open session, if any. */
  get sessionState(): SessionStreamState | null {
    return this.#session.sessionState;
  }

  /** Sets the client state of the open session. */
  set sessionState(value: SessionStreamState | null) {
    this.#session.sessionState = value;
  }

  /** Returns how many reconnects this front end has asked for. */
  get reconnectsAsked(): number {
    return this.#session.reconnectsAsked;
  }

  /** Returns how far the one live channel has got. */
  get stream(): StreamStatus {
    return this.#session.stream;
  }

  /** Sets how far the one live channel has got. */
  set stream(value: StreamStatus) {
    this.#session.stream = value;
  }

  /** Returns the failure the chat surface reports. */
  get error(): string | null {
    return this.#session.error;
  }

  /** Sets the failure the chat surface reports. */
  set error(value: string | null) {
    this.#session.error = value;
  }

  /** Returns whether the daemon this front end talks to has answered. */
  get connection(): ConnectionStatus {
    return this.#session.connection;
  }

  /** Returns the run mode the open session projects, if any. */
  get sessionMode(): RunModeDto | null {
    return this.#session.sessionMode;
  }

  /** Returns the effective provider model, if any. */
  get sessionModel(): string | null {
    return this.#session.sessionModel;
  }

  /** Returns the workspace root the folder tab filters by, if known. */
  get workspaceRoot(): WorkspaceRootDto | null {
    return this.#session.workspaceRoot;
  }

  /** Sets the workspace root the folder tab filters by. */
  set workspaceRoot(value: WorkspaceRootDto | null) {
    this.#session.workspaceRoot = value;
  }

  /** Returns the committed transcript rows of the current session. */
  get transcript(): readonly MessageProjectionDto[] {
    return this.#session.transcript;
  }

  /** Returns the last known run projection. */
  get activeRun(): RunProjectionDto | null {
    return this.#session.activeRun;
  }

  /** Returns the last known run lifecycle status. */
  get runStatus(): RunStatusDto | null {
    return this.#session.runStatus;
  }

  /** Returns the transient answer text of the step in flight. */
  get provisionalText(): string {
    return this.#session.provisionalText;
  }

  /** Returns the transient reasoning text of the step in flight. */
  get provisionalReasoning(): string {
    return this.#session.provisionalReasoning;
  }

  /** Returns the run this front end observes as live, if any. */
  get liveRun(): RunProjectionDto | null {
    return this.#session.liveRun;
  }

  /** Returns the chunks each reasoning block has revealed. */
  get expansions(): ReadonlyMap<string, number> {
    return this.#reasoning.expansions;
  }

  /** Returns the line the composer shows. */
  get draft(): Draft {
    return this.#composer.draft;
  }

  /** Sets the line the composer shows. */
  set draft(value: Draft) {
    this.#composer.draft = value;
  }

  /** Returns the row of the command band the reader is on. */
  get menuSelection(): number {
    return this.#composer.menuSelection;
  }

  /** Sets the row of the command band the reader is on. */
  set menuSelection(value: number) {
    this.#composer.menuSelection = value;
  }

  /** Returns the rows the band shows for the word the caret sits in, if any. */
  get menu(): CommandMenu | null {
    return this.#composer.menu;
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
    if (!this.#session.turnStarted(outcome)) {
      return [];
    }
    this.#watch.beginTurn();
    return this.#session.ensureStream();
  }

  /**
   * Applies one Esc press on the chat surface.
   *
   * The key has one job: stopping the run this front end observes, on a single
   * press, the way a shell cancels a running command. A state with no live run
   * has nothing for it to do - the key never clears the line, closes the band,
   * or leaves the front end.
   */
  #applyEscape(): readonly Effect[] {
    return this.liveRun === null ? [] : this.#requestCancel();
  }

  /**
   * Applies one Ctrl+C press on any surface.
   *
   * The press is layered over the state it finds: a live run arms the stop its
   * next consecutive press fires; a typed line abandons into the recallable
   * history without arming anything; and an empty line arms the exit.
   */
  #applyCtrlC(): readonly Effect[] {
    const run = this.liveRun;
    if (run !== null) {
      if (this.#composer.armed === "stop") {
        this.#composer.armed = null;
        return this.#requestCancel();
      }
      this.#arm("stop", stopArmNotice(run.run_id));
      return [];
    }
    if (this.#composer.hasText) {
      this.#composer.armed = null;
      this.#composer.clearIntoHistory();
      this.notice = null;
      return [];
    }
    if (this.#composer.armed === "exit") {
      this.#composer.armed = null;
      return [{ kind: "quit" }];
    }
    this.#arm("exit", EXIT_ARM_NOTICE);
    return [];
  }

  /** Arms one act for its next consecutive press and shows its notice. */
  #arm(arm: Arm, notice: string): void {
    this.#composer.armed = arm;
    this.notice = notice;
  }

  /** Returns whether the arm standing belongs to the key one action pressed. */
  #armedIsPressedBy(action: Action): boolean {
    return this.#composer.armed !== null && action.kind === "ctrl_c_pressed";
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
    const menu = this.menu;
    if (menu !== null && !menuCompletesNothing(menu)) {
      // The band owns Enter while its word is incomplete: the press completes
      // the word the user is typing instead of running it, so an abbreviated
      // line grows to its command or value first. A word that already spells
      // the command or a declared value has nothing left to complete, so Enter
      // runs the line - one press for `/sessions`, not two.
      this.#composer.acceptMenu();
      return [];
    }
    const text = this.#composer.expanded();
    if (text.trim().length === 0) {
      this.notice = "type a message before sending it";
      return [];
    }
    this.#composer.remember(text);
    this.#composer.clear();
    this.notice = null;
    const submission = resolveSubmission(text);
    switch (submission.kind) {
      case "prompt":
        return this.#submitTurn(submission.text);
      case "notice":
        this.notice = submission.text;
        return [];
      case "quit":
        return [{ kind: "quit" }];
      case "new_session":
        return [{ kind: "create_session", firstTurn: null }];
      case "list_sessions":
        return this.#applySessionsRequested();
      case "theme_picker":
        return this.#applyThemeRequested();
      case "theme":
        return this.#applyThemeSelected(submission.preference);
    }
  }

  /**
   * Sends one prompt the caller supplied as data.
   *
   * With no session open the prompt creates the session it needs as that
   * session's first turn, in one command.
   */
  #submitTurn(content: string): readonly Effect[] {
    this.#session.error = null;
    this.#watch.beginTurn();
    if (this.sessionId === null) {
      return [{ kind: "create_session", firstTurn: content }];
    }
    return [{ kind: "send_turn", sessionId: this.sessionId, content }];
  }

  /**
   * Applies one run-stream frame the client already derived.
   *
   * The frame is stale for a subscription this front end no longer holds;
   * dropping it keeps the committed rows authoritative.
   */
  #applyFrame(state: SessionStreamState): readonly Effect[] {
    if (!this.#session.acceptFrame(state)) {
      return [];
    }
    this.#watch.applyPhase(state);
    return [];
  }

  /** Reveals one more chunk of one reasoning block. */
  #applyReasoningExpanded(id: string | null): void {
    this.#reasoning.expand(id, this.transcript, this.provisionalReasoning);
  }

  /** Returns the chunks one reasoning block has revealed. */
  expansionOf(id: string): number {
    return this.#reasoning.of(id);
  }

  /** Returns the phase the current turn reads as, if one is live. */
  get phase(): RunPhase | null {
    return this.#watch.phase;
  }

  /** Returns the elapsed value this front end measured, if it measured one. */
  get elapsedMillis(): number | null {
    return this.#watch.elapsedMillis;
  }

  /** Returns when the turn in flight began, in Unix milliseconds. */
  get turnStartedAt(): number | null {
    return this.#watch.turnStartedAt;
  }

  /** Returns the live phase the status row names, if one is live. */
  get runPhase(): RunPhase | null {
    return this.#watch.runPhase;
  }

  /** Returns the elapsed value this front end measured, if it measured one. */
  get elapsed(): number | null {
    return this.#watch.elapsedMillis;
  }

  /** Returns when the turn in flight began, in Unix milliseconds. */
  get turnStart(): number | null {
    return this.#watch.turnStartedAt;
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
    this.#browser.requested();
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
   * Records one typed failure as the error line.
   *
   * The line is the error type's own one-line form, so a failure reads the same
   * in every front end.
   */
  #applyFailure(error: ErrorDto): readonly Effect[] {
    return this.#session.failure(error);
  }
}
