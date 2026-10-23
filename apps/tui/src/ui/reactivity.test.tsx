//! The reactive layer: what a keystroke, an appended row, and a streamed frame
//! each repaint.
//!
//! Every test measures the work a view actually did. The instrument is
//! `marked.lexer`: a markdown row that is re-rendered parses its text again, so
//! the sources the parser was handed are exactly the blocks a repaint touched.

import assert from "node:assert/strict";
import test, { mock } from "node:test";
import { testRender } from "@opentui/react/test-utils";
import { marked } from "marked";
import type { AppStore } from "../app/store";
import { delta, row, run } from "../testing/fixtures";
import { offlineStore, openSession } from "../testing/store";
import { App } from "./App";

/** Renders the app and returns the setup a test drives it with. */
async function mount(store: AppStore, width = 60, height = 16) {
  const setup = await testRender(<App store={store} />, { width, height });
  await setup.renderOnce();
  return setup;
}

/** Returns the painted frame's lines, trimmed of padding and of trailing blanks. */
function frameLines(setup: { captureCharFrame: () => string }): string[] {
  const lines = setup
    .captureCharFrame()
    .split("\n")
    .map((line) => line.trim());
  while (lines.length > 0 && lines.at(-1) === "") {
    lines.pop();
  }
  return lines;
}

/**
 * Returns the frame without the composer line, which is where typing lands.
 *
 * A committed user row opens with the same prompt mark and sits above the
 * composer, so the line to drop is the last one that opens that way.
 */
function withoutComposer(lines: readonly string[]): string[] {
  const composer = lines.findLastIndex((line) => line.includes("> "));
  return lines.filter((_, index) => index !== composer);
}

/** The columns one live answer's marker takes before its text. */
const ANSWER_MARK = /^∷ /;

/**
 * Returns the text of a transcript's answers, their markers aside.
 *
 * An answer's marker rides its first display row and the body hangs beside it,
 * so naming the marker is what makes two frames of the same answer comparable.
 */
function answerRows(lines: readonly string[]): string[] {
  return lines.map((line) => line.replace(ANSWER_MARK, "").replace(/^ {2}(?=\S)/, ""));
}

/**
 * Returns the transcript the frame painted, without the chrome around it.
 *
 * The transcript is the region between the panel's own top border and the input
 * block's: the input block is the last frame above the line the draft is typed
 * on, so its top border is where the transcript stops.
 */
function transcriptLines(lines: readonly string[]): string[] {
  const start = lines.findIndex((line) => line.startsWith("╭"));
  const composer = lines.findLastIndex((line) => line.includes("> "));
  let inputTop = lines.length;
  for (let index = composer - 1; index > start; index -= 1) {
    const line = lines[index];
    if (line?.includes("╭") === true) {
      inputTop = index;
      break;
    }
  }
  // A nested frame - the user card or the input block inside the panel -
  // contributes its own border columns, which peel away until the text shows.
  const body = lines.slice(start + 1, inputTop).map((line) => {
    let row = line;
    for (;;) {
      const before = row;
      row = row
        .replace(/^│ ?/, "")
        .replace(/^ +/, "")
        .replace(/ +│ *$/, "")
        .replace(/│ *$/, "")
        .trimEnd();
      if (row === before) {
        return row;
      }
    }
  });
  while (body.length > 0 && (body[0] ?? "").trim().length === 0) {
    body.shift();
  }
  while (body.length > 0 && (body.at(-1) ?? "").trim().length === 0) {
    body.pop();
  }
  return body;
}

/**
 * Records the source of every markdown parse from now on.
 *
 * The replacement keeps the parser the app would have called, so what a test
 * records is the work the app really did.
 */
function watchParses(): string[] {
  const seen: string[] = [];
  const parse = marked.lexer.bind(marked);
  mock.method(marked, "lexer", ((source: string) => {
    seen.push(source);
    return parse(source);
  }) as typeof marked.lexer);
  return seen;
}

