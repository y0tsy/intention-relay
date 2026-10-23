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
import { COMMANDS } from "./commands";
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

/** The rows a word opens, with the row the band cursors. */
export interface CommandMenu {
  /** Whether the rows rank commands or one argument's values. */
  readonly kind: "command" | "argument";
  /** The rows the word ranks, in the order the band paints them. */
  readonly rows: readonly MenuRow[];
  /** The index of the row the band cursors. */
  readonly highlight: number;
}

/** The most rows the band shows before it reports the rest. */
export const MAX_COMMAND_ROWS = 5;

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
 * A first word without `/` is a prompt, not a command, so the word opens
 * nothing; every other position belongs to the command the word already names.
 */
export function commandMenu(draft: Draft): CommandMenu | null {
  const before = draft.text.slice(0, draft.caret);
  const line = before.split("\n").at(-1) ?? "";
  const words = line.split(/\s+/);
  const word = words.at(-1) ?? "";
  if (word.startsWith("/")) {
    return word.length < 2 ? null : commandRows(word, words.length === 1);
  }
  // A second word belongs to the command the line already names: the word the
  // caret sits in ranks that command's values, never the registry's names.
  const command = words[0] ?? "";
  if (!command.startsWith("/") || words.length !== 2) {
    return null;
  }
  return argumentMenu(command, word);
}

/** Returns the commands one typed word ranks. */
function commandRows(word: string, first: boolean): CommandMenu | null {
  if (!first) {
    return null;
  }
  const filter = word.slice(1).toLowerCase();
  const rows = COMMANDS.filter((command) => command.name.startsWith(filter)).map((command) => ({
    label: `/${command.name}`,
    description: command.summary,
    trailing: command.category,
  }));
  if (rows.length === 0) {
    return null;
  }
  return { kind: "command", rows, highlight: exactOrFirst(rows, word) };
}

/** Returns the values one declared argument's word ranks, or `null`. */
function argumentMenu(command: string, filter: string): CommandMenu | null {
  if (command !== "/theme") {
    // Every other argument takes a free-form value: a session id has nothing
    // to rank, so the word opens nothing.
    return null;
  }
  const rows = THEME_PREFERENCES.filter((preference) =>
    preference.startsWith(filter.toLowerCase()),
  ).map((preference) => ({
    label: preference,
    description: THEME_DETAILS[preference],
    trailing: "theme",
  }));
  if (rows.length === 0) {
    return null;
  }
  return { kind: "argument", rows, highlight: exactOrFirst(rows, filter) };
}

/** Returns the row a word names exactly, or the first row, whichever it cursors. */
function exactOrFirst(rows: readonly MenuRow[], word: string): number {
  return Math.max(
    rows.findIndex((row) => row.label === word),
    0,
  );
}
