//! The reducer: every transition a front end can drive.
//!
//! A test states the fact that arrived and reads the effects the state asked
//! for, so the assertions are about the front end's own decisions and never
//! restate the client's rules.

import assert from "node:assert/strict";
import test from "node:test";
import type { ErrorDto } from "../bindings";
import { SessionStreamState } from "../client/stream";
import { RUN_ID, row, run, SESSION_ID, snapshot } from "../testing/fixtures";
import type { Action } from "./action";
import { AppState } from "./state";

/** Returns one typed failure. */
function failure(code = "local_daemon_connection_unavailable"): ErrorDto {
  return {
    code,
    category: "unavailable",
    message: "the local daemon connection is unavailable",
    retryable: true,
  };
}

/** Returns a state with one session open and its stream live. */
function openedSession(): AppState {
  const state = new AppState(SESSION_ID);
  state.update({ kind: "session_snapshot_loaded", snapshot: snapshot([], null) });
  state.update({
    kind: "session_stream_opened",
    state: SessionStreamState.fromSnapshot(snapshot([], run("running"))),
  });
  return state;
}

/** Returns a state whose live stream reports one running run. */
function runningSession(): AppState {
  const state = openedSession();
  state.update({
    kind: "frame_received",
    state: SessionStreamState.fromSnapshot(snapshot([row(1, "user", "hello")], run("running"))),
  });
  return state;
}

/** Applies one action and returns the effect kinds it asked for. */
function effects(state: AppState, action: Action): string[] {
  return state.update(action).map((effect) => effect.kind);
}

test("bootstrap opens the session the caller selected", () => {
  const state = new AppState(SESSION_ID);
  assert.deepEqual(state.update({ kind: "bootstrapped" }), [
    { kind: "open_session", sessionId: SESSION_ID },
  ]);
});

test("bootstrap without a selection asks for nothing", () => {
  const state = new AppState(null);
  assert.deepEqual(state.update({ kind: "bootstrapped" }), []);
  assert.equal(state.error, null);
});

test("an unreachable daemon becomes the error line", () => {
  const state = new AppState(null);
  state.update({ kind: "bootstrap_failed", error: failure() });
  assert.equal(
    state.error,
    "local_daemon_connection_unavailable: the local daemon connection is unavailable",
  );
});

test("a snapshot opens the one subscription the session needs", () => {
  const state = new AppState(null);
  assert.deepEqual(
    state.update({
      kind: "session_snapshot_loaded",
      snapshot: snapshot([row(1, "user", "hi")], null),
    }),
    [{ kind: "subscribe", sessionId: SESSION_ID }],
  );
  assert.equal(state.sessionId, SESSION_ID);
  assert.equal(state.stream, "inactive");
  assert.equal(state.transcript.length, 1);
});

test("a second read of the same session joins the rows it already holds", () => {
  const state = new AppState(null);
  state.update({
    kind: "session_snapshot_loaded",
    snapshot: snapshot([row(1, "user", "first")], null),
  });
  state.update({
    kind: "session_snapshot_loaded",
    snapshot: snapshot([row(2, "assistant", "second")], null),
  });
  assert.deepEqual(
    state.transcript.map((message) => message.text),
    ["first", "second"],
  );
});

test("a read of another session replaces the rows a front end holds", () => {
  const state = new AppState(null);
  state.update({
    kind: "session_snapshot_loaded",
    snapshot: snapshot([row(1, "user", "first")], null),
  });
  const other = "0b1a2c3d-0000-4000-8000-000000000002";
  state.update({
    kind: "session_snapshot_loaded",
    snapshot: snapshot([row(9, "user", "other", { session_id: other })], null, other),
  });
  assert.deepEqual(
    state.transcript.map((message) => message.text),
    ["other"],
  );
});

test("a created session opens itself", () => {
  const state = new AppState(null);
  assert.deepEqual(state.update({ kind: "session_created", sessionId: SESSION_ID }), [
    { kind: "open_session", sessionId: SESSION_ID },
  ]);
});

test("a prompt with no session creates the session it needs in one command", () => {
  const state = new AppState(null);
  assert.deepEqual(state.update({ kind: "turn_submitted", content: "hello" }), [
    { kind: "create_session", firstTurn: "hello" },
  ]);
});

test("a prompt with a session is one user turn", () => {
  const state = openedSession();
  assert.deepEqual(state.update({ kind: "turn_submitted", content: "hello" }), [
    { kind: "send_turn", sessionId: SESSION_ID, content: "hello" },
  ]);
});

test("an accepted turn opens the stream when none is live", () => {
  const state = new AppState(null);
  state.update({ kind: "session_snapshot_loaded", snapshot: snapshot([], null) });
  state.stream = "inactive";
  assert.deepEqual(state.update({ kind: "turn_accepted", outcome: { kind: "pending" } }), [
    { kind: "subscribe", sessionId: SESSION_ID },
  ]);
});

test("an accepted turn asks for nothing while the stream is live", () => {
  const state = openedSession();
  assert.deepEqual(state.update({ kind: "turn_accepted", outcome: { kind: "pending" } }), []);
  assert.deepEqual(
    state.update({ kind: "turn_accepted", outcome: { kind: "started", run_id: RUN_ID } }),
    [],
  );
});

test("a frame replaces the client's state while the stream is live", () => {
  const state = openedSession();
  const derived = SessionStreamState.fromSnapshot(
    snapshot([row(1, "user", "hello")], run("running")),
  );
  state.update({ kind: "frame_received", state: derived });
  assert.equal(state.transcript.length, 1);
  assert.equal(state.runStatus, "running");
});

