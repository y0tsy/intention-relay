//! The keyboard: one event in, one intent out, on each surface.

import assert from "node:assert/strict";
import test from "node:test";
import type { Action } from "./action";
import {
  type KeyContext,
  type KeyEvent,
  type KeyIntent,
  intentAction,
  keyIntent,
  typed,
} from "./keys";

/** A plain key event with nothing modified. */
function key(name: string, overrides: Partial<KeyEvent> = {}): KeyEvent {
  return { name, ctrl: false, meta: false, shift: false, ...overrides };
}

/** The chat surface with the band closed. */
const CHAT: KeyContext = { menu: false, surface: "chat" };

/** Returns the intent one key has on the chat surface. */
function chat(event: KeyEvent, context: Partial<KeyContext> = {}) {
  return keyIntent(event, { ...CHAT, ...context });
}

test("a release is not a gesture", () => {
  assert.deepEqual(chat(key("return", { eventType: "release" })), { kind: "ignore" });
  assert.deepEqual(chat(key("a", { eventType: "release" })), { kind: "ignore" });
  assert.deepEqual(chat(key("return", { eventType: "press" })), { kind: "submit" });
});

test("return submits and a modified return is a line break", () => {
  assert.deepEqual(chat(key("return")), { kind: "submit" });
  assert.deepEqual(chat(key("return", { shift: true })), { kind: "newline" });
  assert.deepEqual(chat(key("return", { ctrl: true })), { kind: "newline" });
  assert.deepEqual(chat(key("j", { ctrl: true })), { kind: "newline" });
});

test("escape is the stop on the chat surface, whatever the state it finds", () => {
  assert.deepEqual(chat(key("escape")), { kind: "escape" });
});

test("Ctrl+C is the layered interrupt, and no key is a separate exit", () => {
  for (const surface of ["chat", "sessions", "theme"] as const) {
    const context = { ...CHAT, surface };
    assert.deepEqual(keyIntent(key("c", { ctrl: true }), context), { kind: "interrupt" });
    assert.deepEqual(keyIntent(key("q", { ctrl: true }), context), { kind: "ignore" });
  }
});

test("Tab commits the band's row while the band is open", () => {
  assert.deepEqual(keyIntent(key("tab"), { ...CHAT, menu: true }), { kind: "accept_menu" });
  assert.deepEqual(chat(key("tab")), { kind: "ignore" }, "no band, no row to commit");
});

test("Home and End move the caret to the line's own edges", () => {
  assert.deepEqual(chat(key("home")), { kind: "edit", edit: "home" });
  assert.deepEqual(chat(key("end")), { kind: "edit", edit: "end" });
});

test("the caret keys edit the draft and never a live surface", () => {
  for (const [name, edit] of [
    ["backspace", "backspace"],
    ["delete", "delete"],
    ["left", "left"],
    ["right", "right"],
    ["home", "home"],
    ["end", "end"],
  ] as const) {
    assert.deepEqual(chat(key(name)), { kind: "edit", edit });
  }
});

test("the arrows walk the band while it is open and the history otherwise", () => {
  assert.deepEqual(chat(key("up")), { kind: "recall", delta: -1 });
  const band = { ...CHAT, menu: true };
  assert.deepEqual(keyIntent(key("up"), band), { kind: "move_menu", delta: -1 });
  assert.deepEqual(keyIntent(key("down"), band), { kind: "move_menu", delta: 1 });
});

test("the arrow keys walk the history", () => {
  assert.deepEqual(chat(key("up")), { kind: "recall", delta: -1 });
  assert.deepEqual(chat(key("down")), { kind: "recall", delta: 1 });
});

test("a page key is the terminal's, not the prompt's", () => {
  // The transcript is the terminal's scrollback, so the application leaves the
  // keys the terminal owns to the terminal.
  assert.deepEqual(chat(key("pageup")), { kind: "ignore" });
  assert.deepEqual(chat(key("pagedown")), { kind: "ignore" });
});

test("a printable key types and a named key does not", () => {
  assert.deepEqual(chat(key("a")), { kind: "insert", text: "a" });
  assert.deepEqual(chat(key("space")), { kind: "insert", text: " " });
  assert.deepEqual(chat(key("/")), { kind: "insert", text: "/" });
  assert.deepEqual(chat(key("f1")), { kind: "ignore" });
  assert.deepEqual(chat(key("a", { ctrl: true })), { kind: "ignore" });
  assert.deepEqual(chat(key("a", { meta: true })), { kind: "ignore" });
});

