//! The binding: one read per version, whatever the selector hands back.

import assert from "node:assert/strict";
import test from "node:test";
import { testRender } from "@opentui/react/test-utils";
import type { AppState } from "../app/state";
import type { AppStore } from "../app/store";
import { offlineStore } from "../testing/store";
import { useStore } from "./use-store";

/** Counts the reads the binding made, so a test can hold it to one per version. */
let reads = 0;

/** Reads the draft as a pair, which is a value built on the spot. */
function selectPair(state: AppState): { readonly length: number } {
  reads += 1;
  return { length: state.draft.text.length };
}

/** Paints the pair's length. */
function Probe({ store }: { store: AppStore }) {
  const pair = useStore(store, selectPair);
  return <text>{String(pair.length)}</text>;
}

test("a selector that builds its value does not repaint forever", async () => {
  reads = 0;
  const store = offlineStore();
  // A selector read on every render hands React a new object each time unless
  // the binding holds one per version, and the subscription then repaints until
  // it exhausts React's depth limit.
  const setup = await testRender(<Probe store={store} />, { width: 20, height: 1 });
  for (const step of ["a", "b", "c"]) {
    store.dispatch({ kind: "draft_typed", text: step });
    await setup.renderOnce();
  }
  assert.equal(setup.captureCharFrame().trim(), "3", "the pair follows the state");
  await setup.dispose();
  store.dispose();
});

test("one version is read once, however many renders it takes", async () => {
  reads = 0;
  const store = offlineStore();
  const setup = await testRender(<Probe store={store} />, { width: 20, height: 1 });
  await setup.renderOnce();
  await setup.renderOnce();
  assert.equal(reads, 1, "the second render reads the version it already has");
  store.dispatch({ kind: "draft_typed", text: "a" });
  await setup.renderOnce();
  assert.equal(reads, 2, "a new version is read again");
  await setup.dispose();
  store.dispose();
});
