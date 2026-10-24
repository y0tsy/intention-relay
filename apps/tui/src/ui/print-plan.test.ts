//! How a print is laid out: the row it leads with and the chunks it goes in.

import assert from "node:assert/strict";
import test from "node:test";
import { chunkRows, chunks, leadRow, leadRows } from "./print-plan";

test("the first print of a session opens the frame", () => {
  const lead = leadRow(0, false);
  assert.equal(lead.top, true, "the first print carries the frame's top edge");
  assert.equal(lead.separator, false, "nothing is above it to be separated from");
  assert.equal(lead.closing, false, "no session is open above it");
  assert.equal(leadRows(lead), 1);
});

test("a later print of the same session is separated from the block above it", () => {
  const lead = leadRow(12, false);
  assert.equal(lead.separator, true, "two blocks are one blank row apart");
  assert.equal(lead.top, false, "the frame's top edge was printed once already");
  assert.equal(lead.closing, false);
  assert.equal(leadRows(lead), 1);
});

test("the first print after a session switch closes the frame above it", () => {
  const lead = leadRow(12, true);
  assert.equal(lead.closing, true, "the previous session's frame gets its bottom edge");
  assert.equal(lead.top, true, "and the new session's frame gets its top edge");
  assert.equal(lead.separator, false, "the closing edge separates them");
  assert.equal(leadRows(lead), 2);
});

test("a print that fits the room goes in one chunk", () => {
  assert.deepEqual(chunks(9, 16), [9]);
  assert.equal(chunkRows(0, 9, 16), 9);
});

test("a print taller than the room is split into chunks the room holds", () => {
  assert.deepEqual(chunks(40, 16), [16, 16, 8], "the last chunk carries the remainder");
  assert.equal(chunks(32, 16).length, 2, "a print that is exactly the room twice is two chunks");
});

test("every chunk is a positive count", () => {
  assert.deepEqual(chunks(0, 16), [], "an empty print has no rows to commit");
  assert.deepEqual(chunks(-3, 16), []);
  assert.deepEqual(chunks(3, 0), [1, 1, 1], "a room of no rows still prints one row at a time");
  assert.equal(chunkRows(8, 9, 16), 1, "the last row is a chunk of its own");
  assert.equal(chunkRows(9, 9, 16), 0, "a print that is done takes no row");
  assert.equal(chunkRows(8, 9, 0), 1);
});

test("a chunk never runs past the print", () => {
  let at = 0;
  for (const size of chunks(37, 10)) {
    at += size;
    assert.ok(at <= 37, "the chunks cover the print exactly once");
  }
  assert.equal(at, 37);
});
