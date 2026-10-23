//! The command hint band: the rows the word the caret sits in opens.

import assert from "node:assert/strict";
import test from "node:test";
import type { Draft } from "../app/editor";
import { commandMenu, menuRows } from "../app/menu";
import { frameLines, painted } from "../testing/render";
import { DARK } from "../theme/palette";
import { HintBand } from "./HintBand";

/** Reads the draft with the caret at its end. */
function draft(text: string): Draft {
  return { text, caret: text.length };
}

/** Renders the band one word opens at one width and returns its lines. */
async function lines(text: string, width = 60): Promise<string[]> {
  const menu = commandMenu(draft(text));
  assert.ok(menu !== null, `/${text} opens a band`);
  return frameLines(
    await painted(<HintBand menu={menu} palette={DARK} width={width} />, width, 10),
  );
}

test("a `/` prefix ranks the registry and cursors the first match", async () => {
  const menu = commandMenu(draft("/t"));
  assert.equal(menu?.kind, "command");
  assert.deepEqual(
    menu?.rows.map((row) => row.label),
    ["/theme"],
  );
  assert.equal(menu?.highlight, 0);
});

test("a named word cursors itself", async () => {
  assert.equal(commandMenu(draft("/new"))?.highlight, 0);
});

test("a prompt is not a command: the caret's word opens no band", async () => {
  assert.equal(commandMenu(draft("explain this")), null);
  assert.equal(commandMenu(draft("/sessions ")), null);
});

test("the theme's word ranks its declared values", async () => {
  const menu = commandMenu(draft("/theme d"));
  assert.equal(menu?.kind, "argument");
  assert.deepEqual(
    menu?.rows.map((row) => row.label),
    ["dark"],
  );
});

test("the band spends two frame rows around the rows it shows", async () => {
  assert.equal(menuRows(null), 0);
  assert.equal(menuRows(0), 0);
  assert.equal(menuRows(3), 5);
  assert.equal(menuRows(6), 2 + 5 + 1, "the overflow row reports the rows the band hides");
});

test("the band paints the rows with the first one cursored", async () => {
  const painted_ = await lines("/s");
  const text = painted_.join("\n");
  assert.match(text, /commands/);
  assert.match(text, /> \/session/);
  assert.match(text, /\/sessions/);
  assert.match(text, /open the session with this id/);
  assert.match(text, /Session/);
});
