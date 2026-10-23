//! The session browser: a table that drops a column before it squeezes one.

import assert from "node:assert/strict";
import test from "node:test";
import { frameLines, painted } from "../testing/render";
import { summary } from "../testing/fixtures";
import { DARK } from "../theme/palette";
import { BROWSER_HINTS, SessionBrowser } from "./SessionBrowser";
import { fitColumns, renderHints, sessionCells } from "./format";

/** Renders the browser at one panel size and returns its lines. */
async function lines(
  summaries: ReturnType<typeof summary>[] | null,
  width: number,
  height: number,
  options: { selection?: number; error?: string | null; now?: number; omitted?: number } = {},
): Promise<string[]> {
  const frame = await painted(
    <SessionBrowser
      summaries={summaries === null ? null : { sessions: summaries, omitted: options.omitted ?? 0 }}
      error={options.error ?? null}
      selection={options.selection ?? 0}
      palette={DARK}
      width={width}
      height={height}
      now={options.now ?? 1_700_000_060}
    />,
    width,
    height,
  );
  return frameLines(frame);
}

test("a browser with no sessions says so instead of painting an empty table", async () => {
  const painted_ = await lines([], 40, 10);
  assert.equal(painted_[0]?.trim(), "sessions · 0");
  assert.match(painted_[1] ?? "", /no sessions yet/);
  assert.equal(painted_.at(-1)?.trim(), renderHints(38, BROWSER_HINTS));
});

test("a browser that is still reading says so", async () => {
  const painted_ = await lines(null, 40, 10);
  assert.equal(painted_[0]?.trim(), "sessions · reading");
});

test("a bounded list reports the sessions it left out", async () => {
  const rows = [summary()];
  assert.equal((await lines(rows, 60, 10))[0], "sessions · 1");
  assert.equal((await lines(rows, 60, 10, { omitted: 7 }))[0], "sessions · 1 +7 more");
});

test("a failure replaces the table with its own line", async () => {
  const painted_ = await lines(null, 40, 10, { error: "local_daemon_unavailable: no daemon" });
  assert.match(painted_.join("\n"), /local_daemon_unavailable: no daemon/);
});

test("a narrow browser drops a column instead of cutting one", async () => {
  const rows = [summary({ session_id: "0b1a2c3d-0000-4000-8000-0000000000ff" })];
  const now = 1_700_000_060;
  const wide = await lines(rows, 100, 10, { now });
  const narrow = await lines(rows, 34, 10, { now });
  const wideCells = sessionCells(rows[0] as never, fitColumns(94), now);
  const narrowCells = sessionCells(rows[0] as never, fitColumns(28), now);
  assert.ok(narrowCells.length < wideCells.length, "the narrow panel keeps fewer columns");
  for (const cell of narrowCells) {
    assert.ok((narrow[1] ?? "").includes(cell), `column "${cell}" is painted whole`);
  }
  assert.equal(narrow[1]?.includes("tokens"), false, "a dropped column is not squeezed in");
  assert.match(wide[1] ?? "", /000000ff/);
  assert.match(narrow[1] ?? "", /000000ff/);
});

test("the browser marks the row the keyboard is on", async () => {
  const rows = [
    summary({ session_id: "0b1a2c3d-0000-4000-8000-000000000001" }),
    summary({ session_id: "0b1a2c3d-0000-4000-8000-000000000002" }),
  ];
  const painted_ = await lines(rows, 60, 10, { selection: 1 });
  assert.equal(painted_[1]?.startsWith("00000001"), true, "the first row carries no marker");
  assert.match(painted_[2] ?? "", /^›\s+00000002/);
});

test("a selection below the window scrolls the table instead of leaving the panel", async () => {
  const rows = Array.from({ length: 12 }, (_unused, index) =>
    summary({
      session_id: `0b1a2c3d-0000-4000-8000-0000000000${index.toString(16).padStart(2, "0")}`,
    }),
  );
  const painted_ = await lines(rows, 60, 10, { selection: 11 });
  const paintedRows = painted_.slice(1, -1).filter((row) => row.trim().length > 0);
  assert.equal(paintedRows.length, 6, "the window holds six rows");
  assert.match(paintedRows.at(-1) ?? "", /^› /, "the selected row is inside the window");
  assert.match(paintedRows.at(-1) ?? "", /0000000b/);
});
