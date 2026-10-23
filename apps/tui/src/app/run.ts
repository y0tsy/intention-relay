//! What a run is doing, in the phase vocabulary the status row prints.
//!
//! The phase is the front end's own reading of the stream it observes, not a wire
//! value: `waiting` from the moment a turn request leaves, `thinking` when the
//! reasoning channel produces its first text, `answering` when the answer channel
//! does, and `working` while a tool call has been in flight past the threshold.
//! A terminal run carries no phase: the status row reads the projection's own
//! status word instead.

/** The live phase of one run. */
export type RunPhase = "waiting" | "thinking" | "answering" | "working";

/**
 * How long an open tool call takes to read as work.
 *
 * Half a second: a shorter call leaves the phase it found, so a fast tool never
 * flickers the row.
 */
export const TOOL_WORKING_AFTER_MILLIS = 500;

/** Returns the word the status row prints for one phase. */
export function phaseWord(phase: RunPhase): string {
  switch (phase) {
    case "waiting":
      return "Waiting…";
    case "thinking":
      return "Thinking…";
    case "answering":
      return "Answering…";
    case "working":
      return "Working…";
  }
}

/**
 * Returns a measured elapsed value in tenths of a second.
 *
 * A live value reads in tenths because that is the resolution the front end
 * actually measures: the committed duration reads in whole seconds.
 */
export function elapsedTenths(millis: number): string {
  return `${(Math.max(0, millis) / 1000).toFixed(1)}s`;
}

/** Returns the committed whole seconds between one run's start and finish. */
export function committedSeconds(startedAt: number, finishedAt: number): number {
  return Math.max(0, finishedAt - startedAt);
}
