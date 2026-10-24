//! The layout bricks: the numbers they hand their content and the rows they draw.

import assert from "node:assert/strict";
import test from "node:test";
import { frameLines, painted } from "../testing/render";
import { DARK } from "../theme/palette";
import { ContentColumns } from "./layout/context";
import { Hero } from "./layout/Hero";
import { heroLayout } from "./layout/hero-plan";
import { Lines } from "./layout/Lines";
import { bandSlotRows } from "./layout/menu-band";
import { Pane } from "./layout/Pane";
import { Rule } from "./layout/Rule";
import { scrollWindow } from "./layout/ScrollWindow";
import { Table } from "./layout/Table";
import { inputSlotRows, TextField } from "./layout/TextField";

/** Renders one element at one size and returns its lines. */
async function lines(element: React.ReactNode, width: number, height: number): Promise<string[]> {
  return frameLines(await painted(element, width, height));
}

test("a rule spans the region it sits in", async () => {
  const painted_ = await lines(<Rule color={DARK.inkFaint} />, 20, 1);
  assert.equal(painted_[0], "─".repeat(20));
});

test("a pane hands its content the columns its chrome leaves", async () => {
  const painted_ = await lines(
    <ContentColumns columns={12}>
      <Pane borderColor={DARK.border} background={DARK.panel}>
        <Lines text="one two three four five" rows={3} ink={DARK.ink} />
      </Pane>
    </ContentColumns>,
    12,
    5,
  );
  assert.ok(painted_.length <= 5);
  for (const line of painted_) {
    assert.ok(line.length <= 12, `"${line}" fits the pane`);
  }
});

test("a pane too narrow for its frame shows its bare rows", async () => {
  const painted_ = await lines(
    <ContentColumns columns={4}>
      <Pane borderColor={DARK.border} fallbackBare>
        <text>hi</text>
      </Pane>
    </ContentColumns>,
    4,
    1,
  );
  assert.equal(painted_[0], "hi");
});

test("lines bound their rows and name the rows they hid", async () => {
  const text = Array.from({ length: 8 }, (_, index) => `row ${index + 1}`).join("\n");
  const painted_ = await lines(
    <ContentColumns columns={20}>
      <Lines
        text={text}
        rows={3}
        ink={DARK.ink}
        overflow={(hidden) => <text>… {hidden} more</text>}
      />
    </ContentColumns>,
    20,
    4,
  );
  assert.deepEqual(painted_, ["row 1", "row 2", "row 3", "… 5 more"]);
});

test("lines give a rail its own columns", async () => {
  const painted_ = await lines(
    <ContentColumns columns={10}>
      <Lines text="abcdefghij" rows={3} ink={DARK.ink} rail="‣" />
    </ContentColumns>,
    10,
    3,
  );
  assert.deepEqual(painted_, ["‣ abcdefgh", "‣ ij"]);
});

test("the band takes its frame and its overflow row", () => {
  assert.equal(bandSlotRows(null), 0);
  assert.equal(bandSlotRows(0), 0);
  assert.equal(bandSlotRows(3), 5, "two frame rows and three rows");
  assert.equal(bandSlotRows(6), 6, "two frame rows, three rows, and the overflow");
});

test("the table fits its columns and hides the narrow ones whole", async () => {
  const rows = [
    {
      key: "one",
      selected: true,
      cells: { label: "/theme", description: "pick the theme", trailing: "Navigation" },
    },
  ];
  const wide = await lines(
    <Table
      columns={[
        { key: "label", grow: true },
        { key: "description", grow: true, hideOrder: 1 },
        { key: "trailing", hideOrder: 2 },
      ]}
      marker={{ selected: "> ", row: "  " }}
      markerGap={0}
      width={40}
      renderCell={() => ({ ink: DARK.ink })}
      rows={rows}
    />,
    40,
    1,
  );
  assert.match(wide[0] ?? "", /^> \/theme pick the theme/);

  const narrow = await lines(
    <Table
      columns={[
        { key: "label", grow: true },
        { key: "description", grow: true, hideOrder: 1 },
        { key: "trailing", hideOrder: 2 },
      ]}
      marker={{ selected: "> ", row: "  " }}
      markerGap={0}
      width={12}
      renderCell={() => ({ ink: DARK.ink })}
      rows={rows}
    />,
    12,
    1,
  );
  assert.match(narrow[0] ?? "", /^> \/theme/);
  assert.equal(narrow[0]?.includes("pick"), false, "the description yields first");
});

test("the table pads fixed cells to their own width", async () => {
  const painted_ = await lines(
    <Table
      columns={[
        { key: "a", width: 4 },
        { key: "b", width: 4 },
      ]}
      rows={[{ key: "r", cells: { a: "x", b: "yy" } }]}
      width={10}
      renderCell={() => ({ ink: DARK.ink })}
    />,
    10,
    1,
  );
  assert.match(painted_[0] ?? "", /^x {4}yy/);
});

test("the scroll window keeps the selected row visible", () => {
  assert.deepEqual(scrollWindow({ count: 6, visible: 3, selected: 3 }), {
    start: 1,
    end: 4,
    first: 2,
    last: 4,
  });
  assert.deepEqual(scrollWindow({ count: 0, visible: 3, selected: 0 }), {
    start: 0,
    end: 1,
    first: 0,
    last: 0,
  });
});

test("the input slot counts its chrome and one row per line", () => {
  assert.equal(inputSlotRows(""), 5);
  assert.equal(inputSlotRows("a\nb"), 6);
});

test("the text field windows its lines around the caret", async () => {
  const painted_ = await lines(
    <ContentColumns columns={20}>
      <TextField text={"one\ntwo\nthree"} caret={13} rows={5} palette={DARK} />
    </ContentColumns>,
    20,
    3,
  );
  assert.deepEqual(painted_, ["three█"]);
  assert.equal(painted_.length, 1, "the slot leaves one visible line");
});

test("the hero pins the overview and drops the lockup first", () => {
  assert.deepEqual(heroLayout(10), { lead: 1, hero: 1, filler: 5, overview: 3 });
  assert.deepEqual(heroLayout(4), { lead: 0, hero: 1, filler: 0, overview: 3 });
  assert.deepEqual(heroLayout(2), { lead: 0, hero: 0, filler: 0, overview: 2 });
  assert.deepEqual(heroLayout(0), { lead: 0, hero: 0, filler: 0, overview: 0 });
});

test("the hero renders the lockup over the pinned overview", async () => {
  const painted_ = await lines(
    <Hero
      rows={6}
      lockup={<text>lockup</text>}
      overview={(rows) => (
        <box height={rows} borderStyle="rounded" border>
          <text>version</text>
        </box>
      )}
    />,
    12,
    6,
  );
  assert.equal(painted_[1], "lockup");
  assert.equal(painted_[2], "");
  assert.match(painted_[3] ?? "", /^╭/);
});
