//! The keyboard: one event in, one intent out.
//!
//! The mapping lives here rather than inside the component, so "a modified
//! return is a line break", "escape stops a live run before it clears the draft",
//! and "a surface owns the arrow keys while it is open" are decisions a test can
//! read instead of a chain of `if`s a reader has to simulate.

import { decodePasteBytes } from "@opentui/core";
import type { Surface } from "../app/state";

/** One key event, as the renderer reports it. */
export interface KeyEvent {
  /** The key's name: `return`, `escape`, `up`, or a single character. */
  readonly name: string;
  readonly ctrl: boolean;
  readonly meta: boolean;
  readonly shift: boolean;
  /** `press` or `repeat` for a gesture; `release` is not a gesture. */
  readonly eventType?: "press" | "repeat" | "release";
}

/** What the surface the keyboard is on has to do about the event. */
export type KeyIntent =
  /** The event is not a gesture this surface knows. */
  | { readonly kind: "ignore" }
  /** Leave the front end. */
  | { readonly kind: "quit" }
  /** Hand the draft to the command grammar or to the session as a turn. */
  | { readonly kind: "submit" }
  /** Put a line break into the draft. */
  | { readonly kind: "newline" }
  /** Stop the live run. */
  | { readonly kind: "stop_run" }
  /** Throw the draft away. */
  | { readonly kind: "clear_draft" }
  /** Move or delete one character of the draft. */
  | { readonly kind: "edit"; readonly edit: "backspace" | "delete" | "left" | "right" }
  /** Walk the prompt history one entry. */
  | { readonly kind: "recall"; readonly delta: number }
  /** Scroll the transcript half a panel. */
  | { readonly kind: "scroll"; readonly delta: number }
  /** Put text into the draft. */
  | { readonly kind: "insert"; readonly text: string }
  /** Move the selection of the open surface. */
  | { readonly kind: "move_selection"; readonly delta: number }
  /** Commit the selection of the open surface. */
  | { readonly kind: "commit_selection" }
  /** Dismiss the open surface. */
  | { readonly kind: "close_surface" }
  /** Reveals one more chunk of the newest reasoning block. */
  | { readonly kind: "expand_reasoning" }
  /** Types one character into the session browser's filter. */
  | { readonly kind: "filter_typed"; readonly text: string }
  /** Deletes the last character of the session browser's filter. */
  | { readonly kind: "filter_backspace" }
  /** Moves the session browser to its other tab. */
  | { readonly kind: "switch_tab"; readonly delta: number }
  /** Walks the command band's rows. */
  | { readonly kind: "move_menu"; readonly delta: number };

/** What the intent depends on beyond the event itself. */
export interface KeyContext {
  /** The surface that owns the keyboard. */
  readonly surface: Surface;
  /** Whether a run is live, which decides what escape means. */
  readonly live: boolean;
  /** Whether the command band is open, which decides what the arrows walk. */
  readonly menu: boolean;
  /** The panel's height, which decides how far one page scrolls. */
  readonly height: number;
}

/** Returns the text a printable key types, or `null` for anything else. */
export function typed(event: {
  readonly name: string;
  readonly ctrl: boolean;
  readonly meta: boolean;
}): string | null {
  if (event.ctrl || event.meta) {
    return null;
  }
  if (event.name === "space") {
    return " ";
  }
  return [...event.name].length === 1 ? event.name : null;
}

/**
 * Returns the text one paste carries.
 *
 * A paste is data, never a gesture: its newlines are the prompt's own, so a
 * pasted block lands in the draft whole instead of submitting at the first line
 * break the way a typed return would.
 */
export function pastedText(bytes: Uint8Array): string {
  return decodePasteBytes(bytes);
}

/**
 * Returns what one key event means on one surface.
 *
 * Leaving is available everywhere: a user who opens a surface and wants out is
 * never trapped in it. Everything else is the open surface's to claim, which is
 * what keeps the browser's arrows from moving a caret the reader cannot see.
 */
export function keyIntent(event: KeyEvent, context: KeyContext): KeyIntent {
  // A release is not a gesture: the renderer reports both edges of a key, and
  // acting on both would submit every prompt twice.
  if (event.eventType === "release") {
    return { kind: "ignore" };
  }
  if (event.ctrl && event.name === "d") {
    return { kind: "quit" };
  }
  // Ctrl+E reveals more of the newest reasoning block on the chat surface; on a
  // surface that owns the keyboard it stays a key that surface may claim.
  if (event.ctrl && event.name === "e" && context.surface === "chat") {
    return { kind: "expand_reasoning" };
  }
  if (context.surface === "sessions") {
    // The browser's own keys: arrows and enter walk the table, tab switches the
    // list it ranks, and every printable key narrows the filter it shows.
    switch (event.name) {
      case "tab":
        return { kind: "switch_tab", delta: event.shift ? -1 : 1 };
      case "backspace":
        return { kind: "filter_backspace" };
      default: {
        const text = typed(event);
        if (text !== null) {
          return { kind: "filter_typed", text };
        }
      }
    }
  }
  if (context.surface !== "chat") {
    switch (event.name) {
      case "up":
      case "k":
        return { kind: "move_selection", delta: -1 };
      case "down":
      case "j":
        return { kind: "move_selection", delta: 1 };
      case "return":
        return { kind: "commit_selection" };
      case "escape":
        return { kind: "close_surface" };
      default:
        return { kind: "ignore" };
    }
  }
  if (event.name === "return") {
    // A prompt may be several lines, so a modified return is a line break and
    // the unmodified key is what submits one.
    return event.shift || event.ctrl ? { kind: "newline" } : { kind: "submit" };
  }
  if (event.ctrl && event.name === "j") {
    return { kind: "newline" };
  }
  if (event.name === "escape") {
    return context.live ? { kind: "stop_run" } : { kind: "clear_draft" };
  }
  switch (event.name) {
    case "backspace":
      return { kind: "edit", edit: "backspace" };
    case "delete":
      return { kind: "edit", edit: "delete" };
    case "left":
      return { kind: "edit", edit: "left" };
    case "right":
      return { kind: "edit", edit: "right" };
    case "up":
      return context.menu ? { kind: "move_menu", delta: -1 } : { kind: "recall", delta: -1 };
    case "down":
      return context.menu ? { kind: "move_menu", delta: 1 } : { kind: "recall", delta: 1 };
    case "pageup":
      return { kind: "scroll", delta: -Math.max(1, Math.floor(context.height / 2)) };
    case "pagedown":
      return { kind: "scroll", delta: Math.max(1, Math.floor(context.height / 2)) };
    default: {
      const text = typed(event);
      return text === null ? { kind: "ignore" } : { kind: "insert", text };
    }
  }
}
