//! The welcome pane: the product lockup over the way into the first session.
//!
//! The pane replaces the transcript's content in the same region, so the input
//! block, the detail line, and the panel geometry never move: the lockup sits at
//! the top of the region and the overview block is pinned to its last rows,
//! directly above the input block. A region too short for both drops the lockup
//! first, and a region with room for nothing draws nothing.
//!
//! A terminal has no font sizes, so the lockup approximates one: both words are
//! fullwidth Latin, tracked out one column per letter, with the product word
//! bold in the accent ink and the trailing word centred below it in the muted
//! ink between two stars. Every row is centred by the layout engine, so the pane
//! measures nothing. The overview says what the product is and which three
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
  useContentColumns,
  useSlotRows,
} from "../kit";
import type { Palette } from "../theme/palette";
import { LOGO_STAR, SEPARATOR } from "./glyphs";

/** The product word of the lockup. */
const PRODUCT = "ＩＮＴＥＮＴＩＯＮ";

/** The trailing word of the lockup. */
const TRAILING = "ＲＥＬＡＹ";

/** The rows the wordmark occupies: the product word over the trailing one. */
const LOCKUP_ROWS = 2;

/** The most rows the overview keeps before the wordmark takes the rest. */
const MAX_OVERVIEW_ROWS = 5;

/** The one-line promise above the commands. */
const TAGLINE = "A local agent for the long run.";

/** The version this application is built as. */
export const VERSION = "0.0.0";

/** Returns one word written with a blank column between its letters. */
function tracked(word: string): string {
  return [...word].join(" ");
}

/** Renders the lockup: the product word over the starred trailing word. */
function Lockup({ palette }: { palette: Palette }) {
  const trailing: Run[] = [
    { text: `${LOGO_STAR} `, ink: palette.accent },
    { text: tracked(TRAILING), ink: palette.inkMuted },
    { text: ` ${LOGO_STAR}`, ink: palette.accent },
  ];
  return (
    <>
      <Center>
        <text fg={palette.accentDeep} attributes={TextAttributes.BOLD} wrapMode="none">
          {tracked(PRODUCT)}
        </text>
      </Center>
      <Center>
        <Runs runs={trailing} />
      </Center>
    </>
  );
}

/**
 * Returns the overview rows the welcome keeps before the wordmark takes the rest.
 *
 * Once a small window has room for the wordmark and four overview rows, the
 * wordmark is painted; below that the region spends itself on orientation alone.
 */
function overviewReserve(rows: number): number {
  const wanted = rows >= LOCKUP_ROWS + 4 ? rows - LOCKUP_ROWS : rows;
  return wanted > MAX_OVERVIEW_ROWS ? MAX_OVERVIEW_ROWS : wanted;
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
      reserve={overviewReserve(rows)}
      lockupRows={LOCKUP_ROWS}
      lockup={<Lockup palette={palette} />}
      overview={(overviewRows) => <Overview rows={overviewRows} palette={palette} />}
    />
  );
}
