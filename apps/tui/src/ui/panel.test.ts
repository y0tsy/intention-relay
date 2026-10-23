//! The panel's row budget: four regions that always sum to the body.

import assert from "node:assert/strict";
import test from "node:test";
import {
  DETAIL_ROWS,
  INPUT_CHROME_ROWS,
  MIN_BLOCK_ROWS,
  PANEL_FRAME,
  TRANSCRIPT_TOP_ROWS,
  inputBlockRows,
  panelFits,
  panelLayout,
  welcomeLayout,
} from "./panel";

/** The rows the theme picker's panel asks for. */
const PICKER_ROWS = 5;

test("the panel spends its body on the transcript, the menu, the input, the detail line", () => {
  const one = panelLayout(80, 24, MIN_BLOCK_ROWS, 0);
  assert.equal(one.detailRows, DETAIL_ROWS);
  assert.equal(one.hintRows, 0, "a closed band takes no row");
  assert.equal(one.inputRows, MIN_BLOCK_ROWS, "a one-line buffer keeps its rows");
  assert.equal(one.transcriptRows, 14, "a one-line buffer leaves the transcript its rows");
  assert.equal(
    one.transcriptRows + one.hintRows + one.inputRows + one.detailRows,
    one.body,
    "the body is spent exactly",
  );

  const two = panelLayout(80, 24, MIN_BLOCK_ROWS + 1, 0);
  assert.equal(two.inputRows, MIN_BLOCK_ROWS + 1, "the block takes one row per buffer line");
  assert.equal(
    one.transcriptRows - two.transcriptRows,
    1,
    "the transcript gives up exactly the row the line added",
  );

  const tall = panelLayout(80, 24, 100, 0);
  // The share is a third of nineteen body rows, plus the block's own chrome.
  assert.equal(tall.inputRows, Math.floor((one.body - DETAIL_ROWS) / 3) + INPUT_CHROME_ROWS);
  assert.equal(tall.transcriptRows, one.body - DETAIL_ROWS - tall.inputRows);

  // The open menu is the transcript's loss, never the input's.
  const menu = panelLayout(80, 24, MIN_BLOCK_ROWS + 1, 8);
  assert.equal(menu.hintRows, 8, "the band takes exactly the rows it asks");
  assert.equal(menu.inputRows, two.inputRows, "the input keeps its rows");
  assert.equal(menu.transcriptRows + 8, two.transcriptRows, "the transcript loses the band's rows");

  // The picker's panel takes the same region at its own fixed height.
  const picker = panelLayout(80, 24, MIN_BLOCK_ROWS, PICKER_ROWS);
  assert.equal(picker.hintRows, PICKER_ROWS);
  assert.equal(picker.transcriptRows + PICKER_ROWS, one.transcriptRows);
  assert.equal(picker.inputRows, one.inputRows);
});

test("a body too short for the band keeps the input and shows no band", () => {
  const cramped = panelLayout(80, 12, MIN_BLOCK_ROWS + 1, 4);
  assert.equal(cramped.hintRows, 0, "the band yields to the input block");
  assert.equal(cramped.inputRows, MIN_BLOCK_ROWS + 1, "the input keeps every row it asked for");
  assert.equal(
    cramped.transcriptRows + cramped.hintRows + cramped.inputRows + cramped.detailRows,
    cramped.body,
  );

  const taller = panelLayout(80, 12, MIN_BLOCK_ROWS, PICKER_ROWS);
  assert.equal(taller.hintRows, 0, "the picker yields too");
  assert.equal(taller.inputRows, MIN_BLOCK_ROWS);
});

test("a window with no body spends no rows", () => {
  const tiny = panelLayout(80, PANEL_FRAME + 2, MIN_BLOCK_ROWS, 8);
  assert.equal(tiny.body, 0);
  assert.equal(tiny.transcriptRows + tiny.hintRows + tiny.inputRows + tiny.detailRows, 0);
});

test("the transcript's first content row is the frame, padding, and margin above it", () => {
  assert.equal(TRANSCRIPT_TOP_ROWS, 4);
});

test("every body height spends itself exactly", () => {
  for (const height of [3, 5, 8, 12, 20, 24, 30, 40, 60]) {
    for (const wanted of [0, 1, MIN_BLOCK_ROWS, MIN_BLOCK_ROWS + 2, 100]) {
      for (const band of [0, 1, 5, 8]) {
        const layout = panelLayout(80, height, wanted, band);
        assert.equal(
          layout.transcriptRows + layout.hintRows + layout.inputRows + layout.detailRows,
          layout.body,
          `height ${height}, input ${wanted}, band ${band}`,
        );
        assert.ok(layout.transcriptRows >= 0);
        assert.ok(layout.inputRows >= 0);
      }
    }
  }
});

test("the input block grows one row per line break", () => {
  assert.equal(inputBlockRows(0), MIN_BLOCK_ROWS);
  assert.equal(inputBlockRows(3), MIN_BLOCK_ROWS + 3);
});

test("a window smaller than the panel draws no panel", () => {
  assert.equal(panelFits(4, 24), false);
  assert.equal(panelFits(80, 4), false);
  assert.equal(panelFits(80, 24), true);
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
