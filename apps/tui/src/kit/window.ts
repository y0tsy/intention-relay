//! The window an application paints into.
//!
//! The render region is the footer, so the window's own height comes from the
//! terminal itself: a surface that reads it here never has to know which part of
//! the terminal the renderer owns.

import { useRenderer } from "@opentui/react";
import { useSyncExternalStore } from "react";

/** One terminal window: the columns and rows the application sees. */
export interface Window {
  /** The terminal's own width. */
  readonly columns: number;
  /** The terminal's own height, not the render region's. */
  readonly rows: number;
}

/** The renderer fields a window reads and the event it follows. */
interface WindowSource {
  readonly width: number;
  readonly terminalHeight: number;
  on(event: "resize", listener: () => void): unknown;
  off(event: "resize", listener: () => void): unknown;
}

/** One renderer's window, shared by every component that reads it. */
interface WindowStore {
  readonly subscribe: (listener: () => void) => () => void;
  readonly getSnapshot: () => Window;
}

const stores = new WeakMap<object, WindowStore>();

/**
 * Returns the one window store for a renderer.
 *
 * A store follows the renderer's resize event once, however many components
 * read the window: a hook that subscribed per component would spend one
 * listener per block until the renderer refused to add another.
 */
function windowStore(source: WindowSource): WindowStore {
  const existing = stores.get(source);
  if (existing !== undefined) {
    return existing;
  }
  let snapshot: Window = { columns: source.width, rows: source.terminalHeight };
  const listeners = new Set<() => void>();
  const update = () => {
    const next: Window = { columns: source.width, rows: source.terminalHeight };
    if (next.columns === snapshot.columns && next.rows === snapshot.rows) {
      return;
    }
    snapshot = next;
    for (const listener of listeners) {
      listener();
    }
  };
  const store: WindowStore = {
    subscribe: (listener) => {
      listeners.add(listener);
      if (listeners.size === 1) {
        source.on("resize", update);
      }
      update();
      return () => {
        listeners.delete(listener);
        if (listeners.size === 0) {
          source.off("resize", update);
        }
      };
    },
    getSnapshot: () => snapshot,
  };
  stores.set(source, store);
  return store;
}

/** Returns the window the application paints into. */
export function useWindow(): Window {
  const renderer = useRenderer();
  const store = windowStore(renderer);
  return useSyncExternalStore(store.subscribe, store.getSnapshot, store.getSnapshot);
}
