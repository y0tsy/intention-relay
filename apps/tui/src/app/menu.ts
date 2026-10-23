//! The input's command hint band: the commands the first word selects, or the
//! values of the argument word the caret sits in.
//!
//! The band opens while the caret is in a `/`-prefixed word: the registry
//! ranks the commands its filter names, and the one registered value-bearing
//! argument - the theme - ranks its declared values under the caret. A word
//! that names a session value stays free-form, so the band shows nothing there
//! and the panel shows the whole transcript. The description of one theme value
//! appears beside it; no such text invents itself.

import { THEME_PREFERENCES, type ThemePreference } from "./action";
import { COMMANDS, type CommandSpec } from "./commands";
import type { Draft } from "./editor";

/** One row the band shows: the cell it paints, its one line, and its third cell. */
export interface MenuRow {
  /** The first cell, exactly as the band paints it. */
  readonly label: string;
  /** The row's one-line description. */
  readonly description: string;
  /** The command's category or the argument's name. */
  readonly trailing: string;
}

/** The rows a word opens. */
export interface CommandMenu {
  /** Whether the rows rank commands or one argument's values. */
  readonly kind: "command" | "argument";
  /** The rows the word ranks, in the order the band paints them. */
  readonly rows: readonly MenuRow[];
}

/**
 * The rows the band shows at once.
 *
 * The band is a hint, not a menu to read: it shows the rows around the one the
 * reader is on and reports the rest, so a bare `/` never paints the whole
 * registry over the transcript.
 */
export const MAX_COMMAND_ROWS = 3;

/** What the picker and the theme values say about each preference. */
export const THEME_DETAILS: Record<ThemePreference, string> = {
  auto: "follow the terminal background",
  dark: "for a dark terminal",
  light: "for a light terminal",
};

/** Returns the rows the band occupies for one ranked row count, or `0` while closed. */
export function menuRows(count: number | null): number {
  if (count === null || count === 0) {
    return 0;
  }
  const shown = Math.min(count, MAX_COMMAND_ROWS);
  return 2 + shown + (count > MAX_COMMAND_ROWS ? 1 : 0);
}

/**
 * Returns the rows the word the caret sits in opens.
 *
 * A slash is a command only as the input's first character, so only the first
 * line can carry a word, and only while that line starts with `/`: a slash
 * anywhere else is ordinary text that opens no band. The command word ends at
 * the first blank of that line, and a caret inside it - a bare `/` included -
 * ranks the registry by what the user typed after the slash. Past that word,
 * the caret sits in the command's argument word, which only a command that
 * declares the argument answers: that is what reopens the band for `/theme `
 * and closes it for `/new `.
 */
export function commandMenu(draft: Draft): CommandMenu | null {
  const text = draft.text;
  if (!text.startsWith("/")) {
    return null;
  }
  const lineEnd = text.indexOf("\n");
  if (lineEnd >= 0 && draft.caret > lineEnd) {
    return null;
  }
  const line = lineEnd < 0 ? text : text.slice(0, lineEnd);
  const nameEnd = firstBlank(line);
  if (draft.caret <= nameEnd) {
    return commandRows(line.slice(1, nameEnd));
  }
  const command = COMMANDS.find((entry) => entry.name === line.slice(1, nameEnd));
  if (command === undefined || command.arguments.length === 0) {
    return null;
  }
  return argumentMenu(command, argumentWord(line.slice(nameEnd)));
}

/** Returns the offset of the first blank of one line, or its length. */
function firstBlank(line: string): number {
  const blank = line.search(/\s/);
  return blank < 0 ? line.length : blank;
}

/** Returns the word the caret's argument cell ranks. */
function argumentWord(rest: string): string {
  return rest.trim().split(/\s+/)[0] ?? "";
}

/** Returns the commands one typed filter ranks. */
function commandRows(filter: string): CommandMenu | null {
  const wanted = filter.toLowerCase();
  const rows = COMMANDS.filter((command) => command.name.startsWith(wanted)).map((command) => ({
    label: `/${command.name}`,
    description: command.summary,
    trailing: command.category,
  }));
  if (rows.length === 0) {
    return null;
  }
  return { kind: "command", rows };
}

/** Returns the values one declared argument's word ranks, or `null`. */
function argumentMenu(command: CommandSpec, filter: string): CommandMenu | null {
  if (command.action !== "choose_theme") {
    // Every other argument takes a free-form value: a session id has nothing
    // to rank, so the word opens nothing.
    return null;
  }
  const wanted = filter.toLowerCase();
  const rows = THEME_PREFERENCES.filter((preference) => preference.startsWith(wanted)).map(
    (preference) => ({
      label: preference,
      description: THEME_DETAILS[preference],
      trailing: "theme",
    }),
  );
  if (rows.length === 0) {
    return null;
  }
  return { kind: "argument", rows };
}

/** Returns the window of rows the band paints around one highlighted row. */
export function menuWindow(total: number, highlight: number): number {
  const shown = Math.min(total, MAX_COMMAND_ROWS);
  return Math.max(0, Math.min(highlight - shown + 1, total - shown));
}
