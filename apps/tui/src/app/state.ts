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
  SessionSnapshotDto,
  SessionSummariesDto,
  SessionSummaryDto,
  WorkspaceRootDto,
} from "../bindings";
import { runStatusIsTerminal, SessionStreamState } from "../client/stream";
import { type Action, isClientReport, THEME_PREFERENCES, type ThemePreference } from "./action";
import { type Arm, CLEAR_ARM_NOTICE, EXIT_ARM_NOTICE, stopArmNotice } from "./arm";
import { BROWSER_TABS, type BrowserTab, browserRows } from "./browser";
import { isCommandLine, notice, resolveCommand, themeValue } from "./commands";
import {
  type Draft,
  type DraftEdit,
  EMPTY_DRAFT,
  editDraft,
  insertNewline,
  insertText,
  moveCaretLine,
  replaceDraft,
} from "./editor";
import type { Effect } from "./effect";
import { PromptHistory } from "./history";
import {
  type CommandMenu,
  commandMenu,
  menuCompletesNothing,
  menuCompletion,
  menuKey,
} from "./menu";
import {
  expandPastes,
  normalisePaste,
  type PastedText,
  pasteIsLarge,
  pasteLabel,
  pasteSpans,
} from "./paste";
import { type RunPhase, TOOL_WORKING_AFTER_MILLIS } from "./run";

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

