//! The footer's row budget: the regions inside the frame always sum to its body.

import assert from "node:assert/strict";
import test from "node:test";
import {
  CONTENT_INDENT,
  DETAIL_ROWS,
  footerLayout,
  INPUT_CHROME_ROWS,
  inputBlockRows,
  LIVE_BAND_ROWS,
  MIN_BLOCK_ROWS,
  PANEL_MARGIN,
  welcomeLayout,
} from "./panel";

/** The rows the theme picker's panel asks for. */
const PICKER_ROWS = 5;

/** The rows a footer takes for one docked panel: the gap and the browser. */

test("an open session keeps a band for the step in flight above the input", () => {
  const one = footerLayout(80, 24, MIN_BLOCK_ROWS, 0, "session", 0);
  assert.equal(one.detailRows, DETAIL_ROWS);
  assert.equal(one.hintRows, 0, "a closed band takes no row");
  assert.equal(one.liveRows, LIVE_BAND_ROWS, "the step's band is the window's, not the run's");
  assert.equal(one.inputRows, MIN_BLOCK_ROWS, "a one-line buffer keeps its rows");
  assert.equal(one.rows, one.body, "the footer is the body it spends");

  const two = footerLayout(80, 24, MIN_BLOCK_ROWS + 1, 0, "session", 0);
  assert.equal(two.inputRows, MIN_BLOCK_ROWS + 1, "the block takes one row per buffer line");
  assert.equal(two.rows - one.rows, 1, "the footer grows with the row the line added");

  const tall = footerLayout(80, 24, 100, 0, "session", 0);
  // The share is a third of the body, plus the block's own chrome.
  assert.equal(tall.inputRows, Math.floor((24 - 1) / 3) + INPUT_CHROME_ROWS);
  assert.equal(tall.body, tall.inputRows + tall.detailRows + tall.liveRows);
});

test("a step in flight keeps the band it was given", () => {
  const live = footerLayout(80, 40, MIN_BLOCK_ROWS, 0, "session", 0);
  assert.equal(live.liveRows, LIVE_BAND_ROWS, "the live region is the band, no more");
  assert.equal(live.inputRows, MIN_BLOCK_ROWS, "the input keeps its rows");
  assert.equal(
    live.rows,
    live.liveRows + live.inputRows + live.detailRows,
    "the footer reserves exactly what it paints",
  );

  // A step on a short window takes what is left, and never less than nothing.
  const small = footerLayout(80, 12, MIN_BLOCK_ROWS, 0, "session", 0);
  assert.ok(small.liveRows > 0 && small.liveRows <= LIVE_BAND_ROWS);
  assert.ok(small.rows <= 12);
});

test("the welcome pane fills the window above the input block", () => {
  const welcome = footerLayout(80, 30, MIN_BLOCK_ROWS, 0, "welcome", 0);
  assert.equal(welcome.rows, 30, "a launch paints the whole window");
  assert.equal(
    welcome.liveRows,
    30 - welcome.inputRows - welcome.detailRows,
    "the welcome takes every row the input and the detail line leave",
  );
});

test("the band is the live region's loss, never the input's", () => {
  const two = footerLayout(80, 24, MIN_BLOCK_ROWS + 1, 0, "session", 0);
  const menu = footerLayout(80, 24, MIN_BLOCK_ROWS + 1, 8, "session", 0);
  assert.equal(menu.hintRows, 8, "the band takes exactly the rows it asks");
  assert.equal(menu.inputRows, two.inputRows, "the input keeps its rows");
  assert.equal(menu.rows, two.rows + 8, "the footer grows by the band's rows");

  const picker = footerLayout(80, 40, MIN_BLOCK_ROWS, PICKER_ROWS, "session", 0);
  assert.equal(picker.hintRows, PICKER_ROWS);
  assert.equal(picker.inputRows, MIN_BLOCK_ROWS);
});

test("a body too short for the band keeps the input and shows no band", () => {
  const cramped = footerLayout(80, 10, MIN_BLOCK_ROWS + 1, 4, "session", 0);
  assert.equal(cramped.hintRows, 0, "the band yields to the input block");
  assert.equal(cramped.inputRows, MIN_BLOCK_ROWS + 1, "the input keeps every row it asked for");
  assert.equal(
    cramped.body,
    cramped.hintRows + cramped.inputRows + cramped.detailRows + cramped.liveRows,
  );

  const taller = footerLayout(80, 10, MIN_BLOCK_ROWS, PICKER_ROWS, "session", 0);
  assert.equal(taller.hintRows, 0, "the picker yields too");
  assert.equal(taller.inputRows, MIN_BLOCK_ROWS);
});

test("every window spends its body exactly and never overflows", () => {
  for (const height of [3, 5, 8, 12, 20, 24, 30, 40, 60]) {
    for (const wanted of [0, 1, MIN_BLOCK_ROWS, MIN_BLOCK_ROWS + 2, 100]) {
      for (const band of [0, 1, 5, 8]) {
        for (const mode of ["session", "session", "welcome"] as const) {
          for (const docked of [0, 1, 2, 12]) {
            const layout = footerLayout(80, height, wanted, band, mode, docked);
            assert.equal(
              layout.liveRows + layout.hintRows + layout.inputRows + layout.detailRows,
              layout.body,
              `height ${height}, input ${wanted}, band ${band}, ${mode}`,
            );
            assert.ok(layout.rows <= height, "the footer never asks for more than the window");
            assert.ok(layout.liveRows >= 0);
            assert.ok(layout.inputRows >= 0);
          }
        }
      }
    }
  }
});

test("the printed transcript starts at the footer's content column", () => {
  assert.equal(CONTENT_INDENT, 3);
  const layout = footerLayout(80, 24, MIN_BLOCK_ROWS, 0, "session", 0);
  assert.equal(layout.contentColumns, 80 - CONTENT_INDENT * 2);
  assert.equal(layout.columns, 80 - PANEL_MARGIN * 2);
});

test("the input block grows one row per line break", () => {
  assert.equal(inputBlockRows(0), MIN_BLOCK_ROWS);
  assert.equal(inputBlockRows(3), MIN_BLOCK_ROWS + 3);
});

test("the welcome keeps the overview before the lockup", () => {
  const roomy = welcomeLayout(10);
  assert.deepEqual(roomy, { lead: 1, logo: 1, filler: 5, overview: 3 });
  const tight = welcomeLayout(4);
  assert.deepEqual(tight, { lead: 0, logo: 1, filler: 0, overview: 3 });
  const tiny = welcomeLayout(2);
  assert.deepEqual(tiny, { lead: 0, logo: 0, filler: 0, overview: 2 });
  const none = welcomeLayout(0);
  assert.deepEqual(none, { lead: 0, logo: 0, filler: 0, overview: 0 });
});
