//! The composer's editing rules and the history a session keeps.

import assert from "node:assert/strict";
import test from "node:test";
import {
  backspace,
  caretToEnd,
  caretToStart,
  characterAt,
  deleteForward,
  EMPTY_DRAFT,
  insertNewline,
  insertText,
  moveCaret,
  renderDraft,
  replaceDraft,
} from "./editor";
import { PromptHistory } from "./history";

test("typed text lands at the caret and the caret follows it", () => {
  const draft = insertText(EMPTY_DRAFT, "hello");
  assert.deepEqual(draft, { text: "hello", caret: 5 });
  assert.deepEqual(insertText(replaceDraft("hello"), "!"), { text: "hello!", caret: 6 });
  assert.deepEqual(insertText({ text: "hello", caret: 0 }, ">"), { text: ">hello", caret: 1 });
});

test("a newline is one character of the same draft", () => {
  assert.deepEqual(insertNewline({ text: "a", caret: 1 }), { text: "a\n", caret: 2 });
});

test("backspace removes before the caret and stops at the start", () => {
  assert.deepEqual(backspace({ text: "hello", caret: 5 }), { text: "hell", caret: 4 });
  assert.deepEqual(backspace({ text: "hello", caret: 0 }), { text: "hello", caret: 0 });
  assert.deepEqual(backspace(EMPTY_DRAFT), EMPTY_DRAFT);
});

test("delete removes after the caret and stops at the end", () => {
  assert.deepEqual(deleteForward({ text: "hello", caret: 0 }), { text: "ello", caret: 0 });
  assert.deepEqual(deleteForward({ text: "hello", caret: 5 }), { text: "hello", caret: 5 });
});

test("the caret moves within the draft and never past either end", () => {
  assert.deepEqual(moveCaret({ text: "hello", caret: 5 }, -2), { text: "hello", caret: 3 });
  assert.deepEqual(moveCaret({ text: "hello", caret: 0 }, -1), { text: "hello", caret: 0 });
  assert.deepEqual(moveCaret({ text: "hello", caret: 5 }, 3), { text: "hello", caret: 5 });
  assert.equal(caretToStart({ text: "hello", caret: 3 }).caret, 0);
  assert.equal(caretToEnd({ text: "hello", caret: 3 }).caret, 5);
});

test("the caret is drawn where the next character goes", () => {
  assert.equal(renderDraft({ text: "hello", caret: 5 }), "hello▏");
  assert.equal(renderDraft({ text: "hello", caret: 0 }), "▏hello");
  assert.equal(renderDraft({ text: "hi", caret: 1 }, "|"), "h|i");
});

test("a surrogate pair counts as one character", () => {
  const text = "a🌍b";
  assert.equal(characterAt(text, 0), 1);
  assert.equal(characterAt(text, 1), 2, "the emoji is one character wide");
});

test("history walks back through the prompts and returns to the live draft", () => {
  const history = new PromptHistory();
  assert.equal(history.navigate(-1), null, "an empty history recalls nothing");
  history.push("one");
  history.push("two");
  assert.equal(history.navigate(-1), "two");
  assert.equal(history.navigate(-1), "one");
  assert.equal(history.navigate(-1), "one", "the walk stops at the oldest prompt");
  assert.equal(history.navigate(1), "two");
  assert.equal(history.navigate(1), null, "past the newest prompt is the live draft");
  assert.equal(history.atLive, true);
});

test("a repeated submission is not stored twice", () => {
  const history = new PromptHistory();
  history.push("same");
  history.push("same");
  assert.deepEqual(history.entries, ["same"]);
  history.push("   ");
  assert.deepEqual(history.entries, ["same"], "a blank submission is not a prompt");
});

test("a new submission resets the walk to the live draft", () => {
  const history = new PromptHistory();
  history.push("one");
  history.navigate(-1);
  history.push("two");
  assert.equal(history.atLive, true);
  assert.deepEqual(history.entries, ["one", "two"]);
});
