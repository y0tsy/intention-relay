//! The footer's row budget: the regions inside the frame always sum to its body.

import assert from "node:assert/strict";
import test from "node:test";
import {
  bandColumnWidths,
  browserDataRows,
  composerSlotRows,
  CONTENT_INDENT,
  DETAIL_ROWS,
  EDGE_ROWS,
  footerPlan,
  INPUT_CHROME_ROWS,
  inputSlotRows,
  MIN_BLOCK_ROWS,
  printRoom,
  SESSION_CHROME_ROWS,
  SESSION_MARGIN,
  TRANSCRIPT_INDENT,
  WELCOME_CHROME_ROWS,
  heroLayout,
} from "./index";

/** The rows the theme picker's panel asks for. */
const PICKER_ROWS = 5;

test("an open session keeps an edge for the step in flight above the input", () => {
  // A full window: the frame keeps its own working rows and takes no slack.
  const one = footerPlan(80, 24, MIN_BLOCK_ROWS, 0, "session", 0, 24);
  assert.equal(one.detailRows, DETAIL_ROWS);
  assert.equal(one.chromeRows, SESSION_CHROME_ROWS, "the footer closes the frame and its margin");
  assert.equal(one.hintRows, 0, "a closed band takes no row");
  assert.equal(one.liveRows, EDGE_ROWS, "the step's edge is the window's, not the run's");
  assert.equal(one.inputRows, MIN_BLOCK_ROWS, "a one-line buffer keeps its rows");
  assert.equal(one.rows, one.body, "the footer is the body it spends");

  const two = footerPlan(80, 24, MIN_BLOCK_ROWS + 1, 0, "session", 0, 24);
  assert.equal(two.inputRows, MIN_BLOCK_ROWS + 1, "the block takes one row per buffer line");
  assert.equal(two.rows - one.rows, 1, "the footer grows with the row the line added");

  const tall = footerPlan(80, 24, 100, 0, "session", 0, 24);
  // The share is a third of the room the frame, chrome, and detail line leave.
  const room = 24 - SESSION_CHROME_ROWS - DETAIL_ROWS;
  assert.equal(tall.inputRows, Math.floor(room / 3) + INPUT_CHROME_ROWS);
  assert.equal(
    tall.body,
    tall.chromeRows + tall.inputRows + tall.detailRows + tall.liveRows,
    "the frame and its margin are part of what the footer spends",
  );
});

test("the footer keeps its own rows, whatever has been printed", () => {
  // Streaming never moves the window's own rows: the edge keeps its working
  // rows, and the page above the tray is the terminal's.
  const footer = footerPlan(80, 30, MIN_BLOCK_ROWS, 0, "session", 0, 30);
  assert.equal(footer.liveRows, EDGE_ROWS, "the step's edge is never the print's");
  assert.equal(footer.rows, footer.body, "a full window leaves the footer its body");
  assert.equal(footer.slackRows, 0);
});

test("the footer asks for the rows it spends and takes the rest from the page", () => {
  // The renderer reserves exactly the rows the plan asks for: the footer's own
  // working rows plus the page's slack. Asking for less paints past the region;
  // asking for more scrolls a printed row out of the terminal.
  for (const height of [14, 24, 40]) {
    for (const printed of [0, 1, 7, height, height + 8]) {
      const plan = footerPlan(80, height, MIN_BLOCK_ROWS, 0, "session", 0, printed);
      assert.ok(plan.rows >= plan.body, `${height}/${printed}: the rows cover the body`);
      assert.equal(
        plan.rows,
        plan.body + plan.slackRows,
        `${height}/${printed}: the slack is the difference`,
      );
    }
  }
});

test("a printed row takes the page's slack and nothing the footer spends", () => {
  const full = footerPlan(80, 24, MIN_BLOCK_ROWS, 0, "session", 0, 0);
  const later = footerPlan(80, 24, MIN_BLOCK_ROWS, 0, "session", 0, 12);
  assert.equal(later.body, full.body, "the footer's working rows do not follow the print");
  assert.equal(
    full.slackRows - later.slackRows,
    full.rows - later.rows,
    "the rows the print took are the rows the page gave back",
  );
});

