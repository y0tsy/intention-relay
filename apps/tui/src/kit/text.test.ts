//! The kernel's one width: wrap, pad, and cut are the same measurement.

import assert from "node:assert/strict";
import test from "node:test";
import {
  cellText,
  cellTextRight,
  displayWidth,
  markerText,
  pad,
  takeRows,
  truncate,
  wrapRows,
} from "./text";

test("a wide glyph takes two columns", () => {
  assert.equal(displayWidth("ab"), 2);
  assert.equal(displayWidth("日本"), 4);
});

test("a cell pads to its column and never widens it", () => {
  assert.equal(pad("ab", 4), "ab  ");
  assert.equal(pad("abcdef", 2), "abcdef");
  assert.equal(cellText("abcdef", 4), "abc…");
  assert.equal(cellText("日本", 3), "日…", "a wide glyph is dropped whole");
  assert.equal(cellTextRight("ab", 4), "  ab");
});

test("a cut keeps one column for its ellipsis", () => {
  assert.equal(truncate("abcdef", 4), "abc…");
  assert.equal(truncate("abc", 4), "abc");
  assert.equal(truncate("abc", 0), "");
});

test("a marker keeps its own gap", () => {
  assert.equal(markerText("∷", "answer"), "∷ answer");
  assert.equal(markerText("∷", "answer", 2), "∷  answer");
});

test("a word wider than the column is cut, never widened", () => {
  assert.deepEqual(wrapRows("abcdefghij", 4), ["abcd", "efgh", "ij"]);
});

test("takeRows counts the rows it hid", () => {
  assert.deepEqual(takeRows("one two three four", 9, 2), {
    shown: ["one two", "three"],
    hidden: 1,
  });
});

test("an empty run takes no row", () => {
  assert.deepEqual(takeRows("", 9, 3), { shown: [""], hidden: 0 });
});
