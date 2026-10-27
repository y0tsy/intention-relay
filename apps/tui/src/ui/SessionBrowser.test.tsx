//! The docked session browser: its geometry, its table, and its legend.

import assert from "node:assert/strict";
import test from "node:test";
import { summary } from "../testing/fixtures";
import { browserColumnWidths, browserPlan, ContentColumns, SlotRows } from "../kit";
import { frameLines, painted } from "../testing/render";
import { DARK } from "../theme/palette";
import { SessionBrowser } from "./SessionBrowser";

/** Renders the browser at one panel size and returns its lines. */
async function lines(
  rows: ReturnType<typeof summary>[],
  width: number,
  height: number,
  options: { selection?: number; error?: string | null; now?: number; filter?: string } = {},
): Promise<string[]> {
  const layout = browserPlan(width, height, rows.length);
  assert.ok(layout !== null, "the window holds the docked panel");
  const frame = await painted(
    <box flexDirection="row">
      <box width={1} />
      <box width={width - 2}>
        <SlotRows rows={layout.rows}>
          {/* The dock keeps one column on each side; the panel is the width it
              was given, not one column wider than its frame. */}
          <ContentColumns columns={width - 2}>
            <SessionBrowser
              rows={rows}
              error={options.error ?? null}
              selection={options.selection ?? 0}
              filter={options.filter ?? ""}
              tab="all"
              palette={DARK}
              now={options.now ?? 1_700_000_060}
              compact={layout.compact}
            />
          </ContentColumns>
        </SlotRows>
      </box>
    </box>,
    width,
    height,
  );
  return frameLines(frame);
}

test("the docked panel is titled, tabbed, and carries its own legend", async () => {
  const painted_ = await lines([summary()], 80, 30);
  const text = painted_.join("\n");
  assert.match(painted_[0] ?? "", /^╭─Sessions─+╮$/);
  assert.match(text, /⦿ All sessions/);
  assert.match(text, /Filter by id or path/);
  assert.match(text, /Modified/);
  assert.match(text, /Enter open · Tab switch list · Esc close/);
});

test("the panel grows with its rows up to half the window", async () => {
  const few = browserPlan(80, 40, 2);
  const many = browserPlan(80, 40, 60);
  assert.ok(few !== null && many !== null);
  assert.equal(few.rows, 12, "the frame, the chrome, and the two rows it has");
  assert.equal(many.rows, 20, "half the window, never more");
  assert.equal(few.compact, false, "a tall window keeps the filter and the headings");
});

test("a panel the footer leaves short keeps its tabs and its rows", async () => {
  // The panel is a page of its own: it takes the rows the tray had and drops
  // the filter row and the column headings rather than the sessions.
  const short = browserPlan(80, 12, 3, 12);
  assert.ok(short !== null);
  assert.equal(short.compact, true, "a short page drops the filter and the headings");
  assert.equal(short.dataRows, 1, "it still shows a session row");
  assert.equal(short.rows, 6, "the tabs, that row, and the panel's own frame and legend");

  const page = await lines([summary()], 60, 12);
  const text = page.join("\n");
  assert.match(page[0] ?? "", /^╭─Sessions─+╮$/, "the panel keeps its title");
  assert.match(text, /⦿ All sessions/, "and its tabs");
  assert.doesNotMatch(text, /Filter by id or path/, "the filter waits for a taller window");
  assert.doesNotMatch(text, /Modified/, "so do the column headings");
});

test("a window too short for the panel opens none", async () => {
  assert.equal(browserPlan(80, 6, 3, 6), null, "six rows hold no panel and no transcript");
});

test("the table's columns are shares of the content width", async () => {
  const wide = browserColumnWidths(74);
  const narrow = browserColumnWidths(24);
  assert.ok(wide.title > narrow.title, "the title column takes what the shares leave");
  for (const column of Object.values(narrow)) {
    assert.ok(column >= 0, "no column goes negative");
  }
});

test("the browser marks the row the keyboard is on", async () => {
  const rows = [summary({ session_id: "a" }), summary({ session_id: "b" })];
  const painted_ = await lines(rows, 80, 30, { selection: 1 });
  const first = painted_.find((line) => line.includes(" a "));
  const second = painted_.find((line) => line.includes(" b "));
  assert.equal(first?.includes(">"), false, "the first row carries no marker");
  assert.equal(second?.includes(">"), true, "the selected row carries the cursor");
});

test("a narrow panel reads its first column and keeps the path whole", async () => {
  const rows = [summary({ session_id: "0b1a2c3d-0000-4000-8000-0000000000ff" })];
  const painted_ = await lines(rows, 60, 30);
  assert.match(painted_.join("\n"), /0b1a2c3d/);
});

test("the footer counts the rows the window shows", async () => {
  const rows = [summary(), summary({ session_id: "second" })];
  const painted_ = await lines(rows, 80, 30);
  assert.match(painted_.join("\n"), /1-2 of 2/);
});

test("a narrow panel keeps its counter whole", async () => {
  // The legend needs more columns than a narrow panel has: it gives way to the
  // counter of the rows the reader is on, rather than being painted over it.
  const painted_ = await lines([], 46, 14);
  const counter = painted_.find((line) => line.includes("of 0")) ?? "";
  assert.match(counter, /0 of 0\s*│$/u, "the counter closes the panel's last row");
  assert.doesNotMatch(counter, /Esc clo0/u, "and the legend never runs through it");
});

test("a failure replaces the table with its own line", async () => {
  const painted_ = await lines([], 80, 30, { error: "local_daemon_unavailable: no daemon" });
  assert.match(painted_.join("\n"), /local_daemon_unavailable: no daemon/);
});

test("a long failure ends inside the panel, with the cut marked", async () => {
  const error = "local_daemon_connection_unavailable: the local daemon connection is unavailable";
  for (const width of [80, 46]) {
    const painted_ = await lines([], width, 30, { error });
    const line = painted_.find((row) => row.includes("local_daemon_connection_unavailable"));
    assert.ok(line !== undefined, `${width}: the failure is painted`);
    // The panel's content is narrower than the window by its margin, frame, and
    // padding. A cell that measures from the window paints one column past it,
    // and the clip takes the ellipsis with it.
    assert.match(line, /…\s*│$/u, `${width}: the cut is marked inside the border`);
    assert.ok(line.length < width, `${width}: the row fits the window`);
  }
});