test("a short print leaves the window above the footer to the page", () => {
  const empty = footerPlan(80, 30, MIN_BLOCK_ROWS, 0, "session", 0, 0);
  assert.equal(empty.rows, 30, "the window still reaches the terminal's last row");
  assert.equal(empty.liveRows, EDGE_ROWS, "the edge is the step's, never the slack's");
  assert.equal(empty.slackRows, 30 - empty.body, "the frame keeps only the rows it spends");

  const half = footerPlan(80, 30, MIN_BLOCK_ROWS, 0, "session", 0, 12);
  assert.equal(half.rows, 18, "the window ends where the print begins");
  assert.equal(half.slackRows, 18 - half.body);
});

test("a panel takes no row a print wrote", () => {
  // Growing the footer scrolls its region, and the rows that scroll leave the
  // scrollback for good. A panel opens into the rows a print left, or shows as
  // much of itself as the footer already has.
  const session = footerPlan(80, 30, MIN_BLOCK_ROWS, 0, "session", 0, 30);
  const band = footerPlan(80, 30, MIN_BLOCK_ROWS, PICKER_ROWS, "session", 0, 30, session.rows);
  assert.equal(band.rows, session.rows, "the band opens inside the rows the footer has");
  assert.equal(band.hintRows, PICKER_ROWS, "the band still takes its rows out of the body");
  assert.equal(band.slackRows, 0, "a printed window hands no slack back");

  const docked = footerPlan(80, 30, MIN_BLOCK_ROWS, 0, "session", 12, 30, session.rows);
  assert.equal(docked.rows, session.rows, "a docked surface takes no row either");
  assert.equal(docked.dockedRows, 12, "the plan still reports the rows the surface wants");

  const free = footerPlan(80, 30, MIN_BLOCK_ROWS, PICKER_ROWS, "session", 0, 0, 30);
  assert.equal(free.rows, 30, "a page with nothing above it still takes the window");
  assert.equal(free.slackRows, 30 - free.body, "the band comes out of the page's slack");
});

test("a step in flight keeps the edge it was given", () => {
  const live = footerPlan(80, 40, MIN_BLOCK_ROWS, 0, "session", 0, 40);
  assert.equal(live.liveRows, EDGE_ROWS, "the edge is the rows the printer withholds");
  assert.equal(live.inputRows, MIN_BLOCK_ROWS, "the input keeps its rows");
  assert.equal(
    live.rows,
    live.chromeRows + live.liveRows + live.inputRows + live.detailRows,
    "the footer reserves exactly what it paints",
  );

  const small = footerPlan(80, 12, MIN_BLOCK_ROWS, 0, "session", 0, 12);
  assert.ok(small.liveRows <= EDGE_ROWS, "a short window never grows the edge");
  assert.ok(small.rows <= 12);
});

test("the welcome pane fills the window above the input block", () => {
  const welcome = footerPlan(80, 30, MIN_BLOCK_ROWS, 0, "welcome", 0);
  assert.equal(welcome.rows, 30, "a launch paints the whole window");
  assert.equal(
    welcome.chromeRows,
    WELCOME_CHROME_ROWS,
    "the header and the margin under the welcome are its chrome",
  );
  assert.equal(
    welcome.liveRows,
    30 - WELCOME_CHROME_ROWS - welcome.inputRows - welcome.detailRows,
    "the welcome takes every row the frame, the input, and the detail line leave",
  );
});

test("the band is the live region's loss, never the input's", () => {
  const two = footerPlan(80, 24, MIN_BLOCK_ROWS + 1, 0, "session", 0, 24);
  const menu = footerPlan(80, 24, MIN_BLOCK_ROWS + 1, 8, "session", 0, 24);
  assert.equal(menu.hintRows, 8, "the band takes exactly the rows it asks");
  assert.equal(menu.inputRows, two.inputRows, "the input keeps its rows");
  assert.equal(menu.rows, two.rows + 8, "the footer grows by the band's rows");

  const picker = footerPlan(80, 40, MIN_BLOCK_ROWS, PICKER_ROWS, "session", 0, 40);
  assert.equal(picker.hintRows, PICKER_ROWS);
  assert.equal(picker.inputRows, MIN_BLOCK_ROWS);
});

