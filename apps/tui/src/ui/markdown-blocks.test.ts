//! Splitting a streamed answer where it is still being written.

import assert from "node:assert/strict";
import test from "node:test";
import { splitBlocks } from "./markdown-blocks";

test("a settled block leaves what follows the last blank line open", () => {
  assert.deepEqual(splitBlocks("one\n\ntwo"), { closed: "one\n\n", open: "two" });
});

test("an answer that ends on a blank line has nothing open", () => {
  assert.deepEqual(splitBlocks("one\n\n"), { closed: "one\n\n", open: "" });
});

test("an answer with no blank line is one open block", () => {
  assert.deepEqual(splitBlocks("one\ntwo"), { closed: "", open: "one\ntwo" });
});

test("an open fence keeps its own lines together", () => {
  const source = "before\n\n```ts\nconst a = 1;\n";
  assert.deepEqual(splitBlocks(source), { closed: "before\n\n", open: "```ts\nconst a = 1;\n" });
});

test("a closed fence settles like any other block", () => {
  const source = "```ts\nconst a = 1;\n```\n\nafter";
  assert.deepEqual(splitBlocks(source), { closed: "```ts\nconst a = 1;\n```\n\n", open: "after" });
});

test("a blank line inside a fence settles nothing", () => {
  const source = "```\n\ncode\n```\n\nafter";
  assert.deepEqual(splitBlocks(source), { closed: "```\n\ncode\n```\n\n", open: "after" });
});

test("an empty answer has nothing open", () => {
  assert.deepEqual(splitBlocks(""), { closed: "", open: "" });
});
