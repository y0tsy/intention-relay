//! The hint band: the keys a surface offers, fitted to the columns it has.

import assert from "node:assert/strict";
import test from "node:test";
import { painted } from "../testing/render";
import { DARK } from "../theme/palette";
import { renderHints } from "./format";
import { HintBand } from "./HintBand";

/** Renders the band at one width and returns the line it painted. */
async function band(width: number): Promise<string> {
  const frame = await painted(<HintBand width={width} palette={DARK} />, width, 1);
  return (frame.split("\n")[0] ?? "").trim();
}

test("the band paints the hints that fit its text area", async () => {
  for (const width of [24, 34, 40, 60, 100, 120]) {
    assert.equal(await band(width), renderHints(width - 2), `${width} columns`);
  }
});

test("no width cuts a hint mid-word", async () => {
  // What the band paints is a prefix of the whole list of hints, joined by the
  // separator, so every hint it shows is whole.
  const whole = renderHints(200);
  for (let width = 20; width <= 120; width += 1) {
    const line = await band(width);
    assert.ok(line.length <= width - 2, `${width}: "${line}" fits the text area`);
    assert.ok(whole.startsWith(line), `${width}: "${line}" is a prefix of the whole band`);
    assert.ok(!line.endsWith(" "), `${width}: "${line}" does not end in a separator`);
  }
});
