//! The welcome composition: what a panel of a given size can hold.

import assert from "node:assert/strict";
import test from "node:test";
import { EXAMPLES, planRows, planWelcome } from "./welcome-plan";

/** Counts the rows one plan renders, blocks and the gaps between them. */
function renderedRows(plan: ReturnType<typeof planWelcome>, textWidth: number): number {
  const blocks: number[] = [1];
  if (plan.rule) {
    blocks.push(1);
  }
  if (plan.tagline !== null) {
    blocks.push(Math.max(1, Math.ceil(plan.tagline.length / textWidth)));
  }
  if (plan.examples.length > 0) {
    blocks.push(plan.examples.length);
  }
  if (plan.commands === "list") {
    blocks.push(5);
  } else if (plan.commands === "hint") {
    blocks.push(1);
  }
  if (plan.workspace) {
    blocks.push(1);
  }
  return blocks.reduce((total, rows) => total + rows, 0) + Math.max(0, blocks.length - 1);
}

test("a tall wide panel gets the full block", () => {
  const plan = planWelcome({ width: 98, height: 24, workspace: "/workspace" });
  assert.equal(plan.examples.length, EXAMPLES.length);
  assert.equal(plan.commands, "list");
  assert.equal(plan.workspace, true);
  assert.equal(plan.rule, true);
});

test("a panel that cannot hold the full block keeps the examples", () => {
  const plan = planWelcome({ width: 58, height: 11, workspace: "/workspace" });
  assert.ok(plan.examples.length > 0, "the examples outrank the command list");
  assert.equal(plan.commands, "hint");
});

test("a small panel keeps as much as it can and no more", () => {
  for (const height of [5, 6, 7, 8, 9, 10, 12, 16, 24]) {
    for (const width of [34, 40, 58, 98]) {
      const plan = planWelcome({ width, height, workspace: "/home/user/project" });
      const rows = renderedRows(plan, Math.max(8, Math.min(width - 4, 78)));
      assert.ok(rows <= height, `${width}x${height}: ${rows} rows planned for ${height}`);
    }
  }
});

test("a tiny panel still shows the lockup", () => {
  const plan = planWelcome({ width: 30, height: 1, workspace: "/workspace" });
  assert.equal(plan.tagline, null);
  assert.deepEqual(plan.examples, []);
  assert.equal(plan.commands, "none");
  assert.equal(plan.workspace, false);
});

test("an example that does not fit the width is dropped whole", () => {
  const plan = planWelcome({ width: 34, height: 12, workspace: "/workspace" });
  for (const example of plan.examples) {
    assert.ok(EXAMPLES.includes(example), "no example is shortened");
  }
});

test("the rows a plan needs are the rows it renders", () => {
  for (const room of [
    { width: 98, height: 24, workspace: "/workspace" },
    { width: 58, height: 11, workspace: "/home/user/project" },
    { width: 34, height: 5, workspace: "/workspace" },
  ]) {
    const plan = planWelcome(room);
    const textWidth = Math.max(8, Math.min(room.width - 4, 78));
    const rows = planRows(plan, textWidth, `new sessions run in ${room.workspace}`);
    assert.equal(renderedRows(plan, textWidth), rows, `${room.width}x${room.height}`);
  }
});
