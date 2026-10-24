//! The docked session browser: its geometry, its table, and its legend.

import assert from "node:assert/strict";
import test from "node:test";
import { summary } from "../testing/fixtures";
import { browserColumnWidths, browserLayout, SlotRows } from "../kit";
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
  const layout = browserLayout(width, height, rows.length);
  assert.ok(layout !== null, "the window holds the docked panel");
  const frame = await painted(
    <box flexDirection="row">
      <box width={1} />
      <box width={width - 1}>
        <SlotRows rows={layout.rows}>
          <SessionBrowser
            rows={rows}
            error={options.error ?? null}
            selection={options.selection ?? 0}
            filter={options.filter ?? ""}
            tab="all"
            palette={DARK}
            now={options.now ?? 1_700_000_060}
          />
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
  assert.match(text, /⦿ All/);
  assert.match(text, /Type to filter sessions\.\.\./);
  assert.match(text, /Modified/);
  assert.match(text, /Enter select {2}Tab switch tab {2}Esc close/);
});

test("the panel grows with its rows up to half the window", async () => {
  const few = browserLayout(80, 40, 2);
  const many = browserLayout(80, 40, 60);
  assert.ok(few !== null && many !== null);
  assert.equal(few.rows, 12, "the frame, the chrome, and the two rows it has");
  assert.equal(many.rows, 20, "half the window, never more");
  assert.equal(browserLayout(80, 12, 3), null, "a window that cannot hold the chat keeps no panel");
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

test("a failure replaces the table with its own line", async () => {
  const painted_ = await lines([], 80, 30, { error: "local_daemon_unavailable: no daemon" });
  assert.match(painted_.join("\n"), /local_daemon_unavailable: no daemon/);
});
