//! The keyboard: one event in, one intent out.
//!
//! The mapping lives here rather than inside the component, so "a modified
//! return is a line break", "escape is the stop and nothing else", and "a surface
//! owns the arrow keys while it is open" are decisions a test can read instead of
//! a chain of `if`s a reader has to simulate.

import type { Action } from "./action";
import type { Surface } from "./state";

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
  /** The layered interrupt: stop the live run, clear the line, or exit. */
  | { readonly kind: "interrupt" }
  /** The layered escape, whose meaning the state it finds decides. */
  | { readonly kind: "escape" }
  /** Hand the draft to the command grammar or to the session as a turn. */
  | { readonly kind: "submit" }
  /** Put a line break into the draft. */
  | { readonly kind: "newline" }
  /** Commit the row the command band highlights into the draft. */
  | { readonly kind: "accept_menu" }
  /** Move the caret or delete, one character or one paste label at a time. */
  | {
      readonly kind: "edit";
      readonly edit: "backspace" | "delete" | "left" | "right" | "home" | "end";
    }
  /** Walk the prompt history one entry. */
  | { readonly kind: "recall"; readonly delta: number }
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
  /** Whether the command band is open, which decides what the arrows walk. */
  readonly menu: boolean;
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
  // Ctrl+C is the layered key every surface shares, so it reaches the stop,
  // clear, and exit arms of the state.
  if (event.ctrl && event.name === "c") {
    return { kind: "interrupt" };
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
    return { kind: "escape" };
  }
  if (event.name === "tab" && context.menu) {
    return { kind: "accept_menu" };
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
    case "home":
      return { kind: "edit", edit: "home" };
    case "end":
      return { kind: "edit", edit: "end" };
    case "up":
      return context.menu ? { kind: "move_menu", delta: -1 } : { kind: "recall", delta: -1 };
    case "down":
      return context.menu ? { kind: "move_menu", delta: 1 } : { kind: "recall", delta: 1 };
    default: {
      const text = typed(event);
      return text === null ? { kind: "ignore" } : { kind: "insert", text };
    }
  }
}

/** Returns the store action one intent asks for, or `null` for an ignored key. */
export function intentAction(intent: KeyIntent): Action | null {
  switch (intent.kind) {
    case "ignore":
      return null;
    case "interrupt":
      return { kind: "ctrl_c_pressed" };
    case "escape":
      return { kind: "escape_pressed" };
    case "submit":
      return { kind: "input_submitted" };
    case "newline":
      return { kind: "draft_newline" };
    case "accept_menu":
      return { kind: "menu_accepted" };
    case "edit":
      return { kind: "draft_edited", edit: intent.edit };
    case "recall":
      return { kind: "history_recalled", delta: intent.delta };
    case "insert":
      return { kind: "draft_typed", text: intent.text };
    case "move_selection":
      return { kind: "selection_moved", delta: intent.delta };
    case "commit_selection":
      return { kind: "selection_committed" };
    case "close_surface":
      return { kind: "surface_closed" };
    case "expand_reasoning":
      return { kind: "reasoning_expanded", id: null };
    case "filter_typed":
      return { kind: "browser_filter_typed", text: intent.text };
    case "filter_backspace":
      return { kind: "browser_filter_backspace" };
    case "switch_tab":
      return { kind: "browser_tab_moved", delta: intent.delta };
    case "move_menu":
      return { kind: "menu_moved", delta: intent.delta };
  }
}
