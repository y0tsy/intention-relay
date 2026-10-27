//! The command hint band: the rows the word the caret sits in opens.

import assert from "node:assert/strict";
import test from "node:test";
import type { Draft } from "../app/editor";
import { commandMenu, MAX_COMMAND_ROWS } from "../app/menu";
import { bandSlotRows } from "../kit";
import { frameLines, painted } from "../testing/render";
import { DARK } from "../theme/palette";
import { HintBand } from "./HintBand";

/** Reads the draft with the caret at its end. */
function draft(text: string): Draft {
  return { text, caret: text.length };
}

/** Renders the band one word opens at one width and returns its lines. */
async function lines(text: string, width = 60, highlight = 0): Promise<string[]> {
  const menu = commandMenu(draft(text));
  assert.ok(menu !== null, `${text} opens a band`);
  return frameLines(
    await painted(<HintBand menu={menu} highlight={highlight} palette={DARK} />, width, 10),
  );
}

test("a bare slash ranks the whole registry, which the band windows", async () => {
  const menu = commandMenu(draft("/"));
  assert.equal(menu?.kind, "command");
  assert.deepEqual(
    menu?.rows.map((row) => row.label),
    ["/new", "/sessions", "/theme", "/quit"],
  );
  const painted_ = await lines("/");
  assert.ok(painted_.some((line) => /> \/new/.test(line)));
  assert.equal(
    painted_.some((line) => line.includes("/quit")),
    false,
    "the whole list is not painted",
  );
  assert.ok(
    painted_.some((line) => /… and 1 more command/.test(line)),
    "the noun agrees with the count it reports",
  );
});

test("a prefix narrows the registry to the rows it ranks", async () => {
  const menu = commandMenu(draft("/t"));
  assert.equal(menu?.kind, "command");
  assert.deepEqual(
    menu?.rows.map((row) => row.label),
    ["/theme"],
  );
});

test("the window follows the row the reader is on", async () => {
  const last = await lines("/", 60, 3);
  assert.ok(
    last.some((line) => /^│\s+> \/quit/.test(line)),
    "the cursor's own row is painted",
  );
  assert.ok(last.some((line) => /… and 1 more command/.test(line)));
});

test("a prompt is not a command: the caret's word opens no band", async () => {
  assert.equal(commandMenu(draft("explain this")), null);
  assert.equal(commandMenu(draft("a / b")), null, "a slash past the first character is text");
  assert.equal(commandMenu(draft("/new ")), null, "the command declares no argument");
  assert.equal(commandMenu(draft("/sessions ")), null, "the command declares no argument");
});

test("the theme's word ranks its declared values", async () => {
  const menu = commandMenu(draft("/theme d"));
  assert.equal(menu?.kind, "argument");
  assert.deepEqual(
    menu?.rows.map((row) => row.label),
    ["dark"],
  );
  assert.deepEqual(
    commandMenu(draft("/theme "))?.rows.map((row) => row.label),
    ["auto", "dark", "light"],
    "a space after the command reopens the band on every value",
  );
});

test("the band spends two frame rows around the rows it shows", async () => {
  assert.equal(bandSlotRows(null, MAX_COMMAND_ROWS), 0);
  assert.equal(bandSlotRows(0, MAX_COMMAND_ROWS), 0);
  assert.equal(bandSlotRows(MAX_COMMAND_ROWS, MAX_COMMAND_ROWS), 2 + MAX_COMMAND_ROWS);
  assert.equal(
    bandSlotRows(6, MAX_COMMAND_ROWS),
    2 + MAX_COMMAND_ROWS + 1,
    "the overflow row reports the rest",
  );
});

test("the band paints the rows with the first one cursored", async () => {
  const painted_ = await lines("/s", 72);
  const text = painted_.join("\n");
  assert.match(text, /Commands/);
  assert.match(text, /> \/sessions/);
  assert.match(text, /browse the sessions on this machine/);
  assert.match(text, /Navigation/);
});
