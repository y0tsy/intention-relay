//! The welcome pane: the product lockup over the version the binary was built as.
//!
//! The pane replaces the transcript's content in the same region, so the input
//! block, the detail line, and the panel geometry never move: the lockup sits at
//! the top of the region and the overview block is pinned to its last rows,
//! directly above the input block. A region too short for both drops the lockup
//! first, and a region with room for nothing draws nothing.
//!
//! A terminal has no font sizes, so the lockup approximates one: the product word
//! is bold in the accent ink and the trailing word is tracked out one column per
//! letter in the muted ink. Every column is measured through the terminal's own
//! width helper, so the pair centres however the letters render.

import { TextAttributes } from "@opentui/core";
import type { Palette } from "../theme/palette";
import { displayWidth } from "./measure";
import { welcomeLayout } from "./panel";

/** The product word of the lockup. */
const PRODUCT = "INTENTION";

/** The trailing word of the lockup. */
const TRAILING = "RELAY";

/** The columns the lockup keeps between its two words. */
const LOGO_GAP = 2;

/** The version this application is built as. */
export const VERSION = "0.0.0";

/** Returns one word written with a blank column between its letters. */
function tracked(word: string): string {
  return [...word].join(" ");
}

/** Returns the blank columns a centred run of `width` needs beside it. */
function pad(width: number): number {
  return Math.max(
    0,
    Math.floor((width - (displayWidth(PRODUCT) + LOGO_GAP + displayWidth(tracked(TRAILING)))) / 2),
  );
}

/** Renders the lockup: the product word, the gap, and the tracked trailing word. */
function Lockup({ width, palette }: { width: number; palette: Palette }) {
  return (
    <box flexDirection="row" height={1}>
      <text>{" ".repeat(pad(width))}</text>
      <text fg={palette.accentDeep} attributes={TextAttributes.BOLD}>
        {PRODUCT}
      </text>
      <text>{" ".repeat(LOGO_GAP)}</text>
      <text fg={palette.inkMuted}>{tracked(TRAILING)}</text>
    </box>
  );
}

/** Renders the framed overview: one row of the sub-blocks the core can fill. */
function Overview({ rows, width, palette }: { rows: number; width: number; palette: Palette }) {
  return (
    <box
      height={rows}
      width={width}
      flexDirection="column"
      borderStyle="rounded"
      border
      borderColor={palette.border}
      backgroundColor={palette.panel}
      paddingLeft={1}
      paddingRight={1}
    >
      <box flexDirection="row" height={1}>
        <text bg={palette.badgeBg} fg={palette.badgeInk}>
          {` Version ${VERSION} `}
        </text>
      </box>
    </box>
  );
}

/** Renders the welcome pane over the rows and columns of the transcript region. */
export function Welcome({
  rows,
  columns,
  palette,
}: {
  rows: number;
  columns: number;
  palette: Palette;
}) {
  const layout = welcomeLayout(rows);
  return (
    <box flexDirection="column" height={rows} width={columns}>
      <box height={layout.lead} />
      {layout.logo > 0 ? <Lockup width={columns} palette={palette} /> : null}
      <box height={layout.filler} />
      {layout.overview > 0 ? (
        <Overview rows={layout.overview} width={columns} palette={palette} />
      ) : null}
    </box>
  );
}
