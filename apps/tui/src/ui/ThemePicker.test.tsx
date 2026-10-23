//! The theme picker: the panel `/theme` opens above the input block.

import assert from "node:assert/strict";
import test from "node:test";
import { frameLines, painted } from "../testing/render";
import { DARK } from "../theme/palette";
import { PICKER_ROWS, ThemePicker } from "./ThemePicker";

/** Renders the picker at one width and returns its lines. */
async function lines(
  selection: number,
  current: "auto" | "dark" | "light" = "dark",
): Promise<string[]> {
  return frameLines(
    await painted(
      <box flexDirection="column" height={PICKER_ROWS}>
        <ThemePicker selection={selection} current={current} palette={DARK} />
      </box>,
      60,
      PICKER_ROWS,
    ),
  );
}

test("the picker is fixed at six rows: the frame, the three names, the legend", async () => {
  assert.equal(PICKER_ROWS, 6);
  assert.equal((await lines(1)).length, 6);
});

test("the picker names every preference and cursors the highlighted one", async () => {
  const painted_ = await lines(2);
  const text = painted_.join("\n");
  assert.match(text, /auto/);
  assert.match(text, /dark/);
  assert.match(text, /> light/);
  assert.match(text, /Theme/);
  assert.match(text, /enter apply \| esc revert/);
});

test("the picker marks the preference in force", async () => {
  const painted_ = await lines(1, "light");
  assert.match(painted_.join("\n"), /for a light terminal\s+current/);
});
