//! The pastes: what the line holds inline, and the one label a large one leaves.

import assert from "node:assert/strict";
import test from "node:test";
import {
  expandPastes,
  normalisePaste,
  PASTE_INLINE_LINES,
  type PastedText,
  pasteIsLarge,
  pasteLabel,
  pasteSpans,
} from "./paste";

/** Returns one paste of `lines` lines, each long enough to be read. */
function block(lines: number): string {
  return Array.from({ length: lines }, (_, index) => `line ${index}`).join("\n");
}

test("a terminal's own line endings read as one spelling of a line break", () => {
  assert.equal(normalisePaste("one\r\ntwo\rthree\n"), "one\ntwo\nthree\n");
});

test("size decides which paste the line cannot show", () => {
  assert.equal(pasteIsLarge("one\ntwo"), false);
  assert.equal(pasteIsLarge(block(PASTE_INLINE_LINES)), false, "the limit's own lines are inline");
  assert.equal(pasteIsLarge(block(PASTE_INLINE_LINES + 1)), true);
  assert.equal(pasteIsLarge("x".repeat(401)), true);
  assert.equal(pasteIsLarge("x".repeat(400)), false);
});

test("the label names the paste's own beginning and how many lines it carries", () => {
  assert.equal(pasteLabel("hello world"), "[pasted hello world... 1 line]");
  assert.equal(
    pasteLabel("one\ntwo\nthree"),
    "[pasted one two three... 3 lines]",
    "the head is the text's own beginning, read as one line of prose",
  );
  assert.equal(
    pasteLabel("   indented\nsecond"),
    "[pasted indented second... 2 lines]",
    "the head is trimmed after its breaks read as spaces",
  );
});

test("a label drops the characters the line cannot show and reads breaks as spaces", () => {
  assert.equal(pasteLabel("\u0007one\ttwo"), "[pasted one two... 1 line]");
  assert.equal(
    pasteLabel("\u0007\u0008"),
    "[pasted ... 1 line]",
    "control characters show no head",
  );
});

test("a label is one span of the line, found once per paste", () => {
  const paste: PastedText = { label: "[pasted x... 1 line]", text: "x" };
  const text = `a ${paste.label} b ${paste.label}`;
  const size = paste.label.length;
  assert.deepEqual(pasteSpans(text, [paste, paste]), [
    { start: 2, end: 2 + size, index: 0 },
    { start: 5 + size, end: 5 + 2 * size, index: 1 },
  ]);
  assert.deepEqual(pasteSpans("nothing here", [paste]), [], "an edited-over label is gone");
});

test("expanding a line puts the pasted text back where its label stood", () => {
  const paste: PastedText = { label: "[pasted x... 1 line]", text: "one\ntwo" };
  assert.equal(expandPastes(`send ${paste.label} now`, [paste]), "send one\ntwo now");
  assert.equal(expandPastes("no label", [paste]), "no label");
});
