//! The configuration file that carries the theme preference.
//!
//! The file is `~/.config/intention-relay/tui.toml` on every platform (the
//! daemon's own directory), and the app owns exactly one key of it: `theme` in
//! the `[tui]` section. Everything else in the file belongs to whoever wrote it,
//! so a write replaces that one line and leaves the rest byte for byte.

import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import type { ThemePreference } from "../app/action";

/** The section the app's key lives in. */
const SECTION = "[tui]";

/** The key the app reads and writes. */
const KEY = "theme";

/** The preferences this file may declare, in the order the picker lists them. */
const PREFERENCES: readonly ThemePreference[] = ["auto", "light", "dark"];

/**
 * Returns the configuration file path.
 *
 * `INTENTION_CONFIG_DIR` overrides the directory, which is what a test or a
 * second installation on one machine needs; without it the user's platform
 * configuration directory is used.
 */
export function configPath(env: NodeJS.ProcessEnv = process.env): string {
  const declared = env.INTENTION_CONFIG_DIR;
  if (declared !== undefined && declared.length > 0) {
    return join(declared, "tui.toml");
  }
  const home = env.HOME ?? env.USERPROFILE;
  if (home === undefined || home.length === 0) {
    return join(".", "tui.toml");
  }
  return join(home, ".config", "intention-relay", "tui.toml");
}

/**
 * Reads the declared theme preference.
 *
 * A file that is absent, unreadable, or spells a value this build does not know
 * reads as `auto`: the terminal's own background decides, which is the choice
 * that is never wrong.
 */
export async function readThemePreference(path: string = configPath()): Promise<ThemePreference> {
  let text: string;
  try {
    text = await readFile(path, "utf8");
  } catch {
    return "auto";
  }
  return parseThemePreference(text) ?? "auto";
}

/** Returns the preference one file body declares, or `null` when it declares none. */
export function parseThemePreference(text: string): ThemePreference | null {
  let inSection = false;
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (trimmed.length === 0 || trimmed.startsWith("#")) {
      continue;
    }
    if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
      inSection = trimmed === SECTION;
      continue;
    }
    if (!inSection) {
      continue;
    }
    const separator = trimmed.indexOf("=");
    if (separator < 0) {
      continue;
    }
    const key = trimmed.slice(0, separator).trim();
    if (key !== KEY) {
      continue;
    }
    const value = unquote(trimmed.slice(separator + 1).trim());
    return PREFERENCES.find((preference) => preference === value) ?? null;
  }
  return null;
}

/**
 * Writes one preference, replacing the declared line and nothing else.
 *
 * A file with no `[tui]` section gains one at the end; a file that already
 * declares the key has exactly that line rewritten, so comments and unrelated
 * keys survive a change of theme.
 */
export async function writeThemePreference(
  preference: ThemePreference,
  path: string = configPath(),
): Promise<void> {
  let text = "";
  try {
    text = await readFile(path, "utf8");
  } catch {
    text = "";
  }
  const rewritten = replaceThemeLine(text, preference);
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, rewritten, "utf8");
}

/** Returns the file body with its theme line replaced or appended. */
export function replaceThemeLine(text: string, preference: ThemePreference): string {
  const replacement = `${KEY} = "${preference}"`;
  const lines = text.length === 0 ? [] : text.split("\n");
  let inSection = false;
  for (const [index, line] of lines.entries()) {
    const trimmed = line.trim();
    if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
      inSection = trimmed === SECTION;
      continue;
    }
    if (!inSection) {
      continue;
    }
    const separator = trimmed.indexOf("=");
    if (separator >= 0 && trimmed.slice(0, separator).trim() === KEY) {
      lines[index] = replacement;
      return lines.join("\n");
    }
  }
  if (inSection) {
    // The section exists and has no theme line: the line joins it directly
    // after its last entry, so a trailing blank line never becomes an empty
    // entry in the section.
    while (lines.length > 0 && (lines.at(-1) ?? "").trim().length === 0) {
      lines.pop();
    }
    lines.push(replacement);
    return `${lines.join("\n")}\n`;
  }
  const body = text.length === 0 ? "" : `${text.replace(/\n+$/, "")}\n`;
  return `${body}${SECTION}\n${replacement}\n`;
}

/** Removes the quotes a TOML string may carry. */
function unquote(value: string): string {
  if (value.length >= 2 && (value[0] === '"' || value[0] === "'") && value.at(-1) === value[0]) {
    return value.slice(1, -1);
  }
  return value;
}
