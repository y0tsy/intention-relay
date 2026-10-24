//! The blocks a transcript draws: one per committed row, calls paired to results.

import assert from "node:assert/strict";
import test from "node:test";
import { row } from "../testing/fixtures";
import { callId, transcriptBlocks } from "./transcript";

/** A tool row carrying one call identity. */
function call(id: number, callId: string, tool = "grep") {
  return {
    ...row(id, "tool_call", ""),
    tool: { row: "call", value: { call_id: callId, tool_id: tool, arguments: null } },
  } as never;
}

/** A result row answering one call. */
function result(id: number, callId: string, text: string) {
  return {
    ...row(id, "tool_result", text),
    tool: {
      row: "result",
      value: { call_id: callId, tool_id: "grep", status: "completed", result: null, metadata: [] },
    },
  } as never;
}

test("a call owns the result that answers it and the result draws no block", () => {
  const blocks = transcriptBlocks([row(1, "user", "hi"), call(3, "c1"), result(4, "c1", "hit")]);
  assert.deepEqual(
    blocks.map((block) => block.key),
    ["1", "3"],
    "the result row is folded into its call",
  );
  assert.equal(blocks[1]?.call?.id, 3);
  assert.equal(blocks[1]?.result?.id, 4);
});

test("a result without its call stays a block of its own", () => {
  const blocks = transcriptBlocks([result(4, "c2", "no call here")]);
  assert.equal(blocks.length, 1);
  assert.equal(blocks[0]?.call, null);
  assert.equal(blocks[0]?.result, null, "the block draws the message as it stands");
  assert.equal(blocks[0]?.message?.id, 4);
});

test("a call with no result keeps a null result", () => {
  const blocks = transcriptBlocks([call(3, "c1")]);
  assert.equal(blocks[0]?.call?.id, 3);
  assert.equal(blocks[0]?.result, null);
});

test("a row without a tool carries no call identity", () => {
  assert.equal(callId(row(1, "user", "hi")), null);
  assert.equal(callId(call(2, "c9")), "c9");
});