test("a modified printable key still types its character", () => {
  // Shift is how a capital letter arrives, so it is not a gesture modifier.
  assert.deepEqual(chat(key("A", { shift: true })), { kind: "insert", text: "A" });
});

test("the front end is left through the layered Ctrl+C and nothing else", () => {
  for (const surface of ["chat", "sessions", "theme"] as const) {
    for (const name of ["d", "q"]) {
      assert.deepEqual(
        keyIntent(key(name, { ctrl: true }), { ...CHAT, surface }),
        { kind: "ignore" },
        `Ctrl+${name.toUpperCase()} is not a quit key`,
      );
    }
  }
});

test("an open surface owns the arrows, the return, and the escape", () => {
  for (const surface of ["sessions", "theme"] as const) {
    const context = { ...CHAT, surface };
    assert.deepEqual(keyIntent(key("up"), context), { kind: "move_selection", delta: -1 });
    assert.deepEqual(keyIntent(key("down"), context), { kind: "move_selection", delta: 1 });
    assert.deepEqual(keyIntent(key("return"), context), { kind: "commit_selection" });
    assert.deepEqual(keyIntent(key("escape"), context), { kind: "close_surface" });
  }
});

test("the browser narrows its own list while the chat keeps its letters", () => {
  const browser = { ...CHAT, surface: "sessions" as const };
  assert.deepEqual(keyIntent(key("k"), browser), { kind: "filter_typed", text: "k" });
  assert.deepEqual(keyIntent(key("space"), browser), { kind: "filter_typed", text: " " });
  assert.deepEqual(keyIntent(key("backspace"), browser), { kind: "filter_backspace" });
  assert.deepEqual(keyIntent(key("tab"), browser), { kind: "switch_tab", delta: 1 });
  assert.deepEqual(keyIntent(key("tab", { shift: true }), browser), {
    kind: "switch_tab",
    delta: -1,
  });
  // The chat surface types the same letters into the draft instead.
  assert.deepEqual(chat(key("k")), { kind: "insert", text: "k" });
});

test("a surface that does not read text swallows it", () => {
  // Typing into the picker would move a caret the reader cannot see, so a
  // surface that does not read text ignores text.
  const picker = { ...CHAT, surface: "theme" as const };
  for (const name of ["a", "space", "backspace", "pagedown", "tab"]) {
    assert.deepEqual(keyIntent(key(name), picker), { kind: "ignore" }, name);
  }
});

test("a paste is data, so it types whatever it carries", () => {
  // The paste handler hands bytes to the same insert path, which is why a
  // multi-line prompt lands in the draft whole instead of submitting at the
  // first newline.
  assert.equal(typed({ name: "a", ctrl: false, meta: false }), "a");
  assert.equal(typed({ name: "space", ctrl: false, meta: false }), " ");
  assert.equal(typed({ name: "return", ctrl: false, meta: false }), null);
});

test("each intent becomes the one action the store expects", () => {
  const cases: readonly [KeyIntent, Action | null][] = [
    [{ kind: "ignore" }, null],
    [{ kind: "interrupt" }, { kind: "ctrl_c_pressed" }],
    [{ kind: "escape" }, { kind: "escape_pressed" }],
    [{ kind: "submit" }, { kind: "input_submitted" }],
    [{ kind: "newline" }, { kind: "draft_newline" }],
    [{ kind: "accept_menu" }, { kind: "menu_accepted" }],
    [
      { kind: "edit", edit: "left" },
      { kind: "draft_edited", edit: "left" },
    ],
    [
      { kind: "recall", delta: -1 },
      { kind: "history_recalled", delta: -1 },
    ],
    [
      { kind: "insert", text: "x" },
      { kind: "draft_typed", text: "x" },
    ],
    [
      { kind: "move_selection", delta: 1 },
      { kind: "selection_moved", delta: 1 },
    ],
    [{ kind: "commit_selection" }, { kind: "selection_committed" }],
    [{ kind: "close_surface" }, { kind: "surface_closed" }],
    [{ kind: "expand_reasoning" }, { kind: "reasoning_expanded", id: null }],
    [
      { kind: "filter_typed", text: "a" },
      { kind: "browser_filter_typed", text: "a" },
    ],
    [{ kind: "filter_backspace" }, { kind: "browser_filter_backspace" }],
    [
      { kind: "switch_tab", delta: 1 },
      { kind: "browser_tab_moved", delta: 1 },
    ],
    [
      { kind: "move_menu", delta: -1 },
      { kind: "menu_moved", delta: -1 },
    ],
  ];
  for (const [intent, action] of cases) {
    assert.deepEqual(intentAction(intent), action, intent.kind);
  }
});
