//! Markdown: what a model's answer looks like when it reaches the panel.

import assert from "node:assert/strict";
import test from "node:test";
import { frameLines, painted } from "../testing/render";
import { DARK } from "../theme/palette";
import { Markdown } from "./Markdown";

/** Renders one document at one width and returns its lines. */
async function lines(source: string, width = 40): Promise<string[]> {
  const frame = await painted(<Markdown source={source} palette={DARK} />, width, 30);
  return frameLines(frame);
}

test("a paragraph wraps by word and never past the panel", async () => {
  const painted_ = await lines(
    "One session, one transcript. The reply streams into this panel while the run is live.",
    40,
  );
  assert.ok(painted_.length > 1, "the paragraph wraps");
  for (const line of painted_) {
    assert.ok(line.length <= 40, `"${line}" fits`);
    assert.ok(!line.startsWith(" "), `"${line}" starts on a word`);
  }
});

test("a list keeps its markers and its items whole", async () => {
  const painted_ = await lines("- first item\n- second item\n\n1. one\n2. two\n");
  assert.deepEqual(painted_, ["- first item", "- second item", "", "1. one", "2. two"]);
});

test("a heading reads as a heading and not as literal hashes", async () => {
  const painted_ = await lines("## What it does\n\nIt relays.\n");
  assert.equal(painted_[0], "What it does");
  assert.equal(painted_[2], "It relays.");
});

test("a fenced block keeps its own lines", async () => {
  const painted_ = await lines("```\nmake quick\nmake verify\n```\n");
  const text = painted_.join("\n");
  assert.match(text, /make quick/);
  assert.match(text, /make verify/);
  assert.ok(
    painted_.some((line) => line.includes("│")),
    "the block is marked as code",
  );
});

test("a block quote is marked as quoted", async () => {
  const painted_ = await lines("> the run was interrupted\n");
  assert.ok(painted_.some((line) => line.includes("the run was interrupted")));
});

test("emphasis does not leak its own punctuation into the text", async () => {
  const painted_ = await lines("This is **bold** and *italic* text.");
  const text = painted_.join(" ");
  assert.match(text, /This is bold and italic text\./);
});

test("a long unbroken word is not cut mid-word", async () => {
  const painted_ = await lines("see https://example.invalid/a/very/long/path/that/keeps/going", 30);
  for (const line of painted_) {
    assert.ok(line.length <= 30, `"${line}" fits`);
  }
});