test("a body too short for the band keeps the input and shows no band", () => {
  const cramped = footerPlan(80, 10, MIN_BLOCK_ROWS + 1, 4, "session");
  assert.equal(cramped.hintRows, 0, "the band yields to the input block");
  assert.equal(cramped.inputRows, MIN_BLOCK_ROWS + 1, "the input keeps every row it asked for");
  assert.equal(
    cramped.body,
    cramped.chromeRows +
      cramped.hintRows +
      cramped.inputRows +
      cramped.detailRows +
      cramped.liveRows,
  );

  const taller = footerPlan(80, 10, MIN_BLOCK_ROWS, PICKER_ROWS, "session");
  assert.equal(taller.hintRows, 0, "the picker yields too");
  assert.equal(taller.inputRows, MIN_BLOCK_ROWS);
});

test("every window spends its body exactly and never overflows", () => {
  for (const height of [3, 5, 8, 12, 20, 24, 30, 40, 60]) {
    for (const wanted of [0, 1, MIN_BLOCK_ROWS, MIN_BLOCK_ROWS + 2, 100]) {
      for (const band of [0, 1, 5, 8]) {
        for (const mode of ["session", "welcome"] as const) {
          // A print takes its rows above the footer, one row of the window per
          // row of the page.
          for (const docked of [0, 1, 2, 12]) {
            for (const printed of [0, 1, 7, height, 1000]) {
              const layout = footerPlan(80, height, wanted, band, mode, docked, printed);
              assert.equal(
                layout.chromeRows +
                  layout.liveRows +
                  layout.hintRows +
                  layout.inputRows +
                  layout.detailRows,
                layout.body,
                `height ${height}, input ${wanted}, band ${band}, ${mode}`,
              );
              assert.ok(layout.rows <= height, "the footer never asks for more than the window");
              assert.ok(layout.liveRows >= 0);
              assert.ok(layout.inputRows >= 0);
              // The window ends where the print begins, and the frame inside it
              // keeps only the rows it spends.
              const reached = Math.min(printed, height);
              if (reached + layout.rows <= height) {
                assert.equal(
                  reached + layout.rows,
                  height,
                  `height ${height}, printed ${printed}, docked ${docked}, ${mode}`,
                );
              }
            }
          }
        }
      }
    }
  }
});

test("the printed transcript starts at the footer's content column", () => {
  assert.equal(CONTENT_INDENT, 3, "the welcome page keeps its own margin");
  assert.equal(TRANSCRIPT_INDENT, 5);
  const session = footerPlan(80, 24, MIN_BLOCK_ROWS, 0, "session", 0, 24);
  assert.equal(
    session.contentColumns,
    80 - TRANSCRIPT_INDENT * 2,
    "a printed row and the edge share one column",
  );
  assert.equal(
    session.columns,
    80 - SESSION_MARGIN * 2,
    "the tray's rail is the transcript's rule",
  );
  const welcome = footerPlan(80, 24, MIN_BLOCK_ROWS, 0, "welcome");
  assert.equal(welcome.contentColumns, 80 - CONTENT_INDENT * 2);
});

test("a print chunk fits the rows above the footer", () => {
  // The rows above the footer are the rows a print gave back: a chunk taller
  // than them scrolls the whole screen, and the footer it moves is never
  // painted again.
  assert.equal(printRoom(30, 14), 16);
  assert.equal(printRoom(24, 8), 16);
  assert.equal(printRoom(30, 30), 1, "a footer that is the whole window still prints a row");
  assert.equal(printRoom(24, 40), 1, "a footer taller than the window cannot stop the print");
});

test("the input block grows one row per line break", () => {
  assert.equal(inputSlotRows(""), MIN_BLOCK_ROWS);
  assert.equal(inputSlotRows("a\nb\nc\nd"), MIN_BLOCK_ROWS + 3);
  assert.equal(composerSlotRows(""), 3, "the bare composer is a frame and one line");
  assert.equal(composerSlotRows("a\nb"), 4, "and one row per buffer line after it");
});

