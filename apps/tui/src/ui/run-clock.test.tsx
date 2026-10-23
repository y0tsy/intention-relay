//! The clock: a live turn measures itself, and a settled one does not.

import assert from "node:assert/strict";
import test, { mock } from "node:test";
import { testRender } from "@opentui/react/test-utils";
import type { Action } from "../app/action";
import type { AppStore } from "../app/store";
import { ELAPSED_TICK_MILLIS, useRunClock } from "./run-clock";

/** Stands in for the store, recording what the clock reports to it. */
function recordingStore(): { store: AppStore; reported: number[] } {
  const reported: number[] = [];
  const store = {
    dispatch: (action: Action) => {
      if (action.kind === "elapsed_reported") {
        reported.push(action.millis);
      }
    },
  };
  return { store: store as unknown as AppStore, reported };
}

/** Mounts the clock alone, so its interval is the only thing under test. */
async function mountClock(
  store: AppStore,
  turnStartedAt: number | null,
  live: boolean,
): Promise<{ renderOnce: () => Promise<void>; dispose: () => Promise<void> }> {
  function Clock() {
    useRunClock(store, turnStartedAt, live);
    return <text>clock</text>;
  }
  return await testRender(<Clock />, { width: 10, height: 1 });
}

test("a live turn reports its elapsed time ten times a second", async (t) => {
  mock.timers.enable({ apis: ["setInterval", "Date"], now: 1_000 });
  t.after(() => mock.timers.reset());
  const { store, reported } = recordingStore();
  const setup = await mountClock(store, 900, true);
  assert.deepEqual(reported, [100], "the turn reads zero at once, not a tenth later");
  for (let step = 0; step < 3; step += 1) {
    mock.timers.tick(ELAPSED_TICK_MILLIS);
  }
  assert.deepEqual(reported, [100, 200, 300, 400], "every tenth is measured");
  await setup.dispose();
});

test("a settled turn measures nothing", async (t) => {
  mock.timers.enable({ apis: ["setInterval", "Date"], now: 1_000 });
  t.after(() => mock.timers.reset());
  const { store, reported } = recordingStore();
  const setup = await mountClock(store, 900, false);
  for (let step = 0; step < 5; step += 1) {
    mock.timers.tick(ELAPSED_TICK_MILLIS);
  }
  assert.deepEqual(reported, [], "a run that is over has no elapsed value to measure");
  await setup.dispose();
});
