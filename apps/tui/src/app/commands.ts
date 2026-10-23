//! The slash commands the fullscreen surface knows.
//!
//! A command exists exactly once, as an entry of [`COMMANDS`]: a typed line
//! resolves through [`resolveCommand`] and runs the transition its entry names,
//! and the unknown-command notice lists the same entries, so a command can never
//! exist in the registry and not in the surface it drives.

import { THEME_PREFERENCES, type ThemePreference } from "./action";

/** One argument a command declares. */
export interface ArgumentSpec {
  /** The argument's name, as the notices spell it. */
  readonly name: string;
  /** Whether a submitted line must spell the argument. */
  readonly required: boolean;
}

/** The transition one registered command runs. */
export type CommandAction =
  /** Create a session in the workspace and open it. */
  | "new_session"
  /** Browse the sessions the daemon holds. */
  | "list_sessions"
  /** Choose the palette, either through the picker or by one declared value. */
  | "choose_theme"
  /** Leave the front end. */
  | "quit";

/** One command the fullscreen surface knows. */
export interface CommandSpec {
  /** The command's name, spelled without its leading slash. */
  readonly name: string;
  /** The column the band names the command under: Session, Navigation, Appearance. */
  readonly category: string;
  /** The arguments the command declares, in the order a line fills them. */
  readonly arguments: readonly ArgumentSpec[];
  /** What the command does, as the welcome surface offers it. */
  readonly summary: string;
  /** The transition a submitted command of this name runs. */
  readonly action: CommandAction;
}

/** Every command the fullscreen surface knows, in the order a notice lists them. */
export const COMMANDS: readonly CommandSpec[] = [
  {
    name: "new",
    category: "Session",
    arguments: [],
    summary: "start a fresh session",
    action: "new_session",
  },
  {
    name: "sessions",
    category: "Navigation",
    arguments: [],
    summary: "browse the sessions on this machine",
    action: "list_sessions",
  },
  {
    name: "theme",
    category: "Appearance",
    // The value is optional: a bare `/theme` opens the picker, and a declared
    // value pins the palette in one word.
    arguments: [{ name: "theme", required: false }],
    summary: "choose the palette",
    action: "choose_theme",
  },
  {
    name: "quit",
    category: "Session",
    arguments: [],
    summary: "leave",
    action: "quit",
  },
];

/**
 * Returns the preference one declared word names.
 *
 * The case rule is the registry's: a value is matched without regard to case, so
 * `/theme DARK` and `/theme dark` name the same preference.
 */
export function themeValue(word: string): ThemePreference | undefined {
  const normalized = word.trim().toLowerCase();
  return THEME_PREFERENCES.find((value) => value === normalized);
}

/** One resolved command line: the registered command and its argument words. */
export interface CommandInvocation {
  /** The command the line named. */
  readonly spec: CommandSpec;
  /** The word the line gave each declared argument, in declaration order. */
  readonly words: readonly string[];
}

/** The outcome of resolving one submitted line through the registry. */
export type CommandResolution =
  | { readonly kind: "invocation"; readonly invocation: CommandInvocation }
  | { readonly kind: "unknown_name"; readonly name: string }
  | { readonly kind: "missing_argument"; readonly argument: ArgumentSpec }
  | { readonly kind: "unexpected_argument"; readonly value: string };

/** Returns the registered command one typed name selects, if any. */
export function commandNamed(name: string): CommandSpec | undefined {
  return COMMANDS.find((command) => command.name === name);
}

/** Returns the name as a user writes it, with its leading slash. */
export function typedName(command: { readonly name: string }): string {
  return `/${command.name}`;
}

/** Returns every registered command name as one line, in registry order. */
export function knownCommands(): string {
  return COMMANDS.map(typedName).join(" ");
}

/**
 * Returns the invocation one submitted command line spells, or the reason it
 * spells none.
 *
 * The line is the text after its leading slash, trimmed. The first word names the
 * command; every word after it fills one declared argument, in declaration
 * order. The parser reads only the registry, so a declaration added there is
 * parsed and explained here without another spelling of it anywhere.
 */
export function resolveCommand(line: string): CommandResolution {
  const words = line
    .trim()
    .split(/\s+/)
    .filter((word) => word.length > 0);
  const name = words.at(0) ?? "";
  const spec = commandNamed(name);
  if (spec === undefined) {
    return { kind: "unknown_name", name };
  }
  return resolveArguments(spec, words.slice(1));
}

/** Returns the resolution of one command's argument words. */
function resolveArguments(spec: CommandSpec, words: readonly string[]): CommandResolution {
  const given: string[] = [];
  for (const [index, argument] of spec.arguments.entries()) {
    const word = words[index];
    if (word === undefined) {
      if (argument.required) {
        return { kind: "missing_argument", argument };
      }
      continue;
    }
    given.push(word);
  }
  const unexpected = words[spec.arguments.length];
  if (unexpected !== undefined) {
    return { kind: "unexpected_argument", value: unexpected };
  }
  return { kind: "invocation", invocation: { spec, words: given } };
}

/**
 * Returns the one-line notice a resolution answers with, or `null` when the line
 * resolved to a command.
 *
 * Every notice is built from the same declaration the line was resolved
 * against: the unknown name lists the registry, and a missing argument names the
 * argument.
 */
export function notice(resolution: CommandResolution): string | null {
  switch (resolution.kind) {
    case "invocation":
      return null;
    case "unknown_name":
      return `unknown command /${resolution.name}; known commands: ${knownCommands()}`;
    case "missing_argument":
      return `missing argument "${resolution.argument.name}"`;
    case "unexpected_argument":
      return `unexpected argument "${resolution.value}"`;
  }
}

/** Reports whether one submitted line is a command rather than a prompt. */
export function isCommandLine(line: string): boolean {
  return line.startsWith("/");
}