test("the band gives its label and trailing cell their registry width", () => {
  const roomy = bandColumnWidths(50, ["/new", "/sessions"], ["Session", "Appearance"]);
  assert.deepEqual(roomy, { label: 9, trailing: 10, description: 27 });
  const narrow = bandColumnWidths(10, ["/new", "/sessions"], ["Session", "Appearance"]);
  assert.deepEqual(narrow, { label: 8, trailing: 0, description: 0 });
});

test("the docked browser shows the rows its panel has above its chrome", () => {
  assert.equal(browserDataRows(12), 2);
  assert.equal(browserDataRows(6), 1, "a panel at its floor still shows one row");
});

test("a window too short for the frame's edge spends no slot", () => {
  // The frame needs its bottom edge and the margin under it before it can hold a
  // row: two rows keep the region on the window's last row and paint no box.
  const two = footerPlan(40, 2, MIN_BLOCK_ROWS, 0, "session", 0, 0, 2);
  assert.equal(
    two.liveRows + two.hintRows + two.inputRows + two.detailRows,
    0,
    "two rows hold the frame's edge and no slot",
  );
  assert.equal(two.body, two.chromeRows, "the frame spends only its own chrome");
  assert.equal(two.rows, 2, "the region still reaches the window's last row");

  const one = footerPlan(40, 1, MIN_BLOCK_ROWS, 0, "session", 0, 0, 1);
  assert.equal(one.body, 0, "one row holds nothing at all");
  assert.equal(one.rows, 1);

  const welcome = footerPlan(40, 2, MIN_BLOCK_ROWS, 0, "welcome", 0, 0, 2);
  assert.equal(welcome.body, 0, "the welcome's bar needs its own row and the margin's");
});

test("no window, print, or ceiling makes the plan paint past its region", () => {
  // The frame, the slots, the docked surface, and the margin are the rows the
  // region spends, and every combination of a short window, a long print, a low
  // ceiling, and a panel has to leave them inside it.
  for (const height of [2, 4, 8, 12, 24, 40]) {
    for (const printed of [0, 3, 12, 24, 60]) {
      for (const maxRows of [height, Math.floor(height / 2), 3]) {
        for (const docked of [0, 1, 5]) {
          for (const hints of [0, PICKER_ROWS, 12]) {
            const plan = footerPlan(
              80,
              height,
              MIN_BLOCK_ROWS + 2,
              hints,
              "session",
              docked,
              printed,
              maxRows,
            );
            const at = `height=${height} printed=${printed} maxRows=${maxRows} docked=${docked} hints=${hints}`;
            assert.ok(
              plan.rows <= Math.min(maxRows, height),
              `the region fits its ceiling (${at})`,
            );
            assert.ok(plan.body <= plan.rows, `the frame fits the region (${at})`);
            assert.ok(plan.liveRows <= EDGE_ROWS, `the edge never grows (${at})`);
            assert.ok(
              plan.hintRows === 0 || plan.hintRows === hints,
              `the band is all or none (${at})`,
            );
            assert.equal(
              plan.slackRows,
              Math.max(0, plan.rows - plan.body - docked),
              `the slack is what the frame and the surface leave (${at})`,
            );
          }
        }
      }
    }
  }
});

test("the welcome keeps the overview before the lockup", () => {
  const reserved = heroLayout(10, 5);
  assert.deepEqual(reserved, { lead: 1, hero: 1, filler: 3, overview: 5 });
  const wordmark = heroLayout(10, 5, 2);
  assert.deepEqual(wordmark, { lead: 1, hero: 2, filler: 2, overview: 5 });
  const snug = heroLayout(6, 4, 2);
  assert.deepEqual(snug, { lead: 0, hero: 2, filler: 0, overview: 4 });
  const roomy = heroLayout(10);
  assert.deepEqual(roomy, { lead: 1, hero: 1, filler: 5, overview: 3 });
  const tight = heroLayout(4);
  assert.deepEqual(tight, { lead: 0, hero: 1, filler: 0, overview: 3 });
  const tiny = heroLayout(2);
  assert.deepEqual(tiny, { lead: 0, hero: 0, filler: 0, overview: 2 });
  const none = heroLayout(0);
  assert.deepEqual(none, { lead: 0, hero: 0, filler: 0, overview: 0 });
});
