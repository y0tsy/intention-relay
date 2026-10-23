//! The composer: the prompt line, its caret, and its placeholder.

import assert from "node:assert/strict";
import test from "node:test";
import type { AppStore } from "../app/store";
import { frameLines, painted } from "../testing/render";
import { offlineStore } from "../testing/store";
import { DARK } from "../theme/palette";
import { Composer } from "./Composer";

/** Builds a store whose input line holds `text`, with the caret `back` from its end. */
function draftStore(text: string, back = 0): AppStore {
  const store = offlineStore();
  store.dispatch({ kind: "draft_typed", text });
  for (let step = 0; step < back; step += 1) {
    store.dispatch({ kind: "draft_edited", edit: "left" });
  }
  return store;
}

/** Returns the line the composer paints at one width. */
async function line(store: AppStore, width = 40): Promise<string> {
  const frame = await painted(<Composer store={store} palette={DARK} focused />, width, 1);
  store.dispose();
  return frameLines(frame)[0] ?? "";
}

test("an empty draft shows the placeholder with the caret", async () => {
  assert.equal(await line(offlineStore()), "> ▏type a prompt");
});

test("a draft shows its text with the caret where the next character goes", async () => {
  assert.equal(await line(draftStore("hello")), "> hello▏");
  assert.equal(await line(draftStore("hello", 5)), "> ▏hello");
  assert.equal(await line(draftStore("hello", 3)), "> he▏llo");
});

test("a draft with a newline keeps both lines", async () => {
  const store = draftStore("one\ntwo");
  const frame = await painted(<Composer store={store} palette={DARK} focused />, 40, 2);
  store.dispose();
  assert.deepEqual(frameLines(frame), ["> one", "two▏"]);
});
