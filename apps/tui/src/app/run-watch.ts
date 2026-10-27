//! What one turn is doing, and how long it has taken.
//!
//! The watch owns the phase a turn reads as, when it began, and what it has
//! measured so far. A tool call watched long enough reads as work; a fast one
//! leaves the phase it found, so the status row never flickers.

import { runStatusIsTerminal } from "../client/stream";
import type { SessionStreamState } from "../client/stream";
import { TOOL_WORKING_AFTER_MILLIS, type RunPhase } from "./run";

/** Owns the live phase, start, and elapsed reading of one turn. */
export class RunWatch {
  /** The phase one accepted frame last set, or `null` before one. */
  phase: RunPhase | null = null;
  /** The elapsed value the front end last measured, or `null` before one. */
  elapsedMillis: number | null = null;
  /** When the current turn began, in Unix milliseconds, or `null` before one. */
  turnStartedAt: number | null = null;
  /** The elapsed reading at which the newest open tool call was first seen. */
  #toolSince: number | null = null;
  /** Whether an open tool call has been in flight past the working threshold. */
  #toolWorking = false;

  /** Returns the live phase the status row names, if one is live. */
  get runPhase(): RunPhase | null {
    return this.#toolWorking ? "working" : this.phase;
  }

  /**
   * Opens a turn: the row reads `Waiting…` until a stream names more.
   *
   * The elapsed value the previous turn measured belongs to that turn, so a new
   * turn starts from zero and the tool watch forgets the call it was watching.
   */
  beginTurn(): void {
    this.phase = "waiting";
    this.elapsedMillis = 0;
    this.turnStartedAt = Date.now();
    this.#toolSince = null;
    this.#toolWorking = false;
  }

  /** Reads one accepted frame's phase out of what it changed. */
  applyPhase(state: SessionStreamState): void {
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
  reportElapsed(millis: number, openToolCalls: number): void {
    this.elapsedMillis = millis;
    if (openToolCalls === 0) {
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
}
