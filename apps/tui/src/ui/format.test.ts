//! Formatting: the hints that fit, the words a status uses, and the columns a
//! narrow table keeps.

import assert from "node:assert/strict";
import test from "node:test";
import type { SessionSummaryDto } from "../bindings";
import {
  basename,
  describeRun,
  fitColumns,
  fitExamples,
  fitHints,
  formatAgo,
  formatDuration,
  formatElapsed,
  HINT_SEGMENTS,
  renderHints,
  SESSION_COLUMNS,
  sessionCells,
  shortId,
} from "./format";

test("a hint band drops whole segments and never cuts one mid-word", () => {
  assert.equal(
    renderHints(100),
    "enter send · shift+enter new line · ↑↓ history · / commands · esc stop · ctrl+d quit",
    "every segment shows where the band is wide enough",
  );
  assert.equal(
    renderHints(80),
    "enter send · shift+enter new line · ↑↓ history · / commands · esc stop",
  );
  for (const width of [40, 34, 24, 16]) {
    const rendered = renderHints(width);
    assert.ok(rendered.length <= width, `${width}: "${rendered}" fits`);
    for (const segment of fitHints(width)) {
      assert.ok(
        rendered.includes(`${segment.keys} ${segment.action}`),
        `${width}: every kept segment is whole`,
      );
    }
    assert.ok(rendered.startsWith("enter send"), `${width}: the first key always shows`);
  }
});

test("a narrow band keeps as many whole segments as fit", () => {
  const at34 = fitHints(34);
  assert.deepEqual(
    at34.map((segment) => segment.keys),
    ["enter", "shift+enter"],
    "34 columns hold two segments and no fragment of a third",
  );
  assert.deepEqual(
    fitHints(20).map((segment) => segment.keys),
    ["enter"],
  );
  assert.deepEqual(
    fitHints(1).map((segment) => segment.keys),
    ["enter"],
    "the first segment is kept even when it overflows",
  );
  assert.equal(HINT_SEGMENTS.length, 6);
});

test("the status line speaks the user's words, not the protocol's", () => {
  assert.equal(describeRun("starting", false), "starting");
  assert.equal(describeRun("running", false), "thinking");
  assert.equal(describeRun("running", true), "answering");
  assert.equal(describeRun("completed", false), "done");
  assert.equal(describeRun("failed", false), "failed");
  assert.equal(describeRun("interrupted", false), "stopped");
  for (const status of ["starting", "running", "completed", "failed", "interrupted"] as const) {
    assert.doesNotMatch(describeRun(status, false), /run|status/i, "no enum vocabulary leaks");
  }
});

test("elapsed times read at the coarsest useful unit", () => {
  assert.equal(formatElapsed(null, 100), null);
  assert.equal(formatElapsed(100, 112), "12s");
  assert.equal(formatDuration(65), "1m 05s");
  assert.equal(formatDuration(3_700), "1h 01m");
  assert.equal(formatAgo(1_000, 1_030), "now");
  assert.equal(formatAgo(1_000, 1_600), "10m");
  assert.equal(formatAgo(1_000, 10_000), "2h");
  assert.equal(formatAgo(0, 200_000), "2d");
});

test("an identity is read by its last eight characters", () => {
  assert.equal(shortId("0b1a2c3d-0000-4000-8000-0000000000a1"), "000000a1");
  assert.equal(shortId("short"), "short");
});

test("a narrow table drops a column whole instead of squeezing every one", () => {
  assert.deepEqual(
    fitColumns(100).map((column) => column.header),
    ["session", "updated", "workspace", "rows"],
  );
  assert.deepEqual(
    fitColumns(40).map((column) => column.header),
    ["session", "updated", "workspace"],
    "40 columns drop the row count first",
  );
  assert.deepEqual(
    fitColumns(30).map((column) => column.header),
    ["session", "updated"],
  );
  assert.deepEqual(
    fitColumns(10).map((column) => column.header),
    ["session"],
    "the identity column is never dropped",
  );
});

test("a session row fills the columns the table kept", () => {
  const summary: SessionSummaryDto = {
    session_id: "0b1a2c3d-0000-4000-8000-0000000000a1",
    project_id: "p",
    workspace_id: "w",
    mode: "build",
    created_at: 1_000,
    updated_at: 1_000,
    workspace_root: "/home/user/projects/intention-relay",
    message_count: 42,
  };
  assert.deepEqual(sessionCells(summary, SESSION_COLUMNS, 1_060), [
    "000000a1",
    "1m",
    "intention-relay",
    "42",
  ]);
  assert.equal(basename("/home/user/"), "user");
  assert.equal(basename("C:\\Users\\user"), "user");
});

test("a welcome example is shown whole or not at all", () => {
  const examples = ["explain this repository", "find the failing test", "add a retry"];
  assert.deepEqual(fitExamples(80, examples), examples);
  assert.deepEqual(fitExamples(22, examples), ["find the failing test", "add a retry"]);
  assert.deepEqual(fitExamples(5, examples), []);
});
