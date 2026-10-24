//! The reactive layer: what a keystroke, an appended row, and a streamed frame
//! each repaint.
//!
//! Every test measures the work a view actually did. The instrument is
//! `marked.lexer`: a markdown row that is re-rendered parses its text again, so
//! the sources the parser was handed are exactly the blocks a repaint touched.
//!
//! A committed row is printed into the terminal's scrollback once and never
//! repainted; the step in flight is the one region the window repaints while it
//! streams.

import assert from "node:assert/strict";
import test, { mock } from "node:test";
import { marked } from "marked";
import type { AppStore } from "../app/store";
import { delta, row, run } from "../testing/fixtures";
import { frameLines, mountApp, printedRows } from "../testing/render";
import { offlineStore, openSession } from "../testing/store";
import { App } from "./App";
import { CONTENT_INDENT } from "./panel";

/** Renders the app and returns the setup a test drives it with. */
async function mount(store: AppStore, width = 60, height = 16) {
  return mountApp(<App store={store} />, width, height);
}

/** Returns the frame without the composer line, which is where typing lands. */
function withoutComposer(lines: readonly string[]): string[] {
  const composer = lines.findLastIndex((line) => line.includes("> "));
  return lines.filter((_, index) => index !== composer);
}

/** The columns one answer's marker takes before its text. */
const ANSWER_MARK = /^∷ /;

/**
 * Returns the text of a transcript's answers, their markers aside.
 *
 * An answer's marker rides its first display row and the body hangs beside it,
 * so naming the marker is what makes two rows of the same answer comparable.
 */
function answerRows(lines: readonly string[]): string[] {
  return lines.map((line) => line.replace(ANSWER_MARK, "").replace(/^ {2}(?=\S)/, ""));
}

/**
 * Returns the transcript the frame painted, without the chrome around it.
 *
 * In the footer that region is the step in flight: the rows between the footer's
 * own top border and the input block's, which is the last frame above the line
 * the draft is typed on.
 */
/** Turns the painted page's invisible spaces into ordinary ones. */
function visible(line: string): string {
  return line.replace(/ /g, " ");
}

function transcriptLines(lines: readonly string[]): string[] {
  const composer = lines.findLastIndex((line) => line.includes("> "));
  let inputTop = lines.length;
  for (let index = composer; index >= 0; index -= 1) {
    if (lines[index]?.includes("╭") === true) {
      inputTop = index;
      break;
    }
  }
  // The band above the input block is the step in flight; its rows are painted
  // on the panel inside the window's frame, so the margin, the frame's side, and
  // the panel's own padding come off before the block's leading spaces.
  const body = lines.slice(0, inputTop).map((line) => {
    const text = visible(line);
    return frameEdge.test(text) ? "" : text.replace(/^│ ?/, "").replace(/ ?│$/, "").trimEnd();
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
 * A row that holds only the window's frame is a separator, not a row the block
 * said: the top edge, the bottom edge, and the frame's sides on an empty row.
 */
const frameEdge = /^[╭╰│╮╯─\s]*$/;

/**
 * Returns the printed rows without the frame the print continues.
 *
 * Each printed row carries the window's own border on both sides, and the first
 * print of a session carries the frame's top edge: none of that is the block's,
 * so only what the frame holds is a row the transcript said.
 */
function printedLines(rows: readonly string[]): string[] {
  const lines = rows.map((line) => {
    const text = visible(line.slice(CONTENT_INDENT)).trimEnd();
    return frameEdge.test(text) ? "" : text.replace(/ ?│[^│]*$/, "").trimEnd();
  });
  while (lines.length > 0 && (lines[0] ?? "").length === 0) {
    lines.shift();
  }
  while (lines.length > 0 && (lines.at(-1) ?? "").length === 0) {
    lines.pop();
  }
  return lines;
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

test("a keystroke never reaches the printed rows", async (t) => {
  const store = offlineStore();
  openSession(store, {
    messages: [
      row(1, "user", "explain this repository"),
      row(2, "assistant", "It relays intentions."),
    ],
    run: run("completed"),
  });
  const setup = await mount(store);
  // The mount printed the committed transcript; from here the rows are the
  // terminal's and nothing may print again.
  const committed = printedRows(setup);
  assert.ok(printedLines(committed).some((line) => line === "∷ It relays intentions."));
  const seen = watchParses();
  t.after(() => mock.restoreAll());

  const before = frameLines(setup.captureCharFrame());
  await setup.mockInput.pressKeys(["a", "b", "c"]);
  await setup.renderOnce();
  const after = frameLines(setup.captureCharFrame());

  assert.deepEqual(seen, [], "typing parses no markdown");
  assert.deepEqual(printedRows(setup), [], "typing prints no row");
  assert.deepEqual(
    withoutComposer(after),
    withoutComposer(before),
    "every cell but the input line is the cell it was",
  );
  assert.match(after.findLast((line) => line.includes("> ")) ?? "", /abc█/);

  store.dispose();
  await setup.dispose();
});

test("an appended row prints once and repaints nothing else", async (t) => {
  const store = offlineStore();
  const state = openSession(store, {
    messages: [row(1, "user", "first"), row(2, "assistant", "an earlier answer")],
    run: run("completed"),
  });
  const setup = await mount(store, 60, 24);
  const before = setup.captureCharFrame();
  printedRows(setup);
  const seen = watchParses();
  t.after(() => mock.restoreAll());

  state.applyFrame({ kind: "content", data: row(3, "assistant", "a later answer") });
  store.dispatch({ kind: "frame_received", state });
  await setup.renderOnce();

  assert.deepEqual(seen, ["a later answer"], "the appended row is the only row parsed");
  const rows = printedRows(setup);
  assert.deepEqual(
    printedLines(rows),
    ["∷ a later answer"],
    "the row is printed once, between blank rows",
  );
  // The footer gives the printed rows back: the window paints exactly the rows
  // the print did not take, and no other row of it moves.
  const paintedRows = (frame: string): number => frame.replace(/\n$/, "").split("\n").length;
  assert.equal(
    paintedRows(setup.captureCharFrame()),
    paintedRows(before) - rows.length,
    "the footer keeps every row the print did not take",
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
  const stream = transcriptLines(frameLines(setup.captureCharFrame()));
  assert.deepEqual(
    answerRows(stream.filter((line) => line.startsWith("∷"))),
    ["one two three"],
    "the tail is the answer the row becomes",
  );

  store.dispose();
  await setup.dispose();
});

test("the streaming tail paints the rows the printed block will have", async () => {
  const store = offlineStore();
  const state = openSession(store, { run: run("running") });
  const setup = await mount(store);

  const source = "- one\n- two\n\nafter the list";
  state.applyFrame({ kind: "text_delta", data: delta(1, "answer", source) });
  store.dispatch({ kind: "frame_received", state });
  // The first delta opens the band, so the footer takes its rows on this frame
  // and paints them on the next.
  await setup.renderOnce();
  await setup.flush();
  await setup.renderOnce();
  const tail = transcriptLines(frameLines(setup.captureCharFrame()));

  state.applyFrame({ kind: "content", data: row(1, "assistant", source) });
  store.dispatch({ kind: "frame_received", state });
  await setup.renderOnce();
  await setup.flush();
  await setup.renderOnce();
  const committed = printedLines(printedRows(setup));

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
