//! The footer in a window with no room to spare.
//!
//! A short terminal is where the plan's rows and the surfaces' own frames have
//! to agree: the composer carries a label, a line, and a status row inside its
//! frame, and a panel carries its own. A frame a slot paints on its own is a
//! frame that lands over the band, the detail line, or the row the page ends on.

import assert from "node:assert/strict";
import test from "node:test";
import { row, run } from "../testing/fixtures";
import { frameLines, mountApp, printedRows } from "../testing/render";
import { offlineStore, openSession } from "../testing/store";
import { App } from "./App";

/** Renders until the printer has nothing left to write. */
async function settled(setup: Awaited<ReturnType<typeof mountApp>>): Promise<void> {
  for (let frame = 0; frame < 16; frame += 1) {
    printedRows(setup);
    await setup.renderOnce();
  }
  printedRows(setup);
}

test("a picker in a short window keeps its panel inside the page", async () => {
  const store = offlineStore();
  openSession(store, {
    messages: [row(1, "user", "a short question"), row(2, "assistant", "A short answer.")],
    run: run("completed"),
  });
  const setup = await mountApp(<App store={store} />, 46, 14);
  await settled(setup);
  store.dispatch({ kind: "theme_requested" });
  await setup.renderOnce();
  await setup.renderOnce();
  const lines = frameLines(setup.captureCharFrame());

  // The panel takes the rows the footer has, and the composer waits: a composer
  // frame painted with no rows of its own would stand over the panel's legend.
  const panel = lines.findIndex((line) => line.includes("╭─Theme"));
  const last = lines.findIndex((line) => line.includes("╰─ prompt"));
  assert.ok(panel > 0, "the picker's panel is painted");
  assert.equal(last, -1, "no composer frame is painted beside it");
  assert.ok(
    lines.some((line) => line.includes("enter apply · esc cancel")),
    "the panel's own legend closes it",
  );
  assert.ok(
    lines.every((line) => !line.includes("enter sends")),
    "and the band waits with the composer",
  );

  store.dispose();
  await setup.dispose();
});

test("a short window still prints the transcript and keeps its tray", async () => {
  const store = offlineStore();
  openSession(store, {
    messages: [row(1, "user", "a short question"), row(2, "assistant", "A short answer.")],
    run: run("completed"),
  });
  const setup = await mountApp(<App store={store} />, 46, 14);
  await settled(setup);
  const lines = frameLines(setup.captureCharFrame());
  assert.ok(
    lines.some((line) => line.includes("prompt")),
    "the tray keeps its composer in a window with room for it",
  );
  assert.equal(
    lines.filter((line) => line.includes("╭")).length,
    1,
    "one frame closes the tray, however short the window is",
  );
  store.dispose();
  await setup.dispose();
});
