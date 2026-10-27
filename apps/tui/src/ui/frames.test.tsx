//! The frames a reader sees: the tray's U and the page's docked margins.

import assert from "node:assert/strict";
import test from "node:test";
import { createElement } from "react";
import { row, run } from "../testing/fixtures";
import { mountApp } from "../testing/render";
import { offlineStore, openSession } from "../testing/store";
import { App } from "./App";

/** Mounts the app, opens a completed session, and returns the footer's rows. */
async function sessionFrame(width: number, height: number): Promise<string[]> {
  const store = offlineStore("dark");
  const setup = await mountApp(createElement(App, { store }), width, height);
  openSession(store, {
    messages: [
      row(1, "user", "a prompt"),
      row(2, "assistant", "an answer that leaves the tray under the page."),
    ],
    run: run("completed"),
  });
  await setup.flush();
  await setup.renderOnce();
  const lines = setup.captureCharFrame().split("\n");
  store.dispose();
  await setup.dispose();
  return lines;
}

/** Mounts the app, opens `/sessions`, and returns the page's rows. */
async function sessionsFrame(width: number, height: number): Promise<string[]> {
  const store = offlineStore("dark");
  const setup = await mountApp(createElement(App, { store }), width, height);
  await setup.mockInput.typeText("/sessions");
  setup.mockInput.pressEnter();
  await setup.flush();
  await setup.renderOnce();
  const lines = setup.captureCharFrame().split("\n");
  store.dispose();
  await setup.dispose();
  return lines;
}

/** Returns the outer tray's bottom row, or -1 when no tray was painted. */
function trayBottom(lines: readonly string[]): number {
  for (let y = lines.length - 1; y >= 0; y -= 1) {
    const line = lines[y] ?? "";
    if (line.includes("╰") && line.trimEnd().endsWith("╯")) {
      return y;
    }
  }
  return -1;
}

test("the tray's U closes on both sides at wide and narrow windows", async () => {
  for (const [width, height] of [
    [190, 44],
    [80, 24],
    [46, 14],
  ] as const) {
    const lines = await sessionFrame(width, height);
    const bottom = trayBottom(lines);
    assert.ok(bottom > 0, `${width}x${height}: the tray is painted`);
    const line = lines[bottom] ?? "";
    const left = line.indexOf("╰");
    const right = line.lastIndexOf("╯");
    assert.ok(left >= 0 && right > left, `${width}x${height}: the tray has both bottom corners`);
    let top = bottom - 1;
    while (top >= 0) {
      const row = lines[top] ?? "";
      if (row[left] !== "│" || row[right] !== "│") {
        break;
      }
      top -= 1;
    }
    top += 1;
    assert.ok(bottom - top >= 2, `${width}x${height}: the tray has frame rows`);
    for (let y = top; y < bottom; y += 1) {
      assert.equal(lines[y]?.[left], "│", `${width}x${height}: left side at row ${y}`);
      assert.equal(lines[y]?.[right], "│", `${width}x${height}: right side at row ${y}`);
    }
    assert.ok(bottom < lines.length - 1, `${width}x${height}: the margin below the tray is kept`);
    assert.equal(lines[bottom + 1]?.trim(), "", `${width}x${height}: the margin row is blank`);
  }
});

test("the sessions page leaves a margin on the right and under the panel", async () => {
  for (const [width, height] of [
    [190, 44],
    [80, 24],
    [46, 14],
  ] as const) {
    const lines = await sessionsFrame(width, height);
    const top = lines.findIndex((line) => line.includes("╭─Sessions"));
    assert.ok(top > 0, `${width}x${height}: the sessions frame is painted`);
    const line = lines[top] ?? "";
    const left = line.indexOf("╭");
    const right = line.indexOf("╮");
    assert.ok(left >= 0 && right > left, `${width}x${height}: the top corners are painted`);
    let bottom = top + 1;
    while (bottom < lines.length && lines[bottom]?.[left] !== "╰") {
      bottom += 1;
    }
    assert.ok(bottom < lines.length, `${width}x${height}: the bottom is painted`);
    assert.equal(lines[bottom]?.[right], "╯", `${width}x${height}: the bottom corner aligns`);
    assert.equal(lines[top - 1]?.trim(), "", `${width}x${height}: one gap row above the panel`);
    assert.ok(right < width - 1, `${width}x${height}: the right margin is kept`);
    for (let y = top; y <= bottom; y += 1) {
      assert.equal(lines[y]?.[width - 1]?.trim(), "", `${width}x${height}: right margin at ${y}`);
    }
    assert.ok(bottom < lines.length - 1, `${width}x${height}: the bottom margin is kept`);
    assert.equal(lines[bottom + 1]?.trim(), "", `${width}x${height}: the margin row is blank`);
  }
});
