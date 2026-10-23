//! The one consecutive-press arm the layered keys share.
//!
//! A key that needs two consecutive presses to act leaves an [`Arm`] naming
//! what its next press fires and shows the notice that asks for that press; the
//! second press fires it and drops the arm. The arm is consecutive over the
//! user's own actions: every action that is not a press of the key the arm
//! belongs to drops it, while a client report leaves it standing, because a live
//! run's frames and the front end's elapsed reports arrive between two presses.
//!
//! The arm names the act, not only the key, because the act is what decides
//! whether a press fires it: a stop arm fires only while its run is still live,
//! and an exit arm fires only when the input line is still empty and no run is
//! live, so a state the press no longer matches starts a fresh sequence instead
//! of firing a stale one.

/** What the last press of a layered key armed for its next consecutive press. */
export type Arm =
  /** The next Ctrl+C press stops the run the notice named. */
  | "stop"
  /** The next Ctrl+C press leaves the front end. */
  | "exit";

/** The notice an exit arm shows until its consecutive press arrives. */
export const EXIT_ARM_NOTICE = "press Ctrl+C again to exit";

/** The characters of a run's identity a notice spells. */
export const SHORT_IDENTIFIER_CHARS = 8;

/** Returns the notice a stop arm shows until its consecutive press arrives. */
export function stopArmNotice(runId: string): string {
  return `press Ctrl+C again to stop run ${runId.slice(0, SHORT_IDENTIFIER_CHARS)}`;
}
