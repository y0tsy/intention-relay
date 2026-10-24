//! The one-second clock the surfaces that show a duration read.

import { useEffect, useState } from "react";

/**
 * Runs a one-second clock while `active`, and returns the current second.
 *
 * The clock lives in the component that shows a duration, so a tick repaints the
 * line that reads it and nothing else.
 */
export function useNow(active: boolean): number {
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
