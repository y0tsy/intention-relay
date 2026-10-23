//! The browser's model: its tabs, its filter, and the age columns.

import assert from "node:assert/strict";
import test from "node:test";
import { summary } from "../testing/fixtures";
import { browserColumns, browserRows, relativeTime } from "./browser";

test("the folder tab keeps the sessions of the workspace this front end runs in", () => {
  const sessions = [
    summary({ session_id: "a", workspace_root: "/home/user/project" }),
    summary({ session_id: "b", workspace_root: "/home/user/other" }),
  ];
  assert.deepEqual(
    browserRows(sessions, "folder", "", "/home/user/project").map((row) => row.session_id),
    ["a"],
  );
  assert.equal(browserRows(sessions, "all", "", "/home/user/project").length, 2);
});

test("a filter matches the identity and the path, without regard to case", () => {
  const sessions = [
    summary({ session_id: "0b1a2c3d", workspace_root: "/home/user/Project" }),
    summary({ session_id: "ffee", workspace_root: "/home/user/other" }),
  ];
  assert.deepEqual(
    browserRows(sessions, "all", "PROJECT", "/home/user/Project").map((row) => row.session_id),
    ["0b1a2c3d"],
  );
  assert.deepEqual(
    browserRows(sessions, "all", "ffee", "/home/user/Project").map((row) => row.session_id),
    ["ffee"],
  );
});

test("an age reads as the browser's columns show it", () => {
  const now = 1_700_000_000;
  assert.equal(relativeTime(now - 10, now), "now");
  assert.equal(relativeTime(now - 120, now), "2m ago");
  assert.equal(relativeTime(now - 7_200, now), "2h ago");
  assert.equal(relativeTime(now - 172_800, now), "2d ago");
  assert.match(relativeTime(now - 1_000_000, now), /^[A-Z][a-z]{2} \d+$/);
});

test("the title column takes what the purpose columns leave", () => {
  const columns = browserColumns(74);
  assert.ok(columns.modified >= 8 && columns.created >= 7 && columns.size >= 4);
  assert.ok(columns.title > 0);
  assert.equal(browserColumns(24).title >= 0, true, "a narrow panel never goes negative");
});
