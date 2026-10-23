//! The one binding between the store and React.
//!
//! A view subscribes to the values it reads, not to the store as a whole: the
//! hook re-renders its component when the value its selector returned moved, and
//! leaves every other view alone. That is what keeps a streamed frame out of the
//! committed rows and a keystroke out of the transcript.

import { useCallback, useSyncExternalStore } from "react";
import type { AppState } from "../app/state";
import type { AppStore } from "../app/store";

/**
 * Returns one value of the store, and repaints when that value moves.
 *
 * `select` must return a primitive or an identity the state owns, never a value
 * built on the spot: React compares what it is handed, and a fresh array or
 * object would make every read look like a change.
 */
export function useStore<T>(store: AppStore, select: (state: AppState) => T): T {
  const subscribe = useCallback((notify: () => void) => store.subscribe(notify), [store]);
  return useSyncExternalStore(subscribe, () => select(store.state));
}