/** Whether the daemon this front end talks to has answered. */
export type ConnectionStatus = "connecting" | "ready" | "offline";

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
  /**
   * The browser's tab.
   *
   * `all` is the deliberate default: the browser opens on the whole list, so a
   * front end with no declared workspace root still shows sessions instead of an
   * empty current folder.
   */
  browserTab: BrowserTab = "all";
  /** The word the browser filters its rows by. */
  browserFilter = "";
  /** The row of the command band the reader is on. */
  menuSelection = 0;
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
  /** Whether the daemon this front end talks to has answered. */
  connection: ConnectionStatus = "connecting";
  /** The run mode the open session's projection carries, or `null`. */
  sessionMode: RunModeDto | null = null;
  /** The effective provider model the session's projection names, or `null`. */
  sessionModel: string | null = null;
  /** The live phase of the run this front end observes, if one is live. */
  phase: RunPhase | null = null;
  /** The elapsed time this front end measured for the current or last turn. */
  elapsedMillis: number | null = null;
  /** When the current turn began, in Unix milliseconds, or `null` before one. */
  turnStartedAt: number | null = null;
  /** The chunks each reasoning block has revealed, keyed by block identity. */
  expansions: ReadonlyMap<string, number> = new Map();
  /** The elapsed reading at which the newest open tool call was first seen. */
  #toolSince: number | null = null;
  /** Whether an open tool call has been in flight past the working threshold. */
  #toolWorking = false;
  /** The rows {@link AppState.transcript} handed out last, and their revision. */
  #rows: readonly MessageProjectionDto[] = NO_ROWS;
  #rowsSource: SessionStreamState | null = null;
  #rowsRevision = -1;
  /** The prompts this front end has submitted, for the arrow keys to walk. */
  readonly #history = new PromptHistory();
  /** The pastes the input line stands for with one labelled line each. */
  #pastes: readonly PastedText[] = [];
  /** What the last layered key press armed for its next consecutive press. */
  #armed: Arm | null = null;
  /** The word the band's cursor belongs to, which a new word resets. */
  #menuWord: string | null = null;
  /** Whether the band's current word was dismissed, which an edit reopens. */
  #menuDismissed = false;

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
    // The arm is consecutive over the user's own actions: only a press of the
    // key it belongs to keeps it standing, and a client report - a live run's
    // frames, the front end's elapsed readings - arrives between two presses
    // without being an action of the user's at all.
    if (!isClientReport(action) && !this.#armedIsPressedBy(action)) {
      this.#armed = null;
    }
    switch (action.kind) {
      case "connect_requested":
        this.connection = "connecting";
        return [{ kind: "connect" }];
      case "bootstrapped":
        return this.#applyBootstrapped();
      case "bootstrap_failed":
        return this.#applyBootstrapFailed(action.error);
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
      case "elapsed_reported":
        this.#applyElapsedReported(action.millis);
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
        this.draft = insertText(this.draft, action.text);
        this.#refreshMenu();
        return [];
      case "draft_pasted":
        this.#applyDraftPasted(action.text);
        return [];
      case "draft_edited":
        this.#applyDraftEdited(action.edit);
        return [];
      case "draft_newline":
        this.draft = insertNewline(this.draft);
        this.#refreshMenu();
        return [];
      case "draft_cleared":
        return this.#clearIntoHistory();
      case "escape_pressed":
        return this.#applyEscape();
      case "ctrl_c_pressed":
        return this.#applyCtrlC();
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
      case "browser_filter_typed":
        this.browserFilter += action.text;
        this.selection = 0;
        return [];
      case "browser_filter_backspace":
        this.browserFilter = [...this.browserFilter].slice(0, -1).join("");
        this.selection = 0;
        return [];
      case "browser_tab_moved":
        this.#applyBrowserTabMoved(action.delta);
        return [];
      case "menu_moved":
        this.menuSelection += action.delta;
        return [];
      case "menu_accepted":
        this.#acceptMenu();
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

  /**
   * Returns the row of the command band the reader is on.
   *
   * The cursor is the state's, and it is clamped to the rows the current word
   * ranks: a keystroke that narrows the list leaves the cursor on a row that
   * exists, and a band with no rows cursors nothing. The walk stops at the
   * first and last row instead of wrapping, which is the same bounded walk the
   * session browser's cursor makes.
   */
  get menuHighlight(): number {
    const rows = this.menu?.rows.length ?? 0;
    if (rows === 0) {
      return 0;
    }
    return Math.min(rows - 1, Math.max(0, this.menuSelection));
  }

  /**
   * Resets the band's cursor whenever the word it ranks is another word.
   *
   * A word that ranks other rows is another list, and the row the reader was on
   * says nothing about it; the same word keeps the row it had, so a caret that
   * moves inside a word does not move the highlight.
   */
  #refreshMenu(): void {
    this.#menuDismissed = false;
    const menu = this.menu;
    const word = menu === null ? null : menuKey(menu);
    if (word !== this.#menuWord) {
      this.#menuWord = word;
      this.menuSelection = 0;
    }
  }

  /** Returns the rows the browser's tab and filter show. */
  get browserRows(): readonly SessionSummaryDto[] {
    return browserRows(
      this.sessions?.sessions ?? [],
      this.browserTab,
      this.browserFilter,
      this.workspaceRoot ?? "",
    );
  }

  /** Returns the workspace root the folder tab filters by, when one is known. */
  workspaceRoot: WorkspaceRootDto | null = null;

  /** Returns the session the browser has highlighted. */
  get selectedSession(): SessionId | null {
    return this.browserRows[this.selection]?.session_id ?? null;
  }

  /** Switches the browser's tab and keeps the cursor on a row that exists. */
  #applyBrowserTabMoved(delta: number): void {
    const index = BROWSER_TABS.indexOf(this.browserTab);
    const count = BROWSER_TABS.length;
    this.browserTab = BROWSER_TABS[(((index + delta) % count) + count) % count] ?? "all";
    this.selection = 0;
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

  /**
   * Returns the rows the band shows for the word the caret sits in, if any.
   *
   * The band is derived from the line, except for one state the line cannot
   * carry: a word the user dismissed with Esc stays closed until the next edit,
   * which is what makes the dismissing press mean nothing else.
   */
  get menu(): CommandMenu | null {
    return this.#menuDismissed ? null : commandMenu(this.draft);
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
    this.connection = "ready";
    const sessionId = this.sessionId ?? this.initialSession;
    return sessionId === null ? [] : [{ kind: "open_session", sessionId }];
  }

  /** Shows one session snapshot and opens the one subscription it needs. */
  #applySessionSnapshot(snapshot: SessionSnapshotDto): readonly Effect[] {
    const read = SessionStreamState.fromSnapshot(snapshot);
    this.#mergeTranscript(read);
    this.sessionId = snapshot.session_id;
    this.sessionMode = snapshot.projection.mode;
    this.sessionModel = snapshot.projection.provider_model ?? null;
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
    this.#applyPhase(state);
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
        this.#beginTurn();
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
    // A multi-line prompt is navigated the way its own rows are: the arrows
    // move the caret while the draft carries a line in that direction, and only
    // at the first and last line do they walk the history.
    const moved = moveCaretLine(this.draft, delta);
    if (moved !== null) {
      this.draft = moved;
      this.#refreshMenu();
      return [];
    }
    const recalled = this.#history.navigate(delta);
    if (recalled !== null) {
      this.draft = replaceDraft(recalled);
      this.#pastes = [];
      this.#refreshMenu();
    }
    return [];
  }

  /**
   * Applies one caret edit, with a paste label read as one unit.
   *
   * Removing one character of a label would leave a label standing for nothing,
   * so the whole label goes at once, exactly as it was inserted, and the caret
   * steps over one instead of into it.
   */
  #applyDraftEdited(edit: DraftEdit): void {
    const spans = pasteSpans(this.draft.text, this.#pastes);
    const caret = this.draft.caret;
    const removal =
      edit === "backspace"
        ? spans.find((span) => span.start < caret && caret <= span.end)
        : edit === "delete"
          ? spans.find((span) => span.start === caret)
          : undefined;
    if (removal !== undefined) {
      this.draft = {
        text: this.draft.text.slice(0, removal.start) + this.draft.text.slice(removal.end),
        caret: removal.start,
      };
      this.#pastes = this.#pastes.filter((_, index) => index !== removal.index);
      this.#refreshMenu();
      return;
    }
    const step = spans.find((span) =>
      edit === "left" ? span.end === caret : edit === "right" ? span.start === caret : false,
    );
    if (step !== undefined) {
      this.draft = { text: this.draft.text, caret: edit === "left" ? step.start : step.end };
      return;
    }
    this.draft = editDraft(this.draft, edit);
    this.#refreshMenu();
  }

  /**
   * Applies one paste, as one edit.
   *
   * A paste the line can hold is inserted as the ordinary text it is; a paste
   * larger than the line can show - more characters than the inline limit, or
   * more lines than one - leaves one labelled line behind and keeps its text
   * outside the buffer, because a line the user cannot read at a glance is not a
   * line they can edit.
   */
  #applyDraftPasted(raw: string): void {
    const text = normalisePaste(raw);
    if (text === "") {
      return;
    }
    if (pasteIsLarge(text)) {
      const label = pasteLabel(text);
      this.draft = insertText(this.draft, label);
      this.#pastes = [...this.#pastes, { label, text }];
    } else {
      this.draft = insertText(this.draft, text);
    }
    this.#refreshMenu();
  }

  /**
   * Commits the band's highlighted row into the input line.
   *
   * The word the band was opened for is replaced by the row's completion, and
   * the band then follows the same open rule: a command that declares an
   * argument reopens it on the argument word, and a complete command or value
   * closes it.
   */
  #acceptMenu(): void {
    const menu = this.menu;
    if (menu === null) {
      return;
    }
    const completion = menuCompletion(menu, this.menuHighlight);
    if (completion === null) {
      return;
    }
    const text =
      this.draft.text.slice(0, completion.start) +
      completion.text +
      this.draft.text.slice(completion.end);
    this.draft = { text, caret: completion.start + completion.text.length };
    this.#refreshMenu();
  }

  /**
   * Abandons the input line into the recallable history and clears it.
   *
   * The line is kept whole: a paste label is expanded first, so the history
   * carries the text the user pasted and a recall puts it back as the line it
   * was.
   */
  #clearIntoHistory(): readonly Effect[] {
    const text = expandPastes(this.draft.text, this.#pastes);
    if (text.trim() !== "") {
      this.#history.push(text);
    }
    this.draft = EMPTY_DRAFT;
    this.#pastes = [];
    this.notice = null;
    this.#refreshMenu();
    return [];
  }

  /**
   * Applies one Esc press on the chat surface.
   *
   * Four meanings meet on the key, and the state it finds decides which one the
   * press carries: the band is the innermost layer, so one press closes it and
   * touches nothing else; a live run stops on a single press, the way a shell
   * cancels a running command; a typed line arms the clear, which its next
   * consecutive press fires; and an empty line leaves the front end.
   */
  #applyEscape(): readonly Effect[] {
    if (this.menu !== null) {
      // Closing the band is not editing the line: the word stays exactly where
      // it is and the caret does not move, and the next edit opens the band
      // again, because an edit is what recomputes the word.
      this.#menuDismissed = true;
      this.#menuWord = null;
      this.menuSelection = 0;
      return [];
    }
    if (this.liveRun !== null) {
      return this.#requestCancel();
    }
    if (this.draft.text.trim() !== "") {
      if (this.#armed === "clear") {
        this.#armed = null;
        return this.#clearIntoHistory();
      }
      this.#arm("clear", CLEAR_ARM_NOTICE);
      return [];
    }
    this.#armed = null;
    return [{ kind: "quit" }];
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
      if (this.#armed === "stop") {
        this.#armed = null;
        return this.#requestCancel();
      }
      this.#arm("stop", stopArmNotice(run.run_id));
      return [];
    }
    if (this.draft.text.trim() !== "") {
      this.#armed = null;
      return this.#clearIntoHistory();
    }
    if (this.#armed === "exit") {
      this.#armed = null;
      return [{ kind: "quit" }];
    }
    this.#arm("exit", EXIT_ARM_NOTICE);
    return [];
  }

  /** Arms one act for its next consecutive press and shows its notice. */
  #arm(arm: Arm, notice: string): void {
    this.#armed = arm;
    this.notice = notice;
  }

  /** Returns whether the arm standing belongs to the key one action pressed. */
  #armedIsPressedBy(action: Action): boolean {
    if (this.#armed === null) {
      return false;
    }
    return this.#armed === "clear"
      ? action.kind === "escape_pressed"
      : action.kind === "ctrl_c_pressed";
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
      this.#acceptMenu();
      return [];
    }
    const text = expandPastes(this.draft.text, this.#pastes);
    if (text.trim().length === 0) {
      this.notice = "type a message before sending it";
      return [];
    }
    this.#history.push(text);
    this.draft = EMPTY_DRAFT;
    this.#pastes = [];
    this.notice = null;
    this.#refreshMenu();
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
    this.#beginTurn();
    if (this.sessionId === null) {
      return [{ kind: "create_session", firstTurn: content }];
    }
    return [{ kind: "send_turn", sessionId: this.sessionId, content }];
  }

  /**
   * Opens a turn: the row reads `Waiting…` until a stream names more.
   *
   * The elapsed value the previous turn measured belongs to that turn, so a new
   * turn starts from zero and the tool watch forgets the call it was watching.
   */
  #beginTurn(): void {
    this.phase = "waiting";
    this.elapsedMillis = 0;
    this.turnStartedAt = Date.now();
    this.#toolSince = null;
    this.#toolWorking = false;
  }

  /** Reads one accepted frame's phase out of what it changed. */
  #applyPhase(state: SessionStreamState): void {
    const status = state.status;
    if (status !== null && runStatusIsTerminal(status)) {
      this.phase = null;
      this.#toolSince = null;
      this.#toolWorking = false;
      return;
    }
    if (state.provisionalText.length > 0) {
      this.phase = "answering";
      return;
    }
    if (state.provisionalReasoning.length > 0) {
      this.phase = "thinking";
      return;
    }
    this.phase = this.phase ?? "waiting";
  }

  /**
   * Records one elapsed reading and decides whether a tool call is work.
   *
   * A call open for longer than the threshold reads as `working`; a shorter one
   * leaves the phase it found, so a fast tool never flickers the row.
   */
  #applyElapsedReported(millis: number): void {
    this.elapsedMillis = millis;
    const open = this.activeRun?.open_tool_calls ?? 0;
    if (open === 0) {
      this.#toolSince = null;
      this.#toolWorking = false;
      return;
    }
    if (this.#toolSince === null) {
      this.#toolSince = millis;
      return;
    }
    this.#toolWorking = millis - this.#toolSince > TOOL_WORKING_AFTER_MILLIS;
  }

  /**
   * Reveals one more chunk of one reasoning block.
   *
   * A block that names no identity asks for the newest block there is: the live
   * segment while a step is reasoning, the last committed one after it, so the
   * gesture reveals what the reader is looking at.
   */
  #applyReasoningExpanded(id: string | null): void {
    const key = id ?? this.#newestReasoningBlock();
    if (key === null) {
      return;
    }
    const next = new Map(this.expansions);
    next.set(key, (this.expansions.get(key) ?? 0) + 1);
    this.expansions = next;
  }

  /** Returns the identity of the newest block that has reasoning to reveal. */
  #newestReasoningBlock(): string | null {
    if (this.provisionalReasoning.length > 0) {
      return "live";
    }
    for (let index = this.transcript.length - 1; index >= 0; index -= 1) {
      const message = this.transcript[index];
      if (
        message !== undefined &&
        message.reasoning !== null &&
        message.reasoning.trim().length > 0
      ) {
        return String(message.id);
      }
    }
    return null;
  }

  /** Returns the chunks one reasoning block has revealed. */
  expansionOf(id: string): number {
    return this.expansions.get(id) ?? 0;
  }

  /** Records a daemon this front end could not reach. */
  #applyBootstrapFailed(error: ErrorDto): readonly Effect[] {
    this.connection = "offline";
    return this.#applyFailure(error);
  }

  /** Returns the live phase the status row names, if one is live. */
  get runPhase(): RunPhase | null {
    return this.#toolWorking ? "working" : this.phase;
  }

  /** Returns the elapsed value this front end measured, if it measured one. */
  get elapsed(): number | null {
    return this.elapsedMillis;
  }

  /** Returns when the turn in flight began, in Unix milliseconds. */
  get turnStart(): number | null {
    return this.turnStartedAt;
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
