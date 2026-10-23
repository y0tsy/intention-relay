//! The input's command hint band: the commands the first word selects, or the
//! values of the argument word the caret sits in.
//!
//! The band is one region of the chat panel directly above the input block, and
//! the transcript gives up exactly the rows the band takes: the panel's body is
//! transcript plus band plus input plus detail, so nothing is painted over
//! anything and the picker's panel, once it opens, spends the same region.
//!
//! The band is its own rounded frame on the header wash, titled `commands` while
//! it ranks the registry and `arguments` while it ranks one declared argument's
//! values. The highlighted row carries the cursor marker and the selected wash,
//! and the first column keeps the accent ink the input line's own prompt uses.

import { TextAttributes } from "@opentui/core";
import { COMMANDS } from "../app/commands";
import type { CommandMenu, MenuRow } from "../app/menu";
import { MAX_COMMAND_ROWS } from "../app/menu";
import type { Palette } from "../theme/palette";
import { displayWidth } from "./measure";

/** The blank column between two band columns. */
const COLUMN_GAP = 1;

/** The columns the cursor marker takes, so every other row aligns with it. */
const MARKER_COLUMNS = 2;

/** The marker the highlighted row starts with. */
const CURSOR_MARKER = "> ";

/** The marker every other row starts with, keeping the columns aligned. */
const ROW_MARKER = "  ";

/** The widths of one band's columns. */
interface Columns {
  /** The first cell: a command name or an argument value. */
  readonly label: number;
  /** The description cell; `0` hides the column. */
  readonly description: number;
  /** The trailing cell: a category or an argument name; `0` hides it. */
  readonly trailing: number;
}

/** Returns the columns of one band inside `columns` available. */
function bandColumns(menu: CommandMenu, columns: number): Columns {
  const [labels, trailing] =
    menu.kind === "command"
      ? [allCommands(), COMMANDS.map((command) => command.category)]
      : [allValues(), ["theme"]];
  const label = maxWidth(labels);
  const trailingCell = maxWidth(trailing);
  const fixed = MARKER_COLUMNS + label + COLUMN_GAP + trailingCell;
  if (columns > fixed + COLUMN_GAP) {
    return { label, description: columns - fixed - COLUMN_GAP, trailing: trailingCell };
  }
  return { label: Math.max(0, columns - MARKER_COLUMNS), description: 0, trailing: 0 };
}

/** Returns the typed names of every registered command. */
function allCommands(): string[] {
  return COMMANDS.map((command) => `/${command.name}`);
}

/** Returns the declared values of the one argument the band ranks. */
function allValues(): string[] {
  return ["auto", "dark", "light"];
}

/** Returns the widest display width of a cell's words. */
function maxWidth(words: readonly string[]): number {
  return words.reduce((widest, word) => Math.max(widest, displayWidth(word)), 0);
}

/** Returns one padded cell of a band row. */
function cell(text: string, columns: number): string {
  return `${text}${" ".repeat(Math.max(0, columns - displayWidth(text)))}`;
}

/** Renders one command or value row. */
function BandRow({
  row,
  highlighted,
  columns,
  palette,
}: {
  row: MenuRow;
  highlighted: boolean;
  columns: Columns;
  palette: Palette;
}) {
  return (
    <text wrapMode="none" bg={highlighted ? palette.rowSelectedBg : palette.headerBg}>
      <span fg={palette.accent} attributes={highlighted ? TextAttributes.BOLD : undefined}>
        {highlighted ? CURSOR_MARKER : ROW_MARKER}
      </span>
      <span fg={palette.accentDeep} attributes={highlighted ? TextAttributes.BOLD : undefined}>
        {cell(row.label, columns.label)}
      </span>
      {columns.description > 0 ? (
        <span fg={palette.ink}>{` ${cell(row.description, columns.description)}`}</span>
      ) : null}
      {columns.trailing > 0 ? (
        <span fg={palette.inkMuted}>{` ${cell(row.trailing, columns.trailing)}`}</span>
      ) : null}
    </text>
  );
}

/** Renders the row that reports the rows the band does not show. */
function OverflowRow({
  hidden,
  noun,
  palette,
}: {
  hidden: number;
  noun: string;
  palette: Palette;
}) {
  return (
    <text wrapMode="none" bg={palette.headerBg}>
      <span fg={palette.headerBg}>{ROW_MARKER}</span>
      <span fg={palette.inkMuted}>{`… and ${hidden} more ${noun}`}</span>
    </text>
  );
}

/** Renders the band the word the caret sits in opens. */
export function HintBand({
  menu,
  palette,
  width,
}: {
  menu: CommandMenu;
  palette: Palette;
  width: number;
}) {
  const columns = bandColumns(menu, width - 4);
  const shown = menu.rows.slice(0, MAX_COMMAND_ROWS);
  const hidden = menu.rows.length - shown.length;
  const title = menu.kind === "command" ? "commands" : "arguments";
  const noun = menu.kind === "command" ? "commands" : "values";
  return (
    <box
      flexDirection="column"
      border
      borderStyle="rounded"
      borderColor={palette.accent}
      backgroundColor={palette.headerBg}
      title={title}
      titleColor={palette.accentDeep}
      titleAlignment="left"
      paddingLeft={1}
      paddingRight={1}
    >
      {shown.map((row, index) => (
        <BandRow
          key={row.label}
          row={row}
          highlighted={index === menu.highlight}
          columns={columns}
          palette={palette}
        />
      ))}
      {hidden > 0 ? <OverflowRow hidden={hidden} noun={noun} palette={palette} /> : null}
    </box>
  );
}
