//! The screen's own pieces: the startup bytes and a print's plan.

import assert from "node:assert/strict";
import test from "node:test";
import {
  chunkRows,
  continues,
  EDGE_ROWS,
  leadRow,
  pushScreen,
  sealedRows,
  transcriptColumns,
  TRANSCRIPT_INDENT,
  windowRows,
} from "./index";

test("a printed row keeps the footer's own text columns", () => {
  // The rule sits under the input's left border and the text under the input's
  // first column, so a printed row and the composer line up at every width.
  assert.equal(TRANSCRIPT_INDENT, 5);
  assert.equal(transcriptColumns(80), 71);
  assert.equal(transcriptColumns(4), 0, "a page too narrow for its rule keeps no text");
});

test("the window clears what is on screen and keeps the history", () => {
  const bytes = pushScreen(30);
  assert.equal(bytes, `${"\n".repeat(30)}\x1B[30A\x1B[J`);
  assert.ok(!bytes.includes("\x1B[3J"), "the saved lines are the session's history");
  assert.ok(!bytes.includes("\x1B[2J"), "erasing the whole screen is not what the window needs");
  assert.ok(bytes.endsWith("\x1B[J"), "the erase runs from the cursor down, and no further");
});

test("a terminal that does not report its rows gets a window of its own", () => {
  assert.equal(windowRows(undefined), 24);
  assert.equal(windowRows(0), 24);
  assert.equal(windowRows(30), 30);
  assert.equal(windowRows(1), 1, "a one-row window is still a window");
});

test("the rows the window pushes are the rows it returns to", () => {
  // The cursor has to come back exactly as far as it went down: a short move
  // leaves the application painting the shell's rows and a long one homes it.
  for (const rows of [1, 24, 30, 60]) {
    const bytes = pushScreen(rows);
    const pushed = (bytes.match(/\n/g) ?? []).length;
    assert.equal(pushed, rows);
    assert.ok(bytes.includes(`\x1B[${pushed}A`));
  }
});

test("the first print of a session leads with nothing", () => {
  const lead = leadRow(0);
  assert.equal(lead.separator, false, "nothing is above it to be separated from");
});

test("a later print of the same session is separated from the block above it", () => {
  const lead = leadRow(12);
  assert.equal(lead.separator, true, "two blocks are one blank row apart");
});

test("a stream seals every row a later row has superseded", () => {
  assert.equal(sealedRows(0, EDGE_ROWS), 0, "an unmeasured page seals nothing");
  assert.equal(sealedRows(1, EDGE_ROWS), 0, "a page of one row is all edge");
  assert.equal(sealedRows(EDGE_ROWS, EDGE_ROWS), 0, "a page the edge fills seals nothing");
  assert.equal(sealedRows(EDGE_ROWS + 1, EDGE_ROWS), 1, "the row above the edge is final");
  assert.equal(sealedRows(12, EDGE_ROWS), 10);
  assert.equal(sealedRows(12, 0), 12, "a page with no edge is final as it stands");
});

test("a step only grows: a text that no longer starts with it is another step", () => {
  const first = { reasoning: "think", text: "" };
  assert.equal(continues(first, { reasoning: "thinking", text: "" }), true, "the stream appends");
  assert.equal(continues(first, { reasoning: "think", text: "an " }), true, "the answer opens");
  assert.equal(
    continues(first, { reasoning: "", text: "another step" }),
    false,
    "a step that starts over is another step",
  );
  const writing = { reasoning: "think", text: "an " };
  assert.equal(
    continues(writing, { reasoning: "think", text: "another" }),
    false,
    "a text that rewrote itself is another step",
  );
});

test("a chunk never runs past the print", () => {
  assert.equal(chunkRows(0, 9, 16), 9);
  assert.equal(chunkRows(8, 9, 16), 1, "the last row is a chunk of its own");
  assert.equal(chunkRows(9, 9, 16), 0, "a print that is done takes no row");
  assert.equal(chunkRows(8, 9, 0), 1, "a room of no rows still prints one row at a time");
  assert.equal(chunkRows(0, 9, 4), 4);
});
