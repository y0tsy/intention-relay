//! The status line: the session's identity, and what is happening to the prompt.

import assert from "node:assert/strict";
import test from "node:test";
import { frameLines, painted } from "../testing/render";
import { DARK } from "../theme/palette";
import { StatusRow, type StatusFacts, statusDetail, statusPhrase } from "./StatusRow";

/** The facts of a session with nothing running. */
const IDLE: StatusFacts = {
  sessionId: null,
  runStatus: null,
  startedAt: null,
  answering: false,
  tokens: null,
  now: 1_700_000_000,
};

/** Returns the line the status row paints at one width. */
async function row(facts: Partial<StatusFacts>, width: number): Promise<string> {
  const frame = await painted(
    <StatusRow facts={{ ...IDLE, ...facts }} palette={DARK} width={width} />,
    width,
    1,
  );
  return frameLines(frame)[0] ?? "";
}

test("a session that is doing nothing reads as ready", async () => {
  assert.match(await row({}, 40), /^no session\s+ready$/);
});

test("the line names the session and what the run is doing", async () => {
  const line = await row(
    {
      sessionId: "0b1a2c3d-0000-4000-8000-000000000001",
      runStatus: "running",
      answering: true,
      startedAt: 1_700_000_000 - 12,
      tokens: 512,
    },
    60,
  );
  assert.match(line, /^00000001\s+answering · 12s · 512 tokens$/);
});

test("the detail line never speaks the protocol's vocabulary", () => {
  const detail = statusDetail({
    ...IDLE,
    runStatus: "running",
    startedAt: 1_700_000_000 - 5,
    tokens: 7,
  });
  assert.equal(detail, "5s · 7 tokens");
  assert.ok(!detail.includes("running"), "a status word never reaches the detail");
});

test("the phrase names the reader's wait, not the run's enum", () => {
  assert.equal(statusPhrase({ ...IDLE, runStatus: "running", answering: true }), "answering");
  assert.equal(statusPhrase({ ...IDLE, runStatus: "running", answering: false }), "thinking");
  assert.equal(statusPhrase({ ...IDLE, runStatus: "completed" }), "done");
  assert.equal(statusPhrase({ ...IDLE, runStatus: "interrupted" }), "stopped");
});

test("a line too narrow for both halves keeps the prompt's fate", async () => {
  const facts = {
    sessionId: "0b1a2c3d-0000-4000-8000-000000000001",
    runStatus: "running" as const,
    answering: true,
    startedAt: 1_700_000_000 - 12,
    tokens: 512,
  };
  assert.match(await row(facts, 60), /^00000001/);
  assert.equal(await row(facts, 24), "answering · 12s");
});

test("a detail that does not fit whole is dropped, not cut", async () => {
  const facts = {
    sessionId: "0b1a2c3d-0000-4000-8000-000000000001",
    runStatus: "running" as const,
    answering: true,
    startedAt: 1_700_000_000 - 12,
    tokens: 512,
  };
  // Every line the row can paint is the phrase, or the phrase with whole parts
  // appended in order; anything else would be a part cut mid-word.
  const whole = ["answering", "answering · 12s", "answering · 12s · 512 tokens"];
  for (let width = 12; width <= 40; width += 1) {
    const line = await row(facts, width);
    assert.ok(line.length <= width, `${width}: "${line}" fits`);
    const tail = whole.find((candidate) => line.endsWith(candidate));
    assert.ok(tail !== undefined, `${width}: "${line}" holds whole parts`);
    assert.match(line.slice(0, line.length - tail.length), /^(00000001\s+)?$/, `${width}`);
  }
});
