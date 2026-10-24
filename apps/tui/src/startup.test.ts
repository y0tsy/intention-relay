//! The bytes the application writes before the renderer takes the terminal.

import assert from "node:assert/strict";
import test from "node:test";
import { pushScreen, windowRows } from "./startup";

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
