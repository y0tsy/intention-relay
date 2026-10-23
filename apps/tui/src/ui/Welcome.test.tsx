//! The welcome surface: the composition a panel of a given size can hold.

import assert from "node:assert/strict";
import test from "node:test";
import { frameLines, painted } from "../testing/render";
import { DARK } from "../theme/palette";
import { Welcome } from "./Welcome";
import { EXAMPLES, planWelcome } from "./welcome-plan";

/**
 * Renders the welcome block at one terminal size.
 *
 * The block is given the rows the frame leaves it: the border takes two and the
 * hint band, the composer, and the status row take one each.
 */
async function lines(
  terminalWidth: number,
  terminalHeight: number,
  workspace = "/home/user/project",
): Promise<string[]> {
  const width = terminalWidth - 2;
  const height = terminalHeight - 5;
  const frame = await painted(
    <Welcome width={width} height={height} workspace={workspace} palette={DARK} />,
    width,
    height,
  );
  return frameLines(frame);
}

test("a welcome at forty by twelve fills its room with the lockup and the examples", async () => {
  const painted_ = await lines(40, 12);
  assert.equal(painted_[0], "intention relay");
  const body = painted_.slice(1).filter((line) => line.length > 0);
  assert.ok(body.length > 0, "the block paints something under the lockup");
  for (const line of body) {
    assert.ok(EXAMPLES.includes(line.slice(2)) || line.startsWith("/"), `"${line}" is whole`);
  }
});

test("a welcome at thirty-four by ten keeps the lockup and drops the rest", async () => {
  const painted_ = await lines(34, 10);
  assert.equal(painted_[0], "intention relay");
  assert.ok(painted_.length <= 5, "the block never asks for more rows than it has");
  const plan = planWelcome({ width: 32, height: 5, workspace: "/home/user/project" });
  assert.equal(plan.tagline, null, "a tagline that would not fit is given up whole");
  assert.equal(plan.workspace, false);
});

test("a tall panel shows the command reference and where sessions run", async () => {
  const painted_ = await lines(100, 30);
  const text = painted_.join("\n");
  assert.match(text, /intention relay/);
  assert.match(text, /\/new\s+start a fresh session/);
  assert.match(text, /new sessions run in \/home\/user\/project/);
});

test("no panel size paints a line longer than the block", async () => {
  for (const [width, height] of [
    [34, 10],
    [40, 12],
    [60, 16],
    [100, 30],
    [200, 60],
  ] as const) {
    for (const line of await lines(width, height)) {
      assert.ok(line.length <= width - 4, `${width}x${height}: "${line}" fits`);
    }
  }
});

test("every example the block keeps is whole", async () => {
  for (const [width, height] of [
    [40, 12],
    [100, 30],
  ] as const) {
    for (const line of await lines(width, height)) {
      if (line.startsWith("- ")) {
        assert.ok(EXAMPLES.includes(line.slice(2)), `"${line}" is one whole example`);
      }
    }
  }
});
