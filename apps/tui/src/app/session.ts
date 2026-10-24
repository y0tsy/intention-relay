//! The open session and the one live stream that observes it.
//!
//! The session owns the client's committed state, the status of the stream,
//! and the transcript the front end reads. Its transcript cache copies the
//! client's rows once per revision, so a view comparing the list it was handed
//! sees a new one only when the rows moved.

import type {
  ErrorDto,
  MessageProjectionDto,
  RunModeDto,
  RunProjectionDto,
  RunStatusDto,
  SendUserTurnOutcomeDto,
  SessionId,
  SessionSnapshotDto,
  WorkspaceRootDto,
} from "../bindings";
import { SessionStreamState } from "../client/stream";
import type { Effect } from "./effect";

/** The rows of a session this front end has not opened. */
const NO_ROWS: readonly MessageProjectionDto[] = [];

/** How far the one live channel of a session has got. */
export type StreamStatus = "inactive" | "live" | "failed" | "ended";

/** Whether the daemon this front end talks to has answered. */
export type ConnectionStatus = "connecting" | "ready" | "offline";

/** Owns the open session, its client state, and its live stream. */
export class Session {
  sessionId: SessionId | null = null;
  /** The client's state of the open session, if any. */
  sessionState: SessionStreamState | null = null;
  /** How many reconnects this front end has asked for the current stream. */
  reconnectsAsked = 0;
  stream: StreamStatus = "inactive";
  /** The failure the chat surface reports. */
  error: string | null = null;
  /** Whether the daemon this front end talks to has answered. */
  connection: ConnectionStatus = "connecting";
  /** The run mode the open session's projection carries, or `null`. */
  sessionMode: RunModeDto | null = null;
  /** The effective provider model the session's projection names, or `null`. */
  sessionModel: string | null = null;
  /** The workspace root the folder tab filters by, when one is known. */
  workspaceRoot: WorkspaceRootDto | null = null;
  /** The rows the transcript getter handed out last, and their revision. */
  #rows: readonly MessageProjectionDto[] = NO_ROWS;
  #rowsSource: SessionStreamState | null = null;
  #rowsRevision = -1;

  constructor(
    /** The session the caller selected, or `null` before one is opened. */
    readonly initialSession: SessionId | null,
  ) {}

  /**
   * Returns the committed transcript rows of the current session.
   *
   * The client mutates its rows in place, so the rows a view compares are
   * copied once per revision: the same read hands back the same list, and an
   * appended row hands back a new one.
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
  bootstrapped(): readonly Effect[] {
    this.error = null;
    this.connection = "ready";
    const sessionId = this.sessionId ?? this.initialSession;
    return sessionId === null ? [] : [{ kind: "open_session", sessionId }];
  }

  /** Shows one session snapshot and opens the one subscription it needs. */
  snapshot(snapshot: SessionSnapshotDto): readonly Effect[] {
    const read = SessionStreamState.fromSnapshot(snapshot);
    this.#mergeTranscript(read);
    this.sessionId = snapshot.session_id;
    this.sessionMode = snapshot.projection.mode;
    this.sessionModel = snapshot.projection.provider_model ?? null;
    this.stream = "inactive";
    this.error = null;
    // Opening a session records no notice: the notice line stays free for real
    // transient events.
    return this.ensureStream();
  }

  /** Opens the session the daemon just created. */
  created(sessionId: SessionId): readonly Effect[] {
    this.error = null;
    return [{ kind: "open_session", sessionId }];
  }

  /**
   * Takes the client's committed state of a freshly opened subscription.
   *
   * An accepted subscription proves only that a stream exists: the reconnect
   * budget a loss spent stays spent until a frame the state accepts shows the
   * stream is carrying the session's traffic.
   */
  streamOpened(state: SessionStreamState): readonly Effect[] {
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
  streamFailed(error: ErrorDto): readonly Effect[] {
    this.stream = "failed";
    this.#endStreamObservation();
    return [...this.failure(error), ...this.#reconnectOnce()];
  }

  /** Records a stream the daemon closed: the same loss, the same one reconnect. */
  streamEnded(): readonly Effect[] {
    this.stream = "ended";
    this.#endStreamObservation();
    return this.#reconnectOnce();
  }

  /**
   * Accepts one run-stream frame the client already derived.
   *
   * The frame is stale for a subscription this front end no longer holds;
   * dropping it keeps the committed rows authoritative. The reconnect a later
   * loss is owed is restored only by an accepted frame, never by a subscription
   * that only opened.
   */
  acceptFrame(state: SessionStreamState): boolean {
    if (this.stream !== "live") {
      return false;
    }
    this.sessionState = state;
    this.reconnectsAsked = 0;
    return true;
  }

  /** Records the turn the daemon accepted when a session is open. */
  turnStarted(outcome: SendUserTurnOutcomeDto): boolean {
    this.error = null;
    if (this.sessionId === null) {
      return false;
    }
    switch (outcome.kind) {
      case "started":
      case "pending":
        return true;
    }
  }

  /** Records a daemon this front end could not reach. */
  bootstrapFailed(error: ErrorDto): readonly Effect[] {
    this.connection = "offline";
    return this.failure(error);
  }

  /** Requests that the live run stop for good. */
  requestCancel(): readonly Effect[] {
    const run = this.liveRun;
    if (run === null || this.sessionId === null) {
      return [];
    }
    return [{ kind: "cancel_run", sessionId: this.sessionId, runId: run.run_id }];
  }

  /**
   * Returns the subscription that makes the open session live, if any.
   *
   * A front end follows the session it has open, not one run: the run a turn
   * starts - its own or another window's - reaches the state as a status frame,
   * so a stream that is already live needs nothing.
   */
  ensureStream(): readonly Effect[] {
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
  failure(error: ErrorDto): readonly Effect[] {
    this.error = `${error.code}: ${error.message}`;
    return [];
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

  /** Ends the observation the lost stream justified. */
  #endStreamObservation(): void {
    this.sessionState?.endStream();
  }

  /**
   * Returns the one reconnect this front end asks for after a lost stream.
   *
   * The loss stays visible: a typed failure keeps its error line, and the
   * stream status keeps naming the loss.
   */
  #reconnectOnce(): readonly Effect[] {
    if (this.reconnectsAsked > 0) {
      return [];
    }
    this.reconnectsAsked += 1;
    return [{ kind: "connect" }];
  }
}

/** Reports whether one run status is terminal. */
function isTerminal(status: RunStatusDto): boolean {
  return status === "completed" || status === "failed" || status === "interrupted";
}
