//! The theme picker: three entries, one highlighted, one in force.

import assert from "node:assert/strict";
import test from "node:test";
import { THEME_PREFERENCES } from "../app/action";
import { frameLines, painted } from "../testing/render";
import { DARK } from "../theme/palette";
import { renderHints } from "./format";
import { PICKER_HINTS, THEME_ENTRIES, ThemePicker } from "./ThemePicker";

/** Renders the picker at one panel size and returns its lines. */
async function lines(
  width: number,
  height: number,
  options: { selection?: number; current?: "auto" | "light" | "dark" } = {},
): Promise<string[]> {
  const frame = await painted(
    <ThemePicker
      selection={options.selection ?? 0}
      current={options.current ?? "auto"}
      palette={DARK}
      width={width}
      height={height}
    />,
    width,
    height,
  );
  return frameLines(frame);
}

test("the picker offers every preference and marks the one in force", async () => {
  const painted_ = await lines(60, 12, { current: "dark" });
  assert.equal(painted_[0], "theme");
  assert.match(painted_[1] ?? "", /^› auto/);
  assert.equal(painted_[1]?.includes("current"), false, "the row in force is the third one");
  assert.match(painted_[2] ?? "", /^dark\s+current/);
  assert.equal(painted_.at(-1), renderHints(58, PICKER_HINTS));
});

test("the selection moves the marker and nothing else", async () => {
  const painted_ = await lines(60, 12, { selection: 2, current: "auto" });
  assert.equal(painted_[1]?.startsWith("›"), false, "the first entry carries no marker");
  assert.match(painted_[1] ?? "", /^auto\s+current/);
  assert.match(painted_[3] ?? "", /^› light/);
});

test("a small panel drops the explanations and keeps the entries", async () => {
  const roomy = await lines(60, 12);
  const tight = await lines(34, 8);
  assert.match(roomy[1] ?? "", /follow the terminal background/);
  assert.equal(tight[1]?.includes("follow"), false, "the detail is dropped whole");
  assert.match(tight[1] ?? "", /^› auto\s+current$/);
  assert.equal(tight.at(-1), renderHints(32, PICKER_HINTS));
});

test("the picker lists the preferences in the order the reducer indexes them", async () => {
  assert.deepEqual(
    THEME_ENTRIES.map((entry) => entry.preference),
    [...THEME_PREFERENCES],
    "a view that reordered the entries would commit the neighbour of the choice",
  );
  const painted_ = await lines(60, 12);
  for (const [index, preference] of THEME_PREFERENCES.entries()) {
    assert.match(painted_[index + 1] ?? "", new RegExp(`\\b${preference}\\b`));
  }
});

test("a tall panel centres the entries instead of leaving them hanging off the title", async () => {
  const painted_ = await lines(80, 24);
  const entries = painted_.filter((line) => /^(›\s+)?(auto|dark|light)\b/.test(line));
  assert.equal(entries.length, 3, "every entry is painted");
  const first = painted_.indexOf(entries[0] ?? "");
  assert.ok(first > 4, `the first entry sits below the title (row ${first})`);
  assert.ok(first < painted_.length - 4, "the entries are not pushed to the bottom");
});
