//! The clock a live turn is measured by.
//!
//! The state records when a turn began and what the last measurement was; this
//! hook is what measures. It reads the wall clock ten times a second while a
//! turn is in flight and feeds the state, so the status row's tenth moves and
//! the tool watch can decide that an open call has become work.

import { useEffect } from "react";
import type { AppStore } from "../app/store";

/**
 * How often a live turn measures itself.
 *
 * Ten readings a second: the row prints one decimal, so a slower clock would
 * step over the digit it shows.
 */
export const ELAPSED_TICK_MILLIS = 100;

/** Feeds the state the elapsed time of the turn in flight, while it is in flight. */
export function useRunClock(store: AppStore, turnStartedAt: number | null, live: boolean): void {
  useEffect(() => {
    if (turnStartedAt === null || !live) {
      return;
    }
    const measure = () => {
      store.dispatch({ kind: "elapsed_reported", millis: Date.now() - turnStartedAt });
    };
    // The row reads `0.0s` from the moment the turn opens, not a tenth later.
    measure();
    const timer = setInterval(measure, ELAPSED_TICK_MILLIS);
    return () => {
      clearInterval(timer);
    };
  }, [store, turnStartedAt, live]);
}
