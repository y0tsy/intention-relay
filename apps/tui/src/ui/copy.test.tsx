//! Selection: a drag over the transcript selects text and copies it.
//!
//! The renderer owns the selection and the app owns the copy, so the tests drive
//! the same mouse events a terminal sends and read what reached the clipboard.

import assert from "node:assert/strict";
import test from "node:test";
import type { CapturedFrame, RGBA } from "@opentui/core";
import { testRender } from "@opentui/react/test-utils";
import { row } from "../testing/fixtures";
import { offlineStore, openSession } from "../testing/store";
import { App } from "./App";

/** Mounts the app over one session and returns its setup and copied texts. */
async function mount() {
  const store = offlineStore();
  openSession(store, {
    messages: [row(1, "user", "explain this repository"), row(2, "assistant", "It relays.")],
  });
  const copied: string[] = [];
  const setup = await testRender(<App store={store} copy={(text) => copied.push(text)} />, {
    width: 80,
    height: 24,
  });
  await setup.renderOnce();
  return { store, setup, copied };
}

/** Returns the background of the span covering one column of one line. */
function backgroundAt(frame: CapturedFrame, line: number, column: number): RGBA | undefined {
  let cell = 0;
  for (const span of frame.lines[line]?.spans ?? []) {
    const width = [...span.text].length;
    if (column >= cell && column < cell + width) {
      return span.bg;
    }
    cell += width;
  }
  return undefined;
}

test("a drag across one line copies the text it crossed", async () => {
  const { store, setup, copied } = await mount();
  await setup.mockMouse.drag(5, 6, 14, 6);
  await setup.renderOnce();
  assert.deepEqual(copied, ["It relays."]);
  store.dispose();
  await setup.dispose();
});

test("a drag across rows copies every row it touched", async () => {
  const { store, setup, copied } = await mount();
  await setup.mockMouse.drag(20, 6, 8, 3);
  await setup.renderOnce();
  assert.deepEqual(copied, ["lain this repository\nIt relays."]);
  store.dispose();
  await setup.dispose();
});

test("a drag over an empty area copies nothing", async () => {
  const { store, setup, copied } = await mount();
  await setup.mockMouse.drag(30, 10, 40, 10);
  await setup.renderOnce();
  assert.deepEqual(copied, [], "nothing was selected, so nothing was copied");
  store.dispose();
  await setup.dispose();
});

test("the selected cells are painted over their own background", async () => {
  const { store, setup } = await mount();
  const before = backgroundAt(setup.captureSpans(), 6, 6);
  const past = backgroundAt(setup.captureSpans(), 6, 30);
  await setup.mockMouse.drag(5, 6, 13, 6);
  await setup.renderOnce();
  const after = backgroundAt(setup.captureSpans(), 6, 6);
  assert.notDeepEqual(after, before, "the selection repaints the cells it covers");
  assert.deepEqual(
    backgroundAt(setup.captureSpans(), 6, 30),
    past,
    "a cell past the selection keeps its own background",
  );
  store.dispose();
  await setup.dispose();
});
