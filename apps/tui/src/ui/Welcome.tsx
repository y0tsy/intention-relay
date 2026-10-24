//! The welcome pane: the product lockup over the way into the first session.
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
//! pane measures nothing. The overview says what the product is and which three
//! commands open the first session; a narrow region keeps the orientation and
//! drops the version before it clips an instruction.

import { TextAttributes } from "@opentui/core";
import {
  Badge,
  Center,
  Hero,
  Line,
  Pane,
  type Run,
  Runs,
  Spacer,
  useContentColumns,
  useSlotRows,
} from "../kit";
import type { Palette } from "../theme/palette";
import { SEPARATOR } from "./glyphs";

/** The product word of the lockup. */
const PRODUCT = "INTENTION";

/** The trailing word of the lockup. */
const TRAILING = "RELAY";

/** The columns the lockup keeps between its two words. */
const LOGO_GAP = 2;

/** The one-line promise above the commands. */
const TAGLINE = "A local agent for the long run.";

/** The version this application is built as. */
export const VERSION = "0.0.0";

/** Returns one word written with a blank column between its letters. */
function tracked(word: string): string {
  return [...word].join(" ");
}

/** Renders the lockup: the product word, the gap, and the tracked trailing word. */
function Lockup({ palette }: { palette: Palette }) {
  return (
    <Center>
      <text fg={palette.accentDeep} attributes={TextAttributes.BOLD}>
        {PRODUCT}
      </text>
      <Spacer columns={LOGO_GAP} />
      <text fg={palette.inkMuted}>{tracked(TRAILING)}</text>
    </Center>
  );
}

/** Returns the commands the overview offers at one content width. */
function actionRuns(palette: Palette, columns: number): Run[] {
  const runs: Run[] = [];
  const add = (command: string, action: string | null): void => {
    if (runs.length > 0) {
      runs.push({ text: SEPARATOR, ink: palette.inkFaint });
    }
    runs.push({ text: command, ink: palette.accentDeep });
    if (action !== null) {
      runs.push({ text: ` ${action}`, ink: palette.inkMuted });
    }
  };
  const full = columns >= 46;
  add("/new", full ? "start" : null);
  add("/sessions", full ? "browse" : null);
  add("/theme", full ? "palette" : null);
  return runs;
}

/** Renders the command row at the columns the overview has. */
function ActionHints({ palette }: { palette: Palette }) {
  return <Runs runs={actionRuns(palette, useContentColumns())} />;
}

/** Renders the framed overview: the promise, the commands, and the version. */
function Overview({ rows, palette }: { rows: number; palette: Palette }) {
  if (rows < 3) {
    // A window this short keeps the orientation bare: its two rows are worth
    // more as a sentence and a command than as the edges of a broken frame.
    return (
      <>
        <text wrapMode="none" fg={palette.ink}>
          {TAGLINE}
        </text>
        {rows > 1 ? <ActionHints palette={palette} /> : null}
      </>
    );
  }
  return (
    <Pane fill borderColor={palette.border} background={palette.panel}>
      <text wrapMode="none" fg={palette.ink}>
        {TAGLINE}
      </text>
      {rows >= 4 ? <ActionHints palette={palette} /> : null}
      {rows >= 5 ? (
        <Line>
          <Badge label={`Version ${VERSION}`} ink={palette.badgeInk} background={palette.badgeBg} />
        </Line>
      ) : null}
    </Pane>
  );
}

/** Renders the welcome pane over the rows of the transcript region. */
export function Welcome({ palette }: { palette: Palette }) {
  const rows = useSlotRows();
  return (
    <Hero
      rows={rows}
      reserve={5}
      lockup={<Lockup palette={palette} />}
      overview={(overviewRows) => <Overview rows={overviewRows} palette={palette} />}
    />
  );
}
