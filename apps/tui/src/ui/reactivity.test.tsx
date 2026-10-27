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
import { CliRenderEvents } from "@opentui/core";
import type { TestRendererSetup } from "@opentui/core/testing";
import { marked } from "marked";
import type { AppStore } from "../app/store";
import { delta, row, run } from "../testing/fixtures";
import { frameLines, mountApp, paintedLines, printedRows } from "../testing/render";
import { offlineStore, openSession } from "../testing/store";
import { App } from "./App";
import { TRANSCRIPT_INDENT } from "../kit";

/** Renders the app and returns the setup a test drives it with. */
async function mount(store: AppStore, width = 60, height = 16) {
  return mountApp(<App store={store} />, width, height);
}

/**
 * Renders until the printer has nothing left to write, and returns the rows.
 *
 * A print takes a frame per chunk: the frame in between is what applies the rows
 * the print took and puts the surface back on the window's last row, so a block
 * that the window is too short to hold arrives over several frames.
 */
async function printed(setup: TestRendererSetup): Promise<string[]> {
  const rows: string[] = [];
  let quiet = 0;
  for (let frame = 0; frame < 24 && quiet < 2; frame += 1) {
    const batch = printedRows(setup);
    rows.push(...batch);
    if (batch.length > 0) {
      quiet = 0;
    } else if (rows.length > 0) {
      quiet += 1;
    }
    await setup.renderOnce();
  }
  return rows;
}

/** Returns the frame without the composer line, which is where typing lands. */
function withoutComposer(lines: readonly string[]): string[] {
  const composer = lines.findLastIndex((line) => line.includes("> "));
  return lines.filter((_, index) => index !== composer);
}

/**
 * Returns the text of a transcript's answers.
 *
 * An answer opens with its own label row and its prose follows, so the label is
 * what two answers of the same text are compared without.
 */
