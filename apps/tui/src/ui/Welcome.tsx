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
//! letter in the muted ink. The pair is centred by the layout engine, so the
//! pane measures nothing.

import { TextAttributes } from "@opentui/core";
import { Badge, Hero, Pane, Row, useSlotRows } from "../kit";
import type { Palette } from "../theme/palette";

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

/** Renders the lockup: the product word, the gap, and the tracked trailing word. */
function Lockup({ palette }: { palette: Palette }) {
  return (
    <Row height={1} justify="center" gap={LOGO_GAP}>
      <text fg={palette.accentDeep} attributes={TextAttributes.BOLD}>
        {PRODUCT}
      </text>
      <text fg={palette.inkMuted}>{tracked(TRAILING)}</text>
    </Row>
  );
}

/** Renders the framed overview: one row of the sub-blocks the core can fill. */
function Overview({ rows, palette }: { rows: number; palette: Palette }) {
  return (
    <Pane
      height={rows}
      borderColor={palette.border}
      background={palette.panel}
      paddingLeft={1}
      paddingRight={1}
    >
      <Row height={1}>
        <Badge label={`Version ${VERSION}`} ink={palette.badgeInk} background={palette.badgeBg} />
      </Row>
    </Pane>
  );
}

/** Renders the welcome pane over the rows of the transcript region. */
export function Welcome({ palette }: { palette: Palette }) {
  const rows = useSlotRows();
  return (
    <Hero
      rows={rows}
      lockup={<Lockup palette={palette} />}
      overview={(rows) => <Overview rows={rows} palette={palette} />}
    />
  );
}
