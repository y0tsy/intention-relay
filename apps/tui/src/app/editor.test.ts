//! The draft's own edits: the caret, the line breaks, and the lines it walks.

import assert from "node:assert/strict";
import test from "node:test";
import { type Draft, editDraft, moveCaretLine } from "./editor";

/** Returns one draft with the caret at `caret`. */
function draft(text: string, caret = text.length): Draft {
  return { text, caret };
}

test("the arrows keep the caret's column while they walk the lines", () => {
  assert.deepEqual(moveCaretLine(draft("one\ntwo\nthree", 2), 1), draft("one\ntwo\nthree", 6));
  assert.deepEqual(moveCaretLine(draft("one\ntwo\nthree", 6), -1), draft("one\ntwo\nthree", 2));
});

test("a column the target line cannot hold stops at its own end", () => {
  assert.deepEqual(moveCaretLine(draft("long line\nx", 8), 1), draft("long line\nx", 11));
  assert.deepEqual(moveCaretLine(draft("x\nlong line", 2), -1), draft("x\nlong line", 0));
});

test("the first and last line have no line to walk to", () => {
  assert.equal(moveCaretLine(draft("one\ntwo", 1), -1), null);
  assert.equal(moveCaretLine(draft("one\ntwo", 6), 1), null);
  assert.equal(moveCaretLine(draft("one", 1), 1), null, "one line walks nothing");
});

test("Home and End move the caret to the draft's own edges", () => {
  assert.deepEqual(editDraft(draft("one\ntwo", 3), "home"), draft("one\ntwo", 0));
  assert.deepEqual(editDraft(draft("one\ntwo", 3), "end"), draft("one\ntwo", 7));
});
