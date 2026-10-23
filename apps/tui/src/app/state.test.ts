//! The reducer: every transition a front end can drive.
//!
//! A test states the fact that arrived and reads the effects the state asked
//! for, so the assertions are about the front end's own decisions and never
//! restate the client's rules.

import assert from "node:assert/strict";
import test from "node:test";
import type { ErrorDto } from "../bindings";
import { SessionStreamState } from "../client/stream";
import { RUN_ID, row, run, SESSION_ID, snapshot, summary } from "../testing/fixtures";
import type { Action } from "./action";
import { THEME_PREFERENCES } from "./action";
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
  state.update({ kind: "draft_typed", text: "hello" });
  assert.deepEqual(state.update({ kind: "input_submitted" }), [
    { kind: "create_session", firstTurn: "hello" },
  ]);
});

test("a prompt with a session is one user turn", () => {
  const state = openedSession();
  state.update({ kind: "draft_typed", text: "hello" });
  assert.deepEqual(state.update({ kind: "input_submitted" }), [
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

test("the picker opens on the preference in force", () => {
  const state = new AppState(null, "light");
  state.update({ kind: "theme_requested" });
  assert.equal(state.surface, "theme");
  assert.equal(state.selectedTheme, "light");
});

test("committing the highlight writes the preference it names", () => {
  for (const preference of THEME_PREFERENCES) {
    const state = new AppState(null, "auto");
    state.update({ kind: "theme_requested" });
    state.update({ kind: "selection_moved", delta: THEME_PREFERENCES.indexOf(preference) });
    assert.deepEqual(state.update({ kind: "selection_committed" }), [
      { kind: "persist_theme", preference },
    ]);
    assert.equal(state.theme, preference);
    assert.equal(state.surface, "chat");
  }
});

test("a dismissed picker changes nothing", () => {
  const state = new AppState(null, "dark");
  state.update({ kind: "theme_requested" });
  state.update({ kind: "selection_moved", delta: 1 });
  assert.deepEqual(state.update({ kind: "surface_closed" }), []);
  assert.equal(state.theme, "dark");
  assert.equal(state.surface, "chat");
});

/** Returns one paste of `lines` lines, large enough for the line to label it. */
function block(lines: number): string {
  return Array.from({ length: lines }, (_, index) => `line ${index}`).join("\n");
}

test("a paste the line can hold enters it as text, and a large one as one label", () => {
  const state = new AppState(SESSION_ID);
  state.update({ kind: "draft_pasted", text: "one\r\ntwo" });
  assert.equal(state.draft.text, "one\ntwo", "the line keeps one spelling of a break");
  state.update({ kind: "draft_cleared" });
  state.update({ kind: "draft_pasted", text: block(12) });
  const labelled: string = state.draft.text;
  assert.equal(labelled, "[pasted line 0 line 1 line 2 line 3 line 4 line 5 line 6 l... 12 lines]");
  assert.equal(state.draft.caret, labelled.length);
});

test("a paste label is one unit for the caret and the delete keys", () => {
  const state = new AppState(SESSION_ID);
  state.update({ kind: "draft_pasted", text: block(12) });
  const label = state.draft.text;
  state.update({ kind: "draft_edited", edit: "left" });
  assert.equal(state.draft.caret, 0, "the caret steps over the whole label");
  state.update({ kind: "draft_edited", edit: "right" });
  assert.equal(state.draft.caret, label.length);
  state.update({ kind: "draft_edited", edit: "backspace" });
  assert.equal(state.draft.text, "", "the whole label goes at once");
});

test("submitting a line expands every label into the paste it stands for", () => {
  const state = openedSession();
  state.update({ kind: "draft_pasted", text: block(12) });
  const submitted = state.update({ kind: "input_submitted" });
  assert.deepEqual(submitted, [{ kind: "send_turn", sessionId: SESSION_ID, content: block(12) }]);
  assert.equal(state.draft.text, "");
});

test("Ctrl+C arms the stop, and its next press fires it", () => {
  const state = runningSession();
  assert.deepEqual(effects(state, { kind: "ctrl_c_pressed" }), []);
  assert.match(state.notice ?? "", /press Ctrl\+C again to stop run/);
  assert.deepEqual(effects(state, { kind: "ctrl_c_pressed" }), ["cancel_run"]);
});

test("a client report leaves the arm standing", () => {
  const state = runningSession();
  state.update({ kind: "ctrl_c_pressed" });
  state.update({ kind: "elapsed_reported", millis: 500 });
  state.update({ kind: "sessions_listed", summaries: { sessions: [summary()], omitted: 0 } });
  assert.deepEqual(effects(state, { kind: "ctrl_c_pressed" }), ["cancel_run"]);
});

test("Ctrl+C abandons a typed line into the recallable history", () => {
  const state = new AppState(SESSION_ID);
  state.update({ kind: "draft_typed", text: "half a prompt" });
  assert.deepEqual(effects(state, { kind: "ctrl_c_pressed" }), []);
  assert.equal(state.draft.text, "");
  assert.equal(state.notice, null, "the clear arms nothing");
  state.update({ kind: "history_recalled", delta: -1 });
  assert.equal(state.draft.text, "half a prompt");
});

test("Ctrl+C on an empty line arms the exit, and its next press leaves", () => {
  const state = new AppState(SESSION_ID);
  assert.deepEqual(effects(state, { kind: "ctrl_c_pressed" }), []);
  assert.equal(state.notice, "press Ctrl+C again to exit");
  assert.deepEqual(effects(state, { kind: "ctrl_c_pressed" }), ["quit"]);
});

test("Esc closes the band first and means nothing else", () => {
  const state = runningSession();
  state.update({ kind: "draft_typed", text: "/theme" });
  assert.notEqual(state.menu, null);
  assert.deepEqual(effects(state, { kind: "escape_pressed" }), []);
  assert.equal(state.menu, null, "the band is gone");
  assert.equal(state.draft.text, "/theme", "the line is untouched");
  assert.deepEqual(effects(state, { kind: "escape_pressed" }), ["cancel_run"]);
});

test("Esc stops a live run on one press and clears a typed line on two", () => {
  const running = runningSession();
  assert.deepEqual(effects(running, { kind: "escape_pressed" }), ["cancel_run"]);
  const state = new AppState(SESSION_ID);
  state.update({ kind: "draft_typed", text: "typed" });
  assert.deepEqual(effects(state, { kind: "escape_pressed" }), []);
  assert.equal(state.notice, "press Esc again to clear the input");
  assert.deepEqual(effects(state, { kind: "escape_pressed" }), []);
  assert.equal(state.draft.text, "");
  state.update({ kind: "history_recalled", delta: -1 });
  assert.equal(state.draft.text, "typed");
});

test("Esc on an empty line leaves the front end", () => {
  const state = new AppState(SESSION_ID);
  assert.deepEqual(effects(state, { kind: "escape_pressed" }), ["quit"]);
});

test("Enter completes an incomplete word and runs a complete one", () => {
  const state = openedSession();
  state.update({ kind: "draft_typed", text: "/the" });
  assert.deepEqual(effects(state, { kind: "input_submitted" }), []);
  assert.equal(state.draft.text, "/theme ", "the press completes the word it ranks");
  assert.equal(state.menu?.kind, "argument", "the band reopens on the argument word");
  // An argument word that is still empty has nothing to complete, so the next
  // press runs the command itself.
  assert.deepEqual(effects(state, { kind: "input_submitted" }), []);
  assert.equal(state.surface, "theme");
  state.update({ kind: "surface_closed" });
  state.update({ kind: "draft_typed", text: "/sessions" });
  assert.deepEqual(
    effects(state, { kind: "input_submitted" }),
    ["list_sessions"],
    "a word that spells the command has nothing to complete",
  );
});

test("Tab commits the highlighted row, whatever the word spells", () => {
  const state = openedSession();
  state.update({ kind: "draft_typed", text: "/theme" });
  assert.deepEqual(effects(state, { kind: "menu_accepted" }), []);
  assert.equal(state.draft.text, "/theme ");
  assert.equal(state.menu?.kind, "argument", "the band reopens on the argument word");
  state.update({ kind: "menu_moved", delta: 1 });
  state.update({ kind: "menu_accepted" });
  assert.equal(state.draft.text, "/theme dark ");
});

test("the band's cursor is bounded by its rows and reset by another word", () => {
  const state = new AppState(SESSION_ID);
  state.update({ kind: "draft_typed", text: "/" });
  state.update({ kind: "menu_moved", delta: 9 });
  assert.equal(state.menuHighlight, 4, "the walk stops at the last row");
  state.update({ kind: "menu_moved", delta: -9 });
  assert.equal(state.menuHighlight, 0, "and at the first");
  state.update({ kind: "menu_moved", delta: 2 });
  state.update({ kind: "draft_typed", text: "t" });
  assert.equal(state.menuHighlight, 0, "another word is another list");
});

test("the arrows walk the draft's own lines before they walk the history", () => {
  const state = new AppState(SESSION_ID);
  state.update({ kind: "draft_typed", text: "one\ntwo" });
  state.update({ kind: "draft_edited", edit: "home" });
  state.update({ kind: "history_recalled", delta: 1 });
  assert.equal(state.draft.caret, 4, "the caret moved to the line below");
  state.update({ kind: "history_recalled", delta: 1 });
  assert.equal(state.draft.text, "one\ntwo", "the last line has no line below it");
});