test("a frame of a stream this front end no longer holds is dropped", () => {
  const state = openedSession();
  state.stream = "failed";
  const derived = SessionStreamState.fromSnapshot(
    snapshot([row(1, "user", "hello")], run("running")),
  );
  assert.deepEqual(state.update({ kind: "frame_received", state: derived }), []);
  assert.equal(state.transcript.length, 0);
});

test("a lost stream reports its failure and asks for one reconnect", () => {
  const state = openedSession();
  assert.deepEqual(state.update({ kind: "session_stream_failed", error: failure() }), [
    { kind: "connect" },
  ]);
  assert.equal(state.stream, "failed");
  assert.match(state.error ?? "", /local_daemon_connection_unavailable/);
});

test("a second loss inside one recovery asks for nothing", () => {
  const state = openedSession();
  state.update({ kind: "session_stream_failed", error: failure() });
  assert.deepEqual(state.update({ kind: "session_stream_failed", error: failure() }), []);
  assert.deepEqual(state.update({ kind: "session_stream_ended" }), []);
});

test("a frame restores the reconnect budget a loss spent", () => {
  const state = openedSession();
  state.update({ kind: "session_stream_failed", error: failure() });
  state.update({
    kind: "session_stream_opened",
    state: SessionStreamState.fromSnapshot(snapshot([], run("running"))),
  });
  state.update({
    kind: "frame_received",
    state: SessionStreamState.fromSnapshot(snapshot([row(1, "user", "hello")], run("running"))),
  });
  assert.deepEqual(state.update({ kind: "session_stream_ended" }), [{ kind: "connect" }]);
});

test("a subscription that opened and delivered nothing leaves the budget spent", () => {
  const state = openedSession();
  state.update({ kind: "session_stream_failed", error: failure() });
  state.update({
    kind: "session_stream_opened",
    state: SessionStreamState.fromSnapshot(snapshot([], run("running"))),
  });
  assert.deepEqual(state.update({ kind: "session_stream_ended" }), []);
});

test("a closed stream keeps the committed rows and drops the transient text", () => {
  const state = runningSession();
  state.update({
    kind: "frame_received",
    state: (() => {
      const derived = SessionStreamState.fromSnapshot(snapshot([], run("running")));
      derived.applyFrame({
        kind: "text_delta",
        data: { session_id: SESSION_ID, run_id: RUN_ID, step: 0, channel: "answer", text: "hi" },
      });
      return derived;
    })(),
  });
  assert.equal(state.provisionalText, "hi");
  state.update({ kind: "session_stream_ended" });
  assert.equal(
    state.provisionalText,
    "",
    "the transient text belongs to the stream that streamed it",
  );
  assert.equal(state.runStatus, "running", "the projection outlives the stream");
});

test("a run counts as live only while a live stream observes it", () => {
  const state = runningSession();
  assert.equal(state.liveRun?.run_id, RUN_ID);
  state.stream = "failed";
  assert.equal(state.liveRun, null);
});

test("a terminal run is not live even while its stream is", () => {
  const state = openedSession();
  state.update({
    kind: "frame_received",
    state: SessionStreamState.fromSnapshot(snapshot([], run("completed"))),
  });
  assert.equal(state.stream, "live");
  assert.equal(state.liveRun, null);
});

test("a stop request names the live run", () => {
  const state = runningSession();
  assert.deepEqual(state.update({ kind: "cancel_run_requested" }), [
    { kind: "cancel_run", sessionId: SESSION_ID, runId: RUN_ID },
  ]);
});

test("a stop request with no live run asks for nothing", () => {
  const state = new AppState(SESSION_ID);
  state.update({ kind: "session_snapshot_loaded", snapshot: snapshot([], null) });
  state.update({
    kind: "session_stream_opened",
    state: SessionStreamState.fromSnapshot(snapshot([], null)),
  });
  assert.equal(state.stream, "live");
  assert.deepEqual(state.update({ kind: "cancel_run_requested" }), []);
});

test("a failed stop becomes the error line", () => {
  const state = runningSession();
  state.update({ kind: "run_cancel_failed", error: failure("run_not_found") });
  assert.match(state.error ?? "", /^run_not_found:/);
});

test("a new session request carries no first turn", () => {
  const state = openedSession();
  assert.deepEqual(state.update({ kind: "new_session_requested" }), [
    { kind: "create_session", firstTurn: null },
  ]);
});

test("a failed read of the session becomes the error line without opening a stream", () => {
  const state = new AppState(null);
  assert.deepEqual(state.update({ kind: "session_snapshot_failed", error: failure() }), []);
  assert.match(state.error ?? "", /^local_daemon_connection_unavailable:/);
});

test("opening a stream clears the failure an earlier loss reported", () => {
  const state = openedSession();
  state.update({ kind: "session_stream_failed", error: failure() });
  assert.notEqual(state.error, null);
  state.update({
    kind: "session_stream_opened",
    state: SessionStreamState.fromSnapshot(snapshot([], null)),
  });
  assert.equal(state.error, null);
});

test("an effect the store could not start becomes the error line", () => {
  const state = openedSession();
  assert.deepEqual(
    effects(state, { kind: "effect_failed", error: failure("daemon_program_unavailable") }),
    [],
  );
  assert.match(state.error ?? "", /^daemon_program_unavailable:/);
});
