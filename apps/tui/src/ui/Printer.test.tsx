//! The printer's bookkeeping: the answer a streamed step became is not printed
//! a second time.

import assert from "node:assert/strict";
import test from "node:test";
import { transcriptBlocks, withRunMeta } from "../app/transcript";
import { row, run } from "../testing/fixtures";
import { adoptedAnswer } from "./Printer";

/** Nothing adopted yet. */
const NOTHING: ReadonlySet<string> = new Set();

test("the block a streamed step became is the block the printer already has", () => {
  const entries = transcriptBlocks([row(1, "assistant", "an answer")]);
  assert.equal(adoptedAnswer(entries, NOTHING, { reasoning: "", text: "an answer" }), "1");
});

test("trailing white space does not make it another block", () => {
  const entries = transcriptBlocks([row(1, "assistant", "an answer\n")]);
  assert.equal(adoptedAnswer(entries, NOTHING, { reasoning: "", text: "an answer" }), "1");
});

test("a block that carries another text is not the stream's", () => {
  const entries = transcriptBlocks([row(1, "assistant", "another answer")]);
  assert.equal(adoptedAnswer(entries, NOTHING, { reasoning: "", text: "an answer" }), null);
});

test("a reasoning-only stream adopts the row that carries its reasoning", () => {
  const entries = transcriptBlocks([
    row(1, "assistant", "", { reasoning: "a thought the step streamed" }),
  ]);
  assert.equal(
    adoptedAnswer(entries, NOTHING, { reasoning: "a thought the step streamed", text: "" }),
    "1",
  );
});

test("a block already behind the printer is not adopted again", () => {
  const entries = transcriptBlocks([row(1, "assistant", "an answer")]);
  assert.equal(adoptedAnswer(entries, new Set(["1"]), { reasoning: "", text: "an answer" }), null);
});

test("a run's closing row is never an answer", () => {
  const entries = withRunMeta(transcriptBlocks([row(1, "user", "ask")]), run("completed"));
  assert.equal(adoptedAnswer(entries, NOTHING, { reasoning: "", text: "ask" }), null);
});
