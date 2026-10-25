//! The transcript's blocks at their own widths: markers, cards, bounds.

import assert from "node:assert/strict";
import test from "node:test";
import { frameLines, painted } from "../testing/render";
import { run } from "../testing/fixtures";
import { DARK } from "../theme/palette";
import { ContentColumns } from "../kit";
import { Notice } from "./blocks/Notice";
import { Reasoning } from "./blocks/Reasoning";
import { runMetaText } from "./blocks/RunMeta";
import { TOOL_PREVIEW_ROWS, ToolBlock } from "./blocks/ToolBlock";
import { UserCard } from "./blocks/UserCard";

/** Renders one element at one width and returns its lines. */
async function lines(element: React.ReactNode, width: number): Promise<string[]> {
  return frameLines(await painted(element, width, 20));
}

test("a user row opens with its label and prints its message under it", async () => {
  const painted_ = await lines(<UserCard text="hi there" palette={DARK} />, 20);
  assert.deepEqual(painted_, ["you", "hi there"], "the label, then the message, nothing else");
});

test("a user row wraps its message in a narrow column", async () => {
  const painted_ = await lines(<UserCard text="hi there" palette={DARK} />, 4);
  assert.ok(painted_.join("").includes("hi"), "the message is painted");
  assert.ok(!painted_.some((line) => line.includes("╭")), "no frame is drawn around it");
});

test("a notice keeps every row its body wraps into", async () => {
  // The body hangs in the columns the marker leaves. A body that only grew into
  // the row wrapped at the panel's width and put its last row under the row
  // after it, which is the row the next block painted.
  const text = "the daemon stopped answering and the run ended without a reply";
  const frame = await painted(
    <ContentColumns columns={30}>
      <Notice text={text} palette={DARK} />
    </ContentColumns>,
    40,
    12,
  );
  assert.deepEqual(frameLines(frame), [
    "※ the daemon stopped answering",
    "and the run ended without a",
    "reply",
  ]);
});

test("a reasoning block keeps every row under its header", async () => {
  const text = Array.from({ length: 20 }, (_, index) => `line ${index + 1}`).join("\n");
  const painted_ = frameLines(await painted(<Reasoning text={text} palette={DARK} />, 40, 30));
  assert.equal(painted_[0], "∴ reasoning");
  assert.equal(painted_.length, 21);
  assert.ok(painted_.includes("line 20"));
});

test("a finished run closes with its status, its seconds, and its tokens", () => {
  const meta = runMetaText(
    run("completed", {
      usage: {
        state: "reported",
        input_tokens: 12_400,
        output_tokens: 2_100,
        total_tokens: 14_500,
      },
      started_at: 1_726_000_000,
      finished_at: 1_726_000_004,
    }),
  );
  assert.equal(meta, "run complete · 4s · 12.4k in / 2.1k out");
});

test("a run without reported usage closes with what it has", () => {
  assert.equal(runMetaText(run("failed", { finished_at: null })), "run failed");
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