test("a keystroke never reaches the committed rows", async (t) => {
  const store = offlineStore();
  openSession(store, {
    messages: [
      row(1, "user", "explain this repository"),
      row(2, "assistant", "It relays intentions."),
    ],
    run: run("completed"),
  });
  const setup = await mount(store);
  const seen = watchParses();
  t.after(() => mock.restoreAll());

  const before = frameLines(setup);
  await setup.mockInput.pressKeys(["a", "b", "c"]);
  await setup.renderOnce();
  const after = frameLines(setup);

  assert.deepEqual(seen, [], "typing parses no markdown");
  assert.deepEqual(
    withoutComposer(after),
    withoutComposer(before),
    "every cell but the input line is the cell it was",
  );
  assert.match(after.findLast((line) => line.includes("> ")) ?? "", /abc█/);

  store.dispose();
  await setup.dispose();
});

test("an appended row lays out only the tail", async (t) => {
  const store = offlineStore();
  const state = openSession(store, {
    messages: [row(1, "user", "first"), row(2, "assistant", "an earlier answer")],
    run: run("completed"),
  });
  const setup = await mount(store, 60, 24);
  const seen = watchParses();
  t.after(() => mock.restoreAll());

  state.applyFrame({ kind: "content", data: row(3, "assistant", "a later answer") });
  store.dispatch({ kind: "frame_received", state });
  await setup.renderOnce();

  assert.deepEqual(seen, ["a later answer"], "the appended row is the only row parsed");
  // A committed row is separated from the next one by one blank row: the user
  // card, the gap, the earlier answer, the gap, the later answer.
  const shown = transcriptLines(frameLines(setup));
  assert.ok(
    shown.some((line) => line.startsWith("╭─") && line.includes("❯ you")),
    "the prompt is a card",
  );
  assert.ok(shown.includes("first"), "the card names the prompt");
  assert.deepEqual(
    shown.filter((line) => line.startsWith("∷")),
    ["∷ an earlier answer", "∷ a later answer"],
    "each answer opens with its own marker",
  );

  store.dispose();
  await setup.dispose();
});

test("the provisional tail is laid out per frame and never cached", async (t) => {
  const store = offlineStore();
  const state = openSession(store, {
    messages: [row(1, "user", "ask")],
    run: run("running"),
  });
  const setup = await mount(store, 60, 24);
  const seen = watchParses();
  t.after(() => mock.restoreAll());

  for (const chunk of ["one ", "two ", "three"]) {
    state.applyFrame({ kind: "text_delta", data: delta(1, "answer", chunk) });
    store.dispatch({ kind: "frame_received", state });
    await setup.renderOnce();
  }

  assert.deepEqual(
    seen,
    ["one ", "one two ", "one two three"],
    "every frame parses the block still being written",
  );
  const stream = transcriptLines(frameLines(setup));
  assert.ok(
    stream.some((line) => line.startsWith("╭─") && line.includes("❯ you")),
    "the prompt is a card",
  );
  assert.deepEqual(
    answerRows(stream.filter((line) => line.startsWith("∷"))),
    ["one two three"],
    "the tail is the answer the row becomes",
  );

  store.dispose();
  await setup.dispose();
});

test("the streaming tail lays out as the committed row it becomes", async () => {
  const store = offlineStore();
  const state = openSession(store, { run: run("running") });
  const setup = await mount(store);

  const source = "- one\n- two\n\nafter the list";
  state.applyFrame({ kind: "text_delta", data: delta(1, "answer", source) });
  store.dispatch({ kind: "frame_received", state });
  await setup.renderOnce();
  const tail = transcriptLines(frameLines(setup));

  state.applyFrame({ kind: "content", data: row(1, "assistant", source) });
  store.dispatch({ kind: "frame_received", state });
  await setup.renderOnce();
  const committed = transcriptLines(frameLines(setup));

  assert.deepEqual(answerRows(tail), ["- one", "- two", "", "after the list"]);
  assert.deepEqual(
    answerRows(committed),
    answerRows(tail),
    "the row lands exactly where the tail stood",
  );

  store.dispose();
  await setup.dispose();
});

test("a streamed delta leaves the committed rows the value they were", () => {
  const store = offlineStore();
  const state = openSession(store, {
    messages: [row(1, "user", "ask")],
    run: run("running"),
  });
  const rows = store.state.transcript;

  state.applyFrame({ kind: "text_delta", data: delta(1, "answer", "one") });
  store.dispatch({ kind: "frame_received", state });

  assert.equal(store.state.transcript, rows, "a view is handed the rows it already painted");
  store.dispose();
});

test("a session with no rows hands over one empty list", () => {
  const store = offlineStore();
  assert.equal(store.state.transcript, store.state.transcript);
  store.dispose();
});
