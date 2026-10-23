//! The keyboard: one event in, one intent out, on each surface.

import assert from "node:assert/strict";
import test from "node:test";
import { type KeyContext, type KeyEvent, keyIntent, typed } from "./keys";

/** A plain key event with nothing modified. */
function key(name: string, overrides: Partial<KeyEvent> = {}): KeyEvent {
  return { name, ctrl: false, meta: false, shift: false, ...overrides };
}

/** The chat surface with no run live. */
const CHAT: KeyContext = { menu: false, surface: "chat", live: false, height: 24 };

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

test("escape stops a live run and otherwise clears the draft", () => {
  assert.deepEqual(chat(key("escape"), { live: true }), { kind: "stop_run" });
  assert.deepEqual(chat(key("escape"), { live: false }), { kind: "clear_draft" });
});

test("the caret keys edit the draft and never a live surface", () => {
  for (const [name, edit] of [
    ["backspace", "backspace"],
    ["delete", "delete"],
    ["left", "left"],
    ["right", "right"],
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

test("a page scrolls half the panel", () => {
  assert.deepEqual(chat(key("pageup"), { height: 24 }), { kind: "scroll", delta: -12 });
  assert.deepEqual(chat(key("pagedown"), { height: 24 }), { kind: "scroll", delta: 12 });
  assert.deepEqual(chat(key("pageup"), { height: 1 }), { kind: "scroll", delta: -1 });
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

test("leaving works on every surface", () => {
  for (const surface of ["chat", "sessions", "theme"] as const) {
    assert.deepEqual(keyIntent(key("d", { ctrl: true }), { ...CHAT, surface }), { kind: "quit" });
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
