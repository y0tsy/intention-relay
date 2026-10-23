//! The one binding between the store and React.
//!
//! A view subscribes to the values it reads, not to the store as a whole: the
//! hook re-renders its component when the value its selector returned moved, and
//! leaves every other view alone. That is what keeps a streamed frame out of the
//! committed rows and a keystroke out of the transcript.

import { useCallback, useRef, useSyncExternalStore } from "react";
import type { AppState } from "../app/state";
import type { AppStore } from "../app/store";

/** The value one selector last produced, and the store version it read. */
interface Selection<T> {
  readonly select: (state: AppState) => T;
  readonly version: number;
  readonly value: T;
}

/**
 * Returns one value of the store, and repaints when that value moved.
 *
 * The store is versioned, so a selection is read once per version and handed
 * back unchanged until the state moves. That is what lets a selector derive a
 * value - a usage pair, a list built from the run - without React seeing a new
 * identity on every read and repainting until it runs out of depth.
 */
export function useStore<T>(store: AppStore, select: (state: AppState) => T): T {
  const held = useRef<Selection<T> | null>(null);
  const subscribe = useCallback((notify: () => void) => store.subscribe(notify), [store]);
  const getSnapshot = useCallback(() => {
    const version = store.getVersion();
    const last = held.current;
    if (last !== null && last.select === select && last.version === version) {
      return last.value;
    }
    const value = select(store.state);
    held.current = { select, version, value };
    return value;
  }, [store, select]);
  return useSyncExternalStore(subscribe, getSnapshot);
}
