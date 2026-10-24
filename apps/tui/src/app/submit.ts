//! What one submitted line asks for, before any effect is chosen.
//!
//! Parsing a line is pure: a prompt becomes a turn, a known command names its
//! transition, and a line that resolves to nothing keeps the notice that says
//! why. The state maps this outcome to its surfaces and its effects, so the
//! command grammar never grows a branch that only a terminal can explain.

import { THEME_PREFERENCES, type ThemePreference } from "./action";
import { isCommandLine, notice, resolveCommand, themeValue } from "./commands";

/** What one submitted line asks the state to do. */
export type Submission =
  /** Send the line as a prompt turn. */
  | { readonly kind: "prompt"; readonly text: string }
  /** Show a line that explains why the command did nothing. */
  | { readonly kind: "notice"; readonly text: string }
  /** Leave the front end. */
  | { readonly kind: "quit" }
  /** Create a session with no first turn. */
  | { readonly kind: "new_session" }
  /** Open the session browser. */
  | { readonly kind: "list_sessions" }
  /** Open the theme picker on the preference in force. */
  | { readonly kind: "theme_picker" }
  /** Persist one theme preference. */
  | { readonly kind: "theme"; readonly preference: ThemePreference };

/** Returns what one non-empty line asks for. */
export function resolveSubmission(text: string): Submission {
  const line = text.trim();
  if (!isCommandLine(line)) {
    return { kind: "prompt", text };
  }
  const resolution = resolveCommand(line.slice(1));
  if (resolution.kind !== "invocation") {
    return { kind: "notice", text: notice(resolution) ?? "unknown command" };
  }
  const { spec, words } = resolution.invocation;
  switch (spec.action) {
    case "quit":
      return { kind: "quit" };
    case "new_session":
      return { kind: "new_session" };
    case "list_sessions":
      return { kind: "list_sessions" };
    case "choose_theme": {
      const word = words[0];
      if (word === undefined) {
        return { kind: "theme_picker" };
      }
      const preference = themeValue(word);
      if (preference === undefined) {
        return {
          kind: "notice",
          text: `unknown theme "${word}"; known themes: ${THEME_PREFERENCES.join(" ")}`,
        };
      }
      return { kind: "theme", preference };
    }
  }
}
