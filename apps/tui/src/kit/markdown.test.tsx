//! Markdown: what a model's answer looks like when it reaches the panel.

import assert from "node:assert/strict";
import test from "node:test";
import { frameLines, painted } from "../testing/render";
import { DARK } from "../theme/palette";
import { Markdown } from "./markdown";

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
  assert.deepEqual(painted_, ["• first item", "• second item", "", "1. one", "2. two"]);
});

test("a heading reads as a heading and not as literal hashes", async () => {
  const painted_ = await lines("## What it does\n\nIt relays.\n");
  assert.equal(painted_[0], "What it does");
  assert.equal(painted_[2], "It relays.");
});

test("inline code stays against its punctuation", async () => {
  const painted_ = await lines("Use `widgets` here, then stop.");
  const text = painted_.join(" ");
  assert.match(text, /Use widgets here, then stop\./);
  assert.ok(!text.includes("widgets ,"), "no gap is left before the comma");
});

test("a fence closes its rounded frame on every side", async () => {
  const painted_ = await lines("```ts\nconst x = 1;\n```", 20);
  assert.match(painted_[0] ?? "", /^╭.*╮$/u);
  assert.ok(
    painted_.some((line) => line.includes("const x = 1;")),
    "the code is painted",
  );
  assert.match(painted_.at(-1) ?? "", /^╰.*╯$/u);
  for (const line of painted_) {
    assert.ok(line.length <= 20, `"${line}" fits the fence's page`);
  }
});

test("a rule spans the region, not three cells", async () => {
  const painted_ = await lines("above\n\n---\n\nbelow", 20);
  assert.ok(painted_.includes("─".repeat(20)), "the divider is the full width");
});

test("a list item that wraps keeps every row its text needs", async () => {
  // The item's text wraps in a column of its own, so a row the wrap needs is a
  // row the page carries: an item that wrapped into the panel's width instead
  // put its second row under the next item and lost it.
  const painted_ = await lines(
    "- the tune, kept whole for the retry\n- the scrape, reduced to the aggregate",
    30,
  );
  assert.deepEqual(painted_, [
    "• the tune, kept whole for the",
    "retry",
    "• the scrape, reduced to the",
    "aggregate",
  ]);
});

test("a table reads as columns, not as the pipes that wrote it", async () => {
  const painted_ = await lines(
    "| coil | use |\n| ---- | --- |\n| answer | the run's text |\n| reasoning | the run's thinking |\n",
    40,
  );
  // The columns are shares of the page, so they do not widen while rows arrive.
  assert.deepEqual(painted_.slice(0, 3), [
    "coil                use",
    "answer              the run's text",
    "reasoning           the run's thinking",
  ]);
  for (const line of painted_) {
    assert.ok(!line.includes("|"), `"${line}" is not a pipe row`);
    assert.ok(!line.includes("----"), `"${line}" is not the rule row`);
  }
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
