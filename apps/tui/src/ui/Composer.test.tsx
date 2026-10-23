//! The composer: the prompt line, its caret, and its placeholder.

import assert from "node:assert/strict";
import test from "node:test";
import { painted, frameLines } from "../testing/render";
import { DARK } from "../theme/palette";
import { Composer } from "./Composer";
import { EMPTY_DRAFT } from "./editor";

/** Returns the line the composer paints at one width. */
async function line(draft: { text: string; caret: number }, width = 40): Promise<string> {
  const frame = await painted(<Composer draft={draft} palette={DARK} focused />, width, 1);
  return frameLines(frame)[0] ?? "";
}

test("an empty draft shows the placeholder with the caret", async () => {
  assert.equal(await line(EMPTY_DRAFT), "> ▏type a prompt");
});

test("a draft shows its text with the caret where the next character goes", async () => {
  assert.equal(await line({ text: "hello", caret: 5 }), "> hello▏");
  assert.equal(await line({ text: "hello", caret: 0 }), "> ▏hello");
  assert.equal(await line({ text: "hello", caret: 2 }), "> he▏llo");
});

test("a draft with a newline keeps both lines", async () => {
  const frame = await painted(
    <Composer draft={{ text: "one\ntwo", caret: 7 }} palette={DARK} focused />,
    40,
    2,
  );
  assert.deepEqual(frameLines(frame), ["> one", "two▏"]);
});