function answerRows(lines: readonly string[]): string[] {
  return lines.filter((line) => line !== "assistant");
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
  // The edge above the input block is the step in flight; its rows are painted
  // on the page inside the window's frame, so the margin, the frame's side, and
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
 * Returns the printed rows without the rule the transcript runs beside them.
 *
 * Every printed row carries the transcript's left rule, and a print leads with
 * the blank row that separates two blocks: neither is the block's, so only what
 * the rule holds is a row the transcript said.
 */
function printedLines(rows: readonly string[]): string[] {
  const lines = rows.map((line) => {
    const text = visible(line.slice(TRANSCRIPT_INDENT)).trimEnd();
    return frameEdge.test(text) ? "" : text;
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

/** The geometry one print wrote its chunk at. */
interface WriteAt {
  /** The rows the footer had taken when the chunk was written. */
  readonly footer: number;
  /** The row the surface stood on when the chunk was written. */
  readonly offset: number | undefined;
  /** The mode the renderer still had pending, if any. */
  readonly pending: unknown;
  /** The window's own rows. */
  readonly height: number;
}

/** Records the geometry of every chunk a print writes from now on. */
function watchWrites(setup: TestRendererSetup): WriteAt[] {
  const writes: WriteAt[] = [];
  setup.renderer.on(CliRenderEvents.EXTERNAL_OUTPUT, () => {
    const renderer = setup.renderer as unknown as {
      readonly terminalHeight: number;
      readonly footerHeight: number;
      readonly renderOffset?: number;
      readonly pendingNativeMode?: unknown;
    };
    writes.push({
      footer: renderer.footerHeight,
      offset: renderer.renderOffset,
      pending: renderer.pendingNativeMode ?? null,
      height: renderer.terminalHeight,
    });
  });
  return writes;
}

/** Returns the rows of the window above the tray, blank rows excluded. */
function aboveTray(setup: TestRendererSetup): string[] {
  const lines = paintedLines(setup);
  const tray = lines.findIndex((line) => line.includes("╭"));
  assert.ok(tray > 0, "the tray is painted");
  return lines.slice(0, tray).filter((line) => line.trim() !== "");
}

/** Asserts that the printed page and the window connect without a bare row. */
function assertRuleRuns(setup: TestRendererSetup, rows: readonly string[], turn: string): void {
  assert.deepEqual(
    rows.filter((line) => !/^\s*│/u.test(line)),
    [],
    `${turn}: no printed row is blank without the rule`,
  );
  const above = aboveTray(setup);
  assert.ok(above.length > 0, `${turn}: the window holds rows above the tray`);
  assert.deepEqual(
    above.filter((line) => !line.includes("│")),
    [],
    `${turn}: every row above the tray carries the rule`,
  );
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
  assert.ok(printedLines(committed).some((line) => line === "It relays intentions."));
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
    run: run("running"),
  });
  const setup = await mount(store, 60, 24);
  const before = setup.captureCharFrame();
  printedRows(setup);
  const seen = watchParses();
  t.after(() => mock.restoreAll());

  state.applyFrame({ kind: "content", data: row(3, "assistant", "a later answer") });
  store.dispatch({ kind: "frame_received", state });
  const rows = await printed(setup);

  assert.deepEqual(seen, ["a later answer"], "the appended row is the only row parsed");
  assert.deepEqual(
    printedLines(rows),
    ["assistant", "a later answer"],
    "the row is printed once, under its label",
  );
  // The printed rows come out of the window above the footer: the footer paints
  // exactly the rows the print did not take, and no other row of it moves.
  const paintedRows = (frame: string): number => frame.replace(/\n$/, "").split("\n").length;
  assert.equal(
    paintedRows(setup.captureCharFrame()),
    paintedRows(before) - rows.length,
    "the footer keeps every row the print did not take",
  );

  store.dispose();
  await setup.dispose();
});

test("an open panel takes no row the printed page wrote", async () => {
  const store = offlineStore();
  openSession(store, {
    messages: [row(1, "user", "explain this repository"), row(2, "assistant", "It relays.")],
    run: run("completed"),
  });
  const setup = await mount(store, 60, 16);
  await printed(setup);
  const before = setup.renderer.footerHeight;
  const transcript = setup.captureCharFrame();

  store.dispatch({ kind: "theme_requested" });
  await setup.renderOnce();
  await setup.renderOnce();

  // The renderer grows a footer by scrolling its region, and a row that scroll
  // leaves the scrollback for good: the picker opens into the rows the footer
  // has and gives the printed page none of them back.
  assert.equal(
    setup.renderer.footerHeight,
    before,
    "the panel takes the rows the footer already has",
  );
  assert.deepEqual(printedRows(setup), [], "the panel prints nothing above the tray");
  assert.notDeepEqual(setup.captureCharFrame(), transcript, "the picker paints the window");

  store.dispose();
  await setup.dispose();
});

test("the session browser is a page that takes the tray's rows", async () => {
  const store = offlineStore();
  openSession(store, {
    messages: [row(1, "user", "explain this repository"), row(2, "assistant", "It relays.")],
    run: run("completed"),
  });
  const setup = await mount(store, 60, 16);
  await printed(setup);
  const before = setup.renderer.footerHeight;

  // The browser owns the footer while it is open: the composer waits, the
  // panel takes the rows the tray had, and the page above it keeps every row a
  // print wrote.
  store.dispatch({ kind: "sessions_requested" });
  await setup.renderOnce();
  await setup.renderOnce();

  const frame = setup.captureCharFrame();
  assert.match(frame, /Sessions/, "the browser's panel is painted");
  assert.doesNotMatch(frame, /prompt/, "the composer waits for the panel to close");
  assert.ok(
    setup.renderer.footerHeight <= Math.max(before, 16),
    "the panel stays inside the window it was opened in",
  );
  assert.deepEqual(printedRows(setup), [], "the panel prints no row");

  store.dispose();
  await setup.dispose();
});

test("every printed row carries the transcript's rule", async () => {
  const store = offlineStore();
  const state = openSession(store, {
    messages: [row(1, "user", "ask")],
    run: run("running"),
  });
  const setup = await mount(store, 60, 16);
  const answer = "First paragraph.\n\nSecond paragraph.";
  for (const chunk of ["First paragraph.", "\n\nSecond"]) {
    state.applyFrame({ kind: "text_delta", data: delta(1, "answer", chunk) });
    store.dispatch({ kind: "frame_received", state });
    await setup.renderOnce();
  }
  // The step commits: the page seals, the block it became is adopted, and the
  // run line closes the page.
  state.applyFrame({ kind: "content", data: row(2, "assistant", answer) });
  store.dispatch({ kind: "frame_received", state });
  state.applyFrame({ kind: "status", data: run("completed") });
  store.dispatch({ kind: "frame_received", state });
  const first = await printed(setup);
  assert.ok(
    first.some((line) => line.includes("run complete")),
    "the run line is printed",
  );

  // A row the page did not write is a row of the canvas the print carried with
  // it: it lands in the scrollback as a blank line without the rule, and the
  // reader sees the transcript's rule break after the row it closed with.
  assertRuleRuns(setup, first, "first turn");

  // A second turn prints after the first one closed. The footer gives new rows
  // back, and the page it leaves must meet the tray on the same rule.
  state.applyFrame({ kind: "content", data: row(3, "user", "and then?") });
  store.dispatch({ kind: "frame_received", state });
  state.applyFrame({ kind: "content", data: row(4, "assistant", "It keeps relaying.") });
  store.dispatch({ kind: "frame_received", state });
  state.applyFrame({
    kind: "status",
    data: run("completed", { run_id: "0b1a2c3d-0000-4000-8000-0000000000a2" }),
  });
  store.dispatch({ kind: "frame_received", state });
  const second = await printed(setup);
  assert.ok(
    second.some((line) => line.includes("It keeps relaying.")),
    "the second turn is printed",
  );
  assertRuleRuns(setup, second, "second turn");

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

  // The page prints the rows that are final and the edge paints the last rows
  // that may still change, so a frame lays the text out for both and no frame
  // reuses the layout it made before.
  assert.deepEqual(
    [...new Set(seen)].sort(),
    ["one ", "one two ", "one two three"],
    "every frame parses the block still being written",
  );
  const stream = transcriptLines(frameLines(setup.captureCharFrame()));
  assert.deepEqual(answerRows(stream), ["one two three"], "the edge is the answer's last rows");

  store.dispose();
  await setup.dispose();
});

test("a streamed answer prints once, in the rows its block has", async () => {
  const source = "• one\n• two\n\nafter the list";

  /** Streams one answer chunk by chunk and returns every row the terminal took. */
  async function streamed(): Promise<string[]> {
    const store = offlineStore();
    const state = openSession(store, { run: run("running") });
    const setup = await mount(store);
    const rows: string[] = [];
    for (const chunk of ["• one\n", "• two\n", "\n", "after the list"]) {
      state.applyFrame({ kind: "text_delta", data: delta(1, "answer", chunk) });
      store.dispatch({ kind: "frame_received", state });
      rows.push(...(await printed(setup)));
    }
    state.applyFrame({ kind: "content", data: row(1, "assistant", source) });
    store.dispatch({ kind: "frame_received", state });
    rows.push(...(await printed(setup)));
    store.dispose();
    await setup.dispose();
    return printedLines(rows);
  }

  /** Prints the committed answer and returns every row the terminal took. */
  async function committed(): Promise<string[]> {
    const store = offlineStore();
    openSession(store, {
      messages: [row(1, "assistant", source)],
      run: run("running"),
    });
    const setup = await mount(store);
    const rows = await printed(setup);
    store.dispose();
    await setup.dispose();
    return printedLines(rows);
  }

  const rows = await streamed();
  assert.equal(
    rows.filter((line) => line.includes("after the list")).length,
    1,
    "the answer is written once",
  );
  assert.deepEqual(rows, await committed(), "the streamed rows are the rows its block has");
});

test("two blocks in a row print without an empty row between them", async () => {
  const store = offlineStore();
  const state = openSession(store, {
    messages: [row(1, "user", "first")],
    run: run("running"),
  });
  const setup = await mount(store, 60, 24);
  // The opening page is the one this test does not watch: the two prints that
  // follow are what a chunk written before the footer applied would split.
  await printed(setup);
  const writes = watchWrites(setup);

  state.applyFrame({ kind: "content", data: row(2, "assistant", "an earlier answer") });
  store.dispatch({ kind: "frame_received", state });
  const first = await printed(setup);
  state.applyFrame({ kind: "content", data: row(3, "assistant", "a later answer") });
  store.dispatch({ kind: "frame_received", state });
  const second = await printed(setup);
  const rows = [...first, ...second];

  // A write above a footer that still holds the rows scrolls the row the print
  // stopped on out of the window and leaves a row without the rail behind.
  assert.deepEqual(
    rows.filter((line) => !/^\s*│/u.test(line)),
    [],
    "no printed row is blank without the rail",
  );
  assert.deepEqual(
    printedLines(rows),
    ["assistant", "an earlier answer", "", "assistant", "a later answer"],
    "each block is printed once, separated by the page's own blank row",
  );
  assert.ok(writes.length > 0, "the printed blocks were written");
  for (const write of writes) {
    assert.equal(write.pending, null, "a chunk is written only after the footer applied");
    assert.equal(
      write.offset,
      write.height - write.footer,
      "a chunk lands on the row the footer left for it",
    );
  }

  store.dispose();
  await setup.dispose();
});

test("a print writes every row it measured, visible or scrolled", async () => {
  const store = offlineStore();
  const items = Array.from(
    { length: 40 },
    (_, index) => `• kept ${String(index + 1).padStart(2, "0")}`,
  );
  openSession(store, {
    messages: [row(1, "assistant", items.join("\n"))],
    run: run("completed"),
  });
  const setup = await mount(store, 60, 16);
  const writes = watchWrites(setup);
  const rows: string[] = [];
  // A window this short cannot hold the block: the footer gives its working
  // rows back chunk by chunk, and every row written above the tray stays in the
  // terminal's history.
  for (let frame = 0; frame < 40; frame += 1) {
    rows.push(...printedRows(setup));
    await setup.renderOnce();
  }
  rows.push(...printedRows(setup));

  for (const item of items) {
    const seen = rows.filter((line) => line.includes(item)).length;
    assert.equal(seen, 1, `"${item}" is written once`);
  }
  assert.deepEqual(
    rows.filter((line) => line.trim() === ""),
    [],
    "no chunk leaves a blank row above the page",
  );
  for (const write of writes) {
    assert.equal(write.pending, null, "a chunk is written only after the footer applied");
    assert.equal(
      write.offset,
      write.height - write.footer,
      "the surface stands where the footer begins",
    );
  }

  store.dispose();
  await setup.dispose();
});

test("a burst of rows prints every block once, in the order it committed", async () => {
  const store = offlineStore();
  const texts = Array.from(
    { length: 24 },
    (_, index) => `burst row ${String(index + 1).padStart(2, "0")}`,
  );
  openSession(store, {
    messages: texts.map((text, index) =>
      row(index + 1, index % 2 === 0 ? "user" : "assistant", text),
    ),
    run: run("completed"),
  });
  const setup = await mount(store, 80, 24);
  const lines: string[] = [];
  // One print is one block, so a burst takes a frame per block: the window does
  // not mount the whole burst into one surface.
  for (let frame = 0; frame < 40; frame += 1) {
    lines.push(...printedRows(setup));
    await setup.renderOnce();
  }
  const printed = printedLines(lines);
  for (const [index, text] of texts.entries()) {
    const seen = printed.filter((line) => line.includes(text)).length;
    assert.equal(seen, 1, `"${text}" is printed once, not ${seen} times`);
    if (index === 0) {
      continue;
    }
    const at = printed.findIndex((line) => line.includes(text));
    const before = printed.findIndex((line) => line.includes(texts[index - 1] as string));
    assert.ok(before < at, `"${text}" is printed under the row before it`);
  }
  store.dispose();
  await setup.dispose();
});

test("a block taller than the window prints in chunks and keeps every row", async () => {
  const store = offlineStore();
  const items = Array.from(
    { length: 96 },
    (_, index) => `• item ${String(index + 1).padStart(3, "0")}`,
  );
  openSession(store, {
    messages: [row(1, "assistant", items.join("\n"))],
    run: run("completed"),
  });
  const setup = await mount(store, 80, 24);
  const lines: string[] = [];
  let quiet = 0;
  for (let frame = 0; frame < 40 && quiet < 3; frame += 1) {
    const batch = printedRows(setup);
    lines.push(...batch);
    // One chunk per frame: the print moves on the frame that carries the rows
    // the last chunk took, so the block arrives over several of them.
    quiet = batch.length === 0 ? quiet + 1 : 0;
    await setup.renderOnce();
  }
  const printed = printedLines(lines);
  assert.ok(printed.length >= items.length, "every row of the block reached the terminal");
  for (const item of items) {
    const seen = printed.filter((line) => line.includes(item)).length;
    assert.equal(seen, 1, `"${item}" is printed once, not ${seen} times`);
  }
  store.dispose();
  await setup.dispose();
});

test("a streamed row reaches the terminal once, however the page grew", async (t) => {
  const store = offlineStore();
  const state = openSession(store, {
    messages: [row(1, "user", "ask")],
    run: run("running"),
  });
  const setup = await mount(store, 60, 20);
  t.after(() => mock.restoreAll());
  const answer =
    "## Relay types\n\nThe relay carries three kinds of value, and each one has a writer.";
  const rows: string[] = [];
  for (let at = 0; at < answer.length; at += 4) {
    state.applyFrame({ kind: "text_delta", data: delta(1, "answer", answer.slice(at, at + 4)) });
    store.dispatch({ kind: "frame_received", state });
    await setup.renderOnce();
    rows.push(...printedRows(setup));
  }
  // The step is over: its page is frozen and the rows it held are its last rows.
  state.applyFrame({ kind: "content", data: row(2, "assistant", answer) });
  store.dispatch({ kind: "frame_received", state });
  for (let frame = 0; frame < 12; frame += 1) {
    rows.push(...printedRows(setup));
    await setup.renderOnce();
  }
  rows.push(...printedRows(setup));
  const lines = printedLines(rows);
  assert.equal(
    lines.filter((line) => line === "Relay types").length,
    1,
    "the heading is printed once, not once per page the stream re-read",
  );
  assert.equal(
    lines.filter((line) => line.includes("The relay carries three kinds")).length,
    1,
    "the paragraph is printed once",
  );

  store.dispose();
  await setup.dispose();
});

test("a page of many blocks adds no resize listener per block", async () => {
  const store = offlineStore();
  const answer = Array.from(
    { length: 12 },
    (_, index) =>
      `## Part ${index + 1}\n\nA paragraph that wraps at the panel's own width, ${index}.`,
  ).join("\n\n");
  openSession(store, { messages: [row(1, "assistant", answer)], run: run("completed") });
  const setup = await mount(store, 80, 24);
  const before = setup.renderer.listenerCount("resize");
  for (let frame = 0; frame < 10; frame += 1) {
    printedRows(setup);
    await setup.renderOnce();
  }
  const after = setup.renderer.listenerCount("resize");
  // The renderer warns above ten listeners, and its warning is written to the
  // terminal the transcript owns: it lands between two printed rows and the
  // window never finds its place again.
  assert.equal(after, before, "a printed block listens to no resize of its own");
  assert.ok(after < 10, `the renderer holds ${after} resize listeners`);

  store.dispose();
  await setup.dispose();
});

test("a welcome page reads the window through one resize listener", async () => {
  const store = offlineStore();
  const setup = await mount(store, 80, 24);
  const before = setup.renderer.listenerCount("resize");
  for (let frame = 0; frame < 5; frame += 1) {
    await setup.renderOnce();
  }
  const after = setup.renderer.listenerCount("resize");
  // The welcome page asks for its columns from many components. A window hook
  // that subscribed for each of them would spend the renderer's listener budget
  // and print its warning over the page.
  assert.equal(after, before, "the welcome page adds no listener per reader");
  assert.ok(after <= 1, `the renderer holds ${after} resize listeners`);

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
