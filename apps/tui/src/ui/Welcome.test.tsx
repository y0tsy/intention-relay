//! The welcome surface: the lockup over the version, in the region it was given.

import assert from "node:assert/strict";
import test from "node:test";
import { frameLines, painted } from "../testing/render";
import { DARK } from "../theme/palette";
import { Welcome } from "./Welcome";

/** Renders the welcome pane at one region size and returns its lines. */
async function lines(columns: number, rows: number): Promise<string[]> {
  return frameLines(await painted(<Welcome rows={rows} palette={DARK} />, columns, rows));
}

test("the lockup centres in the region it was given", async () => {
  const frame = await painted(<Welcome rows={12} palette={DARK} />, 40, 12);
  const painted_ = frame.split("\n");
  assert.equal(painted_[1]?.trim(), "INTENTION  R E L A Y", "the pair is whole");
  assert.equal(painted_[1]?.indexOf("I"), 10, "and centred in the region");
  assert.match(painted_[2] ?? "", /^\s*$/);
});

test("the overview is pinned to the last rows and names the version", async () => {
  const painted_ = await lines(40, 12);
  const text = painted_.join("\n");
  assert.match(text, /Version 0\.0\.0/);
  assert.match(painted_.at(-1) ?? "", /╰/, "the overview's bottom edge is the last row");
});

test("a region too short for the lockup keeps the overview", async () => {
  const painted_ = await lines(40, 3);
  assert.equal(painted_.length, 3);
  assert.match(painted_.join("\n"), /Version 0\.0\.0/);
  assert.doesNotMatch(painted_.join("\n"), /INTENTION/);
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
