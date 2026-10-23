//! The render-free state machine every front end drives.
//!
//! The state owns the open session, the client's committed state of it, and the
//! status of the one stream that observes it. A front end reads the state and
//! mutates it only through [`AppState.update`], which returns the client effects
//! the transition asks for and never throws: a failure becomes the error line
//! instead of an error return, so no caller has to recover from a transition.

import type {
  ErrorDto,
  MessageProjectionDto,
  RunProjectionDto,
  RunStatusDto,
  SendUserTurnOutcomeDto,
  SessionId,
  SessionSnapshotDto,
} from "../bindings";
import { SessionStreamState } from "../client/stream";
import type { Action } from "./action";
import type { Effect } from "./effect";

/** How far the one live channel of a session has got. */
export type StreamStatus = "inactive" | "live" | "failed" | "ended";

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
   * Creates the state a front end starts from.
   *
   * `initialSession` is the session the caller selected, or the session the
   * client resolved for an explicit continuation; `null` opens nothing.
   */
  constructor(initialSession: SessionId | null) {
    this.initialSession = initialSession;
  }

  /** Applies one action and returns the client effects it asks for. */
  update(action: Action): readonly Effect[] {
    switch (action.kind) {
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
      case "turn_submitted":
        return this.#submitTurn(action.content);
      case "new_session_requested":
        return [{ kind: "create_session", firstTurn: null }];
      case "cancel_run_requested":
        return this.#requestCancel();
    }
  }

  /** Returns the committed transcript rows of the current session. */
  get transcript(): readonly MessageProjectionDto[] {
    return this.sessionState?.messages ?? [];
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
