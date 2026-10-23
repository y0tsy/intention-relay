//! The transcript: committed rows, then the text of the step in flight.

import assert from "node:assert/strict";
import test from "node:test";
import { frameLines, painted } from "../testing/render";
import { row } from "../testing/fixtures";
import { DARK } from "../theme/palette";
import { Transcript, toolLabel } from "./Transcript";

/** Renders a transcript of committed rows at one width. */
async function lines(
  messages: Parameters<typeof Transcript>[0]["messages"],
  options: { provisionalReasoning?: string; provisionalText?: string; height?: number } = {},
): Promise<string[]> {
  const frame = await painted(
    <Transcript
      messages={messages}
      provisionalReasoning={options.provisionalReasoning ?? ""}
      provisionalText={options.provisionalText ?? ""}
      palette={DARK}
    />,
    60,
    options.height ?? 20,
  );
  return frameLines(frame);
}

test("a prompt row reads as the prompt and an answer row as the answer", async () => {
  const painted_ = await lines([
    row(1, "user", "explain this repository"),
    row(2, "assistant", "It relays intentions."),
  ]);
  assert.equal(painted_[0], "> explain this repository");
  assert.equal(painted_[2], "It relays intentions.");
});

test("a tool row names the tool the daemon reported", async () => {
  const call = row(3, "tool_call", "src/main.rs", {
    tool: { row: "call", value: { tool_id: "read", input: {} } } as never,
  });
  const result = row(4, "tool_result", "done", {
    tool: {
      row: "result",
      value: { tool_id: "mystery", status: "completed", result: {} },
    } as never,
  });
  const painted_ = await lines([call, result]);
  assert.equal(painted_[0], "read src/main.rs");
  assert.equal(painted_[2], "result done");
});

test("a tool row with no identity falls back to its own text", async () => {
  const painted_ = await lines([row(5, "tool_result", "no identity here")]);
  assert.equal(painted_[0], "no identity here");
});

test("the step in flight follows the committed rows", async () => {
  const painted_ = await lines([row(1, "user", "hello")], {
    provisionalReasoning: "weighing it",
    provisionalText: "an answer forming",
  });
  assert.equal(painted_[0], "> hello");
  assert.equal(painted_[2], "weighing it");
  assert.equal(painted_[4], "an answer forming");
});

test("a notice row reads as prose, not as a protocol row", async () => {
  const painted_ = await lines([row(6, "notice", "the run was interrupted")]);
  assert.equal(painted_[0], "the run was interrupted");
});

test("the tool label is the wire name, whatever the model called", () => {
  // The identity travels as one string, so a name the product does not expose
  // reaches the reader unchanged instead of failing a lookup.
  assert.equal(
    toolLabel({ row: "call", value: { tool_id: "execute", input: {} } } as never),
    "execute",
  );
  assert.equal(
    toolLabel({ row: "call", value: { tool_id: "mystery", input: {} } } as never),
    "mystery",
  );
  assert.equal(
    toolLabel({
      row: "result",
      value: { tool_id: "grep", status: "completed", result: {} },
    } as never),
    "result",
  );
});
