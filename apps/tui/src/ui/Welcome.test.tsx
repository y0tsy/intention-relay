//! The welcome surface: the lockup over the version, in the region it was given.

import assert from "node:assert/strict";
import test from "node:test";
import { frameLines, painted } from "../testing/render";
import { DARK } from "../theme/palette";
import { SlotRows } from "../kit";
import { Welcome } from "./Welcome";

/** Renders the welcome pane at one region size and returns its lines. */
async function lines(columns: number, rows: number): Promise<string[]> {
  return frameLines(
    await painted(
      <SlotRows rows={rows}>
        <Welcome palette={DARK} />
      </SlotRows>,
      columns,
      rows,
    ),
  );
}

test("the lockup stacks the fullwidth words and stars RELAY", async () => {
  const frame = await painted(
    <SlotRows rows={12}>
      <Welcome palette={DARK} />
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
  assert.match(painted_[3] ?? "", /^\s*$/, "the overview follows the wordmark");
});

test("the overview is pinned to the last rows and names the version", async () => {
  const painted_ = await lines(40, 12);
  const text = painted_.join("\n");
  assert.match(text, /Version 0\.0\.0/);
  assert.match(painted_.at(-1) ?? "", /╰/, "the overview's bottom edge is the last row");
});

test("a region too short for the lockup keeps the orientation", async () => {
  const painted_ = await lines(40, 3);
  assert.equal(painted_.length, 3);
  assert.match(painted_.join("\n"), /A local agent for the long run\./);
  assert.doesNotMatch(painted_.join("\n"), /INTENTION/);
});

test("a welcome with room for the overview names the commands and the version", async () => {
  const text = (await lines(80, 16)).join("\n");
  assert.match(text, /A local agent for the long run\./);
  assert.match(text, /\/new start · \/sessions browse · \/theme palette/);
  assert.match(text, /Version 0\.0\.0/);
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
