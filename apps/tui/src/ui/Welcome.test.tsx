//! The welcome page: the open Atrium surface and its responsive commands.

import assert from "node:assert/strict";
import test from "node:test";
import { frameLines, painted } from "../testing/render";
import { DARK } from "../theme/palette";
import { Slot, SlotRows } from "../kit";
import { Welcome } from "./Welcome";

/** Renders the welcome page at one region size and returns its lines. */
async function lines(columns: number, rows: number): Promise<string[]> {
  return frameLines(
    await painted(
      <SlotRows rows={rows}>
        <Slot>
          <Welcome palette={DARK} />
        </Slot>
      </SlotRows>,
      columns,
      rows,
    ),
  );
}

test("the lockup stacks the fullwidth words and stars RELAY", async () => {
  const frame = await painted(
    <SlotRows rows={12}>
      <Slot>
        <Welcome palette={DARK} />
      </Slot>
    </SlotRows>,
    40,
    12,
  );
  const painted_ = frame.split("\n");
  const top = painted_[1] ?? "";
  const bottom = painted_[2] ?? "";
  assert.equal(top.trim(), "Ｉ Ｎ Ｔ Ｅ Ｎ Ｔ Ｉ Ｏ Ｎ", "the product word is whole");
  assert.equal(bottom.trim(), "✦ Ｒ Ｅ Ｌ Ａ Ｙ ✦", "the trailing word carries its stars");
  assert.equal(
    top.length - top.trimStart().length,
    top.length - top.trimEnd().length,
    "the product word is centred",
  );
  assert.equal(
    bottom.length - bottom.trimStart().length,
    bottom.length - bottom.trimEnd().length,
    "the trailing word is centred",
  );
  assert.equal(painted_[3]?.trim(), "A local agent for the long run.", "the promise follows");
});

test("a narrow page keeps the command row and the short note", async () => {
  const text = (await lines(40, 12)).join("\n");
  assert.match(text, /A local agent for the long run\./);
  assert.match(text, /local-first · single-user · no cloud/);
  assert.match(text, /\/new · \/sessions · \/theme/);
  assert.match(text, /No session yet\./);
});

test("a wide page offers three command cards", async () => {
  const painted_ = await lines(140, 30);
  const text = painted_.join("\n");
  assert.ok(painted_.join("").split("╭").length - 1 >= 3, "each command has its own card");
  assert.match(text, /\/new/);
  assert.match(text, /start a fresh session/);
  assert.match(text, /01/);
  assert.match(text, /\/sessions/);
  assert.match(text, /browse this machine/);
  assert.match(text, /\/theme/);
  assert.match(text, /choose the palette/);
  assert.match(text, /03/);
  assert.match(text, /No session yet\. Type a prompt, or pick a command\./);
});

test("a region too short for the lockup keeps the orientation", async () => {
  const painted_ = await lines(40, 3);
  assert.match(painted_.join("\n"), /A local agent for the long run\./);
  assert.doesNotMatch(painted_.join("\n"), /INTENTION/);
});

test("a welcome with room names the commands and the empty state", async () => {
  const text = (await lines(80, 16)).join("\n");
  assert.match(text, /A local agent for the long run\./);
  assert.match(text, /\/new start · \/sessions browse · \/theme palette/);
  assert.match(text, /No session yet\. Type a prompt, or pick a command\./);
});

test("a region with no rows draws nothing", async () => {
  assert.deepEqual(await lines(40, 0), []);
});

test("no line is wider than the region", async () => {
  for (const [columns, rows] of [
    [34, 10],
    [40, 12],
    [60, 16],
    [100, 30],
    [200, 60],
  ] as const) {
    for (const line of await lines(columns, rows)) {
      assert.ok(line.length <= columns, `${columns}x${rows}: "${line}" fits`);
    }
  }
});
