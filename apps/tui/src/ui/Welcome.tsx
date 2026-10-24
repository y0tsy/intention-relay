//! The welcome page: the open Atrium launch surface.
//!
//! A launch is an open page, not a framed card: one header row carries the
//! product and the connection, the wordmark sits high on the page, the promise
//! follows it, and three command cards wait above the composer. The page is the
//! transcript's region when nothing has printed yet, so the header, the cards,
//! and the composer never move the input or the detail line.
//!
//! The wordmark is the tracked fullwidth pair, and the command cards collapse to
//! one hint row when the window is narrow. The page keeps its orientation down
//! to the smallest supported window.

import { TextAttributes } from "@opentui/core";
import { selectConnection } from "../app/select";
import type { AppStore } from "../app/store";
import {
  Center,
  Columns,
  Line,
  Pane,
  type Run,
  Runs,
  SlotRows,
  Spacer,
  useContentColumns,
  useSlotRows,
} from "../kit";
import type { Palette } from "../theme/palette";
import { LOGO_STAR, SEPARATOR } from "./glyphs";
import { connectionStatus } from "./status-phrases";
import { useStore } from "./use-store";

/** The product word of the lockup. */
const PRODUCT = "ＩＮＴＥＮＴＩＯＮ";

/** The trailing word of the lockup. */
const TRAILING = "ＲＥＬＡＹ";

/** The one-line promise above the commands. */
const TAGLINE = "A local agent for the long run.";

/** The quiet line that tells the reader what the binary is. */
const MICRO = "local-first · single-user · no cloud";

/** The note an empty page shows under its commands. */
const NO_SESSION = "No session yet. Type a prompt, or pick a command.";

/** The note a narrow page shows instead of the full sentence. */
const NO_SESSION_SHORT = "No session yet.";

/** The rows one command card occupies. */
const CARD_ROWS = 6;

/** The version this application is built as. */
export const VERSION = "0.0.0";

/** The blank runs one wordmark gap can use. */
const LOGO_GAPS = ["", " ", "  ", "   "] as const;

/**
 * Returns one word written with blank columns between its letters.
 *
 * The gap grows with the page, so a wide window gives the wordmark the presence
 * the reference gives it without a second font size.
 */
function tracked(word: string, gap: number): string {
  return [...word].join(LOGO_GAPS[gap] ?? LOGO_GAPS[1]);
}

/** Returns the tracking the page width gives the wordmark. */
function logoGap(columns: number): number {
  return columns >= 150 ? 3 : columns >= 60 ? 2 : 1;
}

/** Renders the page header: the product, the connection, and the version. */
export function WelcomeHeader({ store, palette }: { store: AppStore; palette: Palette }) {
  const connection = useStore(store, selectConnection);
  const columns = useContentColumns();
  const status = connectionStatus(connection);
  const runs: Run[] = [
    { text: "● ", ink: palette.accent },
    { text: status.text, ink: palette[status.ink] },
  ];
  if (columns >= 50) {
    runs.push({ text: `   v${VERSION}`, ink: palette.inkFaint });
  }
  return (
    <Line>
      <text fg={palette.accentDeep} attributes={TextAttributes.BOLD}>
        INTENTION RELAY
      </text>
      <Spacer grow />
      <Runs runs={runs} />
    </Line>
  );
}

/** Renders the tracked wordmark: INTENTION over the starred RELAY. */
function Lockup({ palette }: { palette: Palette }) {
  const gap = logoGap(useContentColumns());
  const trailing: Run[] = [
    { text: `${LOGO_STAR} `, ink: palette.accent },
    { text: tracked(TRAILING, gap), ink: palette.inkMuted },
    { text: ` ${LOGO_STAR}`, ink: palette.accent },
  ];
  return (
    <>
      <Center>
        <text fg={palette.accentDeep} attributes={TextAttributes.BOLD} wrapMode="none">
          {tracked(PRODUCT, gap)}
        </text>
      </Center>
      <Center>
        <Runs runs={trailing} />
      </Center>
    </>
  );
}

/** Returns the commands the compact hint row offers at one content width. */
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

/** Renders the compact command row at the columns the page has. */
function ActionHints({ palette }: { palette: Palette }) {
  return <Runs runs={actionRuns(palette, useContentColumns())} />;
}

/** Renders one framed command card. */
function CommandCard({
  title,
  detail,
  index,
  palette,
}: {
  title: string;
  detail: string;
  index: string;
  palette: Palette;
}) {
  return (
    <SlotRows rows={CARD_ROWS}>
      <Pane fill borderColor={palette.border} background={palette.panel}>
        <Line>
          <text fg={palette.accentDeep} attributes={TextAttributes.BOLD}>
            {title}
          </text>
          <Spacer grow />
          <text fg={palette.inkFaint}>{index}</text>
        </Line>
        <Spacer rows={1} />
        <text wrapMode="none" fg={palette.inkMuted}>
          {detail}
        </text>
      </Pane>
    </SlotRows>
  );
}

/** Renders the three command cards in equal columns. */
function CommandCards({ palette }: { palette: Palette }) {
  return (
    <Columns count={3} limit={46} align="center">
      <CommandCard title="/new" detail="start a fresh session" index="01" palette={palette} />
      <CommandCard title="/sessions" detail="browse this machine" index="02" palette={palette} />
      <CommandCard title="/theme" detail="choose the palette" index="03" palette={palette} />
    </Columns>
  );
}

/** Renders the welcome page over the rows of the transcript region. */
export function Welcome({ palette }: { palette: Palette }) {
  const rows = useSlotRows();
  const columns = useContentColumns();
  if (rows < 1) {
    return null;
  }
  if (rows < 4) {
    return (
      <Center>
        <text wrapMode="none" fg={palette.ink}>
          {TAGLINE}
        </text>
      </Center>
    );
  }
  const cards = columns >= 120 && rows >= 16;
  const micro = columns >= 40 && rows >= 9;
  const commands = !cards && rows >= 5;
  const note = rows >= 6;
  const topGap = rows >= 30 ? 9 : rows >= 16 ? 2 : rows >= 10 ? 1 : 0;
  const groupGap = cards ? (rows >= 30 ? 5 : 1) : rows >= 12 ? 2 : 1;
  const noteGap = cards || rows >= 10 ? 1 : 0;
  return (
    <>
      {topGap > 0 ? <Spacer rows={topGap} /> : null}
      <Lockup palette={palette} />
      <Center>
        <text wrapMode="none" fg={palette.ink}>
          {TAGLINE}
        </text>
      </Center>
      {micro ? (
        <Center>
          <text wrapMode="none" fg={palette.inkFaint}>
            {MICRO}
          </text>
        </Center>
      ) : null}
      {cards || commands ? <Spacer rows={groupGap} /> : null}
      {cards ? (
        <CommandCards palette={palette} />
      ) : commands ? (
        <Center>
          <ActionHints palette={palette} />
        </Center>
      ) : null}
      {noteGap > 0 ? <Spacer rows={noteGap} /> : null}
      {note ? (
        <Center>
          <text wrapMode="none" fg={palette.inkFaint}>
            {columns >= 52 ? NO_SESSION : NO_SESSION_SHORT}
          </text>
        </Center>
      ) : null}
      <Spacer grow />
    </>
  );
}
