//! The transcript's blocks at their own widths: markers, cards, bounds.

import assert from "node:assert/strict";
import test from "node:test";
import { frameLines, painted } from "../testing/render";
import { DARK } from "../theme/palette";
import { REASONING_VISIBLE_ROWS, Reasoning } from "./blocks/Reasoning";
import { TOOL_PREVIEW_ROWS, ToolBlock } from "./blocks/ToolBlock";
import { UserCard } from "./blocks/UserCard";

/** Renders one element at one width and returns its lines. */
async function lines(element: React.ReactNode, width: number): Promise<string[]> {
  return frameLines(await painted(element, width, 20));
}

test("a card frames its message on the user wash", async () => {
  const painted_ = await lines(<UserCard text="hi there" palette={DARK} />, 20);
  assert.equal(painted_.length, 3, "a three-row card: its top border, its line, its bottom");
  assert.match(painted_[0] ?? "", /^╭─❯ you─+╮$/);
  assert.match(painted_[1] ?? "", /^│ hi there +│$/);
  assert.match(painted_[2] ?? "", /^╰─+╯$/);
});

test("a card too narrow for a frame falls back to its rows", async () => {
  const painted_ = await lines(<UserCard text="hi there" palette={DARK} />, 4);
  assert.ok(painted_.join("").includes("hi"), "the message is painted");
  assert.ok(!painted_.some((line) => line.includes("╭")), "no frame is drawn around it");
});

test("a reasoning block shows fifteen rows and the rows it counted", async () => {
  const text = Array.from({ length: 20 }, (_, index) => `line ${index + 1}`).join("\n");
  const painted_ = await lines(<Reasoning text={text} palette={DARK} expansion={0} />, 40);
  assert.equal(painted_.length, 1 + REASONING_VISIBLE_ROWS + 1 + 1);
  assert.ok(painted_.some((line) => /… 5 more · click or Ctrl\+E/.test(line)));
});

test("a tool block previews twelve rows and names the rows it hid", async () => {
  const text = Array.from({ length: 20 }, (_, index) => `hit ${index + 1}`).join("\n");
  const painted_ = await lines(
    <ToolBlock
      call={null}
      result={{
        id: 4,
        session_id: "s",
        run_id: null,
        kind: "tool_result",
        text,
        reasoning: null,
        tool: {
          row: "result",
          value: {
            call_id: "c1",
            tool_id: "grep",
            status: "completed",
            result: null,
            metadata: [],
          },
        },
      }}
      palette={DARK}
    />,
    40,
  );
  assert.ok(painted_[0]?.includes("▸ grep"));
  assert.equal(painted_.filter((line) => line.includes("‣ ")).length, TOOL_PREVIEW_ROWS);
  assert.ok(painted_.some((line) => /… 8 more rows/.test(line)));
});
