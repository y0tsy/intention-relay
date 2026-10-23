//! The palette the terminal app paints with.
//!
//! Roles, not colors: a view names what a piece of text is, and the palette
//! decides how that reads on the background the terminal actually reports. Both
//! variants keep every role above 4.5:1 against their own background, so a muted
//! label is dimmer than the body but never unreadable.

/** One resolved palette. */
export interface Palette {
  /** The panel background. */
  readonly background: string;
  /** The body text on that background. */
  readonly foreground: string;
  /** Secondary text: labels, timestamps, and hints. */
  readonly muted: string;
  /** Tertiary text: separators and disabled entries. */
  readonly faint: string;
  /** The one accent: the border of the surface that owns the keyboard. */
  readonly accent: string;
  /** Text that sits on the accent. */
  readonly onAccent: string;
  /** The border of a surface that does not own the keyboard. */
  readonly border: string;
  /** A committed user turn. */
  readonly user: string;
  /** A committed assistant answer. */
  readonly assistant: string;
  /** Tool evidence: names, calls, and results. */
  readonly tool: string;
  /** A typed failure. */
  readonly error: string;
  /** A condition worth a second look. */
  readonly warning: string;
  /** A completed run. */
  readonly success: string;
  /** Inline code and fenced blocks. */
  readonly code: string;
  /** A link target. */
  readonly link: string;
  /** A quoted passage. */
  readonly quote: string;
  /** Transient text a committed row has not replaced yet. */
  readonly provisional: string;
}

/** The palette for a terminal with a dark background. */
export const DARK: Palette = {
  background: "#12131a",
  foreground: "#e4e6ef",
  muted: "#9aa0b4",
  faint: "#808799",
  accent: "#8ab4ff",
  onAccent: "#0b0d13",
  border: "#606880",
  user: "#c9d4ff",
  assistant: "#e4e6ef",
  tool: "#a8d4c0",
  error: "#ff8b8b",
  warning: "#f0c674",
  success: "#9ad4a0",
  code: "#c9b6ff",
  link: "#8ab4ff",
  quote: "#9aa0b4",
  provisional: "#8f96ab",
};

/** The palette for a terminal with a light background. */
export const LIGHT: Palette = {
  background: "#fbfbfd",
  foreground: "#1b1d26",
  muted: "#5c6274",
  faint: "#6b7183",
  accent: "#2a5db0",
  onAccent: "#fbfbfd",
  border: "#8a90a4",
  user: "#2a3f78",
  assistant: "#1b1d26",
  tool: "#1e6b52",
  error: "#a8202a",
  warning: "#7a5200",
  success: "#1e6b3a",
  code: "#5b3d9c",
  link: "#2a5db0",
  quote: "#5c6274",
  provisional: "#4f5566",
};

/** Returns the palette one preference resolves to. */
export function paletteFor(preference: "light" | "dark"): Palette {
  return preference === "light" ? LIGHT : DARK;
}
