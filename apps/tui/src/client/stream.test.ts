//! The committed session state: retention, deduplication, and transient text.

import assert from "node:assert/strict";
import test from "node:test";
import { delta, RUN_ID, row, run, SESSION_ID, snapshot } from "../testing/fixtures";
import { ClientFailure } from "./errors";
import { RETAINED_TRANSCRIPT_MESSAGES, runStatusIsTerminal, SessionStreamState } from "./stream";

test("a snapshot seeds the rows and the run projection", () => {
  const state = SessionStreamState.fromSnapshot(
    snapshot([row(1, "user", "hello"), row(2, "assistant", "hi")], run("running")),
  );
  assert.deepEqual(
    state.messages.map((message) => message.text),
    ["hello", "hi"],
  );
  assert.equal(state.run?.run_id, RUN_ID);
  assert.equal(state.status, "running");
  assert.equal(state.provisionalText, "", "a read carries no transient text");
});

test("a content frame appends its row and a repeat of the newest row is unchanged", () => {
  const state = SessionStreamState.fromSnapshot(snapshot([row(1, "user", "hello")], null));
  assert.deepEqual(state.applyFrame({ kind: "content", data: row(2, "assistant", "hi") }), {
    kind: "row",
    row: 2,
  });
  assert.deepEqual(
    state.applyFrame({ kind: "content", data: row(2, "assistant", "hi") }),
    { kind: "unchanged" },
    "the newest accepted row carries the identity the snapshot already carried",
  );
  assert.equal(state.messages.length, 2);
});

test("the retained transcript keeps the newest rows and drops the oldest", () => {
  const rows = Array.from({ length: RETAINED_TRANSCRIPT_MESSAGES + 5 }, (_unused, index) =>
    row(index + 1, "user", `row ${index + 1}`),
  );
  const state = SessionStreamState.fromSnapshot(snapshot(rows, null));
  assert.equal(state.messages.length, RETAINED_TRANSCRIPT_MESSAGES);
  assert.equal(state.messages[0]?.id, 6, "the five oldest rows were dropped");
  assert.equal(state.messages.at(-1)?.id, RETAINED_TRANSCRIPT_MESSAGES + 5);
});

test("a text delta advances only the channel it names", () => {
  const state = SessionStreamState.fromSnapshot(snapshot([], run("running")));
  state.applyFrame({ kind: "text_delta", data: delta(0, "reasoning", "thinking") });
  state.applyFrame({ kind: "text_delta", data: delta(0, "answer", "ans") });
  state.applyFrame({ kind: "text_delta", data: delta(0, "answer", "wer") });
  assert.equal(state.provisionalReasoning, "thinking");
  assert.equal(state.provisionalText, "answer");
});

test("a delta of a new step replaces both buffers", () => {
  const state = SessionStreamState.fromSnapshot(snapshot([], run("running")));
  state.applyFrame({ kind: "text_delta", data: delta(0, "answer", "first") });
  state.applyFrame({ kind: "text_delta", data: delta(1, "answer", "second") });
  assert.equal(state.provisionalText, "second");
});

test("a committed assistant row drops the transient text of its step", () => {
  const state = SessionStreamState.fromSnapshot(snapshot([], run("running")));
  state.applyFrame({ kind: "text_delta", data: delta(0, "answer", "streamed") });
  state.applyFrame({ kind: "content", data: row(7, "assistant", "streamed") });
  assert.equal(state.provisionalText, "");
  assert.deepEqual(
    state.messages.map((message) => message.text),
    ["streamed"],
  );
});

test("a delta for a run the state no longer projects changes nothing", () => {
  const state = SessionStreamState.fromSnapshot(snapshot([], run("running")));
  const update = state.applyFrame({
    kind: "text_delta",
    data: { ...delta(0, "answer", "late"), run_id: "another-run" },
  });
  assert.deepEqual(update, { kind: "unchanged" });
  assert.equal(state.provisionalText, "");
});

test("a terminal status drops the transient text and keeps the projection", () => {
  const state = SessionStreamState.fromSnapshot(snapshot([], run("running")));
  state.applyFrame({ kind: "text_delta", data: delta(0, "answer", "streamed") });
  const update = state.applyFrame({ kind: "status", data: run("completed") });
  assert.deepEqual(update, { kind: "run", run: RUN_ID });
  assert.equal(state.provisionalText, "");
  assert.equal(state.status, "completed");
  assert.equal(runStatusIsTerminal("completed"), true);
});

test("a repeated status frame is unchanged", () => {
  const state = SessionStreamState.fromSnapshot(snapshot([], run("running")));
  assert.deepEqual(state.applyFrame({ kind: "status", data: run("running") }), {
    kind: "unchanged",
  });
});

test("the first status of a run the session did not carry is accepted", () => {
  const state = SessionStreamState.fromSnapshot(snapshot([], null));
  assert.deepEqual(state.applyFrame({ kind: "status", data: run("starting") }), {
    kind: "run",
    run: RUN_ID,
  });
});

test("a frame of another session is rejected without mutating the state", () => {
  const state = SessionStreamState.fromSnapshot(snapshot([row(1, "user", "hello")], null));
  assert.throws(
    () =>
      state.applyFrame({
        kind: "content",
        data: row(2, "assistant", "other", { session_id: SESSION_ID.replace(/1$/, "2") }),
      }),
    (error: unknown) =>
      error instanceof ClientFailure && error.dto.code === "invalid_session_subscription",
  );
  assert.equal(state.messages.length, 1);
});

test("a reconciliation joins rows by identity and keeps the read's run", () => {
  const state = SessionStreamState.fromSnapshot(
    snapshot([row(1, "user", "first"), row(2, "assistant", "second")], run("running")),
  );
  state.reconcile(
    SessionStreamState.fromSnapshot(
      snapshot([row(2, "assistant", "second"), row(3, "user", "third")], run("completed")),
    ),
  );
  assert.deepEqual(
    state.messages.map((message) => message.id),
    [1, 2, 3],
    "the rows the read repeated are not appended twice",
  );
  assert.equal(state.status, "completed");
});

test("ending a stream drops the transient text and keeps the committed rows", () => {
  const state = SessionStreamState.fromSnapshot(
    snapshot([row(1, "user", "hello")], run("running")),
  );
  state.applyFrame({ kind: "text_delta", data: delta(0, "answer", "streamed") });
  state.endStream();
  assert.equal(state.provisionalText, "");
  assert.equal(state.messages.length, 1);
  assert.equal(state.status, "running", "the projection outlives the stream");
});
