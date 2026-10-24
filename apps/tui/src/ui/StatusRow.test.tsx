//! The status row and the detail line: the words a reader acts on.

import assert from "node:assert/strict";
import test from "node:test";
import { frameLines, painted } from "../testing/render";
import { DARK } from "../theme/palette";
import {
  DEFAULT_HINT,
  DetailLine,
  detailRuns,
  WIDE_HINT,
  type StatusFacts,
  StatusRow,
  statusRuns,
} from "./StatusRow";

/** The facts of a front end that reached its daemon and opened nothing. */
const IDLE: StatusFacts = {
  connection: "ready",
  sessionId: null,
  mode: null,
  phase: null,
  runStatus: null,
  elapsed: null,
  usage: null,
  startedAt: null,
  finishedAt: null,
};

/** Renders one status row at one width and returns its one line. */
async function row(facts: StatusFacts, width = 78): Promise<string> {
  const frame = await painted(
    <box width={width} height={1}>
      <StatusRow facts={facts} palette={DARK} />
    </box>,
    width,
    1,
  );
  return frameLines(frame)[0] ?? "";
}

/** Returns the text of every run, without its styling. */
function textOf(runs: readonly { readonly text: string }[]): string {
  return runs.map((run) => run.text).join("");
}

test("a ready front end with no session reads its connection and nothing else", async () => {
  assert.equal(await row(IDLE), "Ready · no session");
});

test("a session names its identity, its mode, and its live phase", async () => {
  assert.equal(
    await row({
      ...IDLE,
      sessionId: "abcdef0123456789",
      mode: "build",
      phase: "answering",
      runStatus: "running",
      elapsed: 4123,
    }),
    "Ready · session abcdef01 · build · Answering… 4.1s",
  );
});

test("a turn that has not produced text yet is waiting", () => {
  assert.equal(
    textOf(statusRuns({ ...IDLE, phase: "waiting", runStatus: "running", elapsed: 400 })),
    "Ready · no session · Waiting… 0.4s",
  );
});

test("a completed run reads its committed duration in whole seconds", async () => {
  assert.equal(
    await row({
      ...IDLE,
      sessionId: "abcdef01",
      mode: "build",
      runStatus: "completed",
      startedAt: 1000,
      finishedAt: 1013,
      usage: { input: 1200, output: 300 },
    }),
    "Ready · session abcdef01 · build · Answered in 13s · ctx in 1200 out 300",
  );
});

test("a narrow window keeps the live state and gives up the session first", () => {
  assert.equal(
    textOf(
      statusRuns(
        {
          ...IDLE,
          sessionId: "abcdef0123456789",
          mode: "build",
          phase: "answering",
          runStatus: "running",
          elapsed: 4123,
        },
        34,
      ),
    ),
    "Ready · Answering… 4.1s",
  );
});

test("a run that reported no usage says so, and one that committed none says nothing", () => {
  assert.equal(
    textOf(statusRuns({ ...IDLE, runStatus: "completed", usage: "not_reported" })),
    "Ready · no session · Answered · ctx not reported",
  );
  assert.equal(
    textOf(statusRuns({ ...IDLE, runStatus: "completed", usage: null })),
    "Ready · no session · Answered",
  );
});

test("a failed run keeps its word, and the typed error keeps the detail line", async () => {
  assert.equal(
    await row({ ...IDLE, connection: "offline", runStatus: "failed" }),
    "Offline · no session · Failed",
  );
  assert.equal(
    textOf(detailRuns({ error: "transport: refused", notice: null })),
    "transport: refused",
  );
});

test("the detail line carries the notice, then the idle hint", () => {
  assert.equal(
    textOf(detailRuns({ error: null, notice: "unknown command /x" })),
    "unknown command /x",
  );
  assert.equal(textOf(detailRuns({ error: null, notice: null })), DEFAULT_HINT);
  assert.equal(
    textOf(detailRuns({ error: null, notice: null }, 80)),
    WIDE_HINT,
    "a wide page names every command",
  );
});

test("the detail line paints one row at the panel's width", async () => {
  const frame = await painted(
    <box width={60} height={1}>
      <DetailLine error={null} notice={null} palette={DARK} />
    </box>,
    60,
    1,
  );
  assert.deepEqual(frameLines(frame), [DEFAULT_HINT]);
});
