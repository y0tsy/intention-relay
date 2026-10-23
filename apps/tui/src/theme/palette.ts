//! The terminal app's colour system: roles, not colours.
//!
//! A view names what a piece of text or a wash is, and the palette decides how it
//! reads on the background the terminal reports. Both variants live here as full
//! role tables: the light theme is the warm off-white canvas, the dark theme a
//! neutral graphite ramp whose only hue is its accent pair.
//!
//! The rules hold per theme, and one holds across them: every role is fully
//! opaque, because a terminal shows no transparency; within one theme no two roles
//! share a value, the one documented alias being `error`, which spells `scarlet`
//! exactly; within one theme every role carries its own value, so a surface is
//! never mistaken for its neighbour; and every role differs between the two
//! themes, so a frame is wholly one theme and never a half-switched surface. Dark
//! is monochrome: every role but `accent` and `accent_deep` is a neutral grey, so
//! a state there reaches the reader through its word and glyph, never through a
//! colour of its own. Colours degrade, words do not: a terminal without truecolor
//! maps each role to its nearest ANSI colour, so no role carries a state alone.

function hex(red: number, green: number, blue: number): string {
  const pair = (value: number): string => value.toString(16).padStart(2, "0").toUpperCase();
  return `#${pair(red)}${pair(green)}${pair(blue)}`;
}

/** One resolved palette. Dark is monochrome except for its accent pair. */
export interface Palette {
  /** The window behind every panel. */
  readonly canvas: string;
  /** Card and panel fills. */
  readonly panel: string;
  /** The tab row and the table header band. */
  readonly headerBg: string;
  /** The wash behind the active tab. */
  readonly tabActiveBg: string;
  /** The wash behind the browser's cursor row. */
  readonly rowSelectedBg: string;
  /** The zebra tint of every second table row. */
  readonly rowAltBg: string;
  /** The wash behind an explanatory empty state the core cannot fill yet. */
  readonly stubBg: string;
  /** The wash behind the chat input line. */
  readonly inputBg: string;
  /** The wash behind one badge of the input header. */
  readonly badgeBg: string;
  /** The ink of a badge's label in the input header. */
  readonly badgeInk: string;
  /** The wash behind a committed user message's card. */
  readonly userSurface: string;
  /** The frame of a committed user message's card. */
  readonly userBorder: string;
  /** The `you` label ink of a committed user message's card. */
  readonly userLabel: string;
  /** The fill of the transcript's selected display rows. */
  readonly selectionBg: string;
  /** The wash behind one tool block. */
  readonly toolSurface: string;
  /** The frame of one tool block. */
  readonly toolBorder: string;
  /** The badge ink of one tool block's header. */
  readonly toolLabel: string;
  /** The parsed result ink of one tool block. */
  readonly toolPreview: string;
  /** The wash behind one daemon notice block. */
  readonly noticeSurface: string;
  /** The ink of one daemon notice block. */
  readonly noticeInk: string;
  /** The track of the transcript's native scrollbar. */
  readonly scrollTrack: string;
  /** The thumb of the transcript's native scrollbar. */
  readonly scrollThumb: string;
  /** The tint of a divider band between two blocks. */
  readonly dividerTint: string;
  /** The neutral frame of a chat panel or a table. */
  readonly border: string;
  /** The primary accent: the focused frame and every hotkey. */
  readonly accent: string;
  /** Accent text that has to stay readable on an accent wash. */
  readonly accentDeep: string;
  /** The secondary accent: a live run's marker and destructive hints. */
  readonly scarlet: string;
  /** The ink of an assistant heading line. */
  readonly markdownHeading: string;
  /** The ink of inline code and code blocks in an assistant answer. */
  readonly markdownCode: string;
  /** The ink of link text in an assistant answer. */
  readonly markdownLink: string;
  /** The dimmed ink of a committed reasoning (chain-of-thought) block. */
  readonly reasoningBody: string;
  /** The ink of the reasoning block's glyph and header. */
  readonly reasoningHeader: string;
  /** The ink of the expand affordance a collapsed reasoning block shows. */
  readonly reasoningMarker: string;
  /** The ink of the elapsed value the front end measured. */
  readonly timerInk: string;
  /** Body ink: the ordinary text of every row. */
  readonly ink: string;
  /** Secondary ink: hints, legends, and column headers. */
  readonly inkMuted: string;
  /** Faint ink: separators and dividers. */
  readonly inkFaint: string;
  /** Success ink: a run that completed. */
  readonly success: string;
  /** Warning ink: work that is queued or was interrupted. */
  readonly warning: string;
  /** Failure ink: a typed error's line. Spells `scarlet` exactly. */
  readonly error: string;
}

/** The palette for a terminal with a light background. */
export const LIGHT: Palette = {
  canvas: hex(0xf7, 0xf2, 0xe9),
  panel: hex(0xff, 0xfc, 0xf5),
  headerBg: hex(0xf3, 0xe7, 0xd5),
  tabActiveBg: hex(0xfc, 0xe2, 0xc4),
  rowSelectedBg: hex(0xfa, 0xdf, 0xc0),
  rowAltBg: hex(0xfb, 0xf5, 0xec),
  stubBg: hex(0xfb, 0xe4, 0xdf),
  inputBg: hex(0xff, 0xf4, 0xe2),
  badgeBg: hex(0xef, 0xe0, 0xc8),
  badgeInk: hex(0x5c, 0x46, 0x32),
  userSurface: hex(0xed, 0xf1, 0xf5),
  userBorder: hex(0xa9, 0xbc, 0xd0),
  userLabel: hex(0x2f, 0x5d, 0x7c),
  selectionBg: hex(0xd8, 0xe6, 0xf8),
  toolSurface: hex(0xfb, 0xed, 0xde),
  toolBorder: hex(0xd8, 0xa7, 0x6f),
  toolLabel: hex(0x8f, 0x4a, 0x16),
  toolPreview: hex(0x7a, 0x53, 0x30),
  noticeSurface: hex(0xf7, 0xe8, 0xce),
  noticeInk: hex(0x8a, 0x5a, 0x0b),
  scrollTrack: hex(0xe7, 0xdc, 0xcb),
  scrollThumb: hex(0xb0, 0x89, 0x68),
  dividerTint: hex(0xef, 0xe6, 0xd8),
  border: hex(0xdc, 0xcf, 0xbb),
  accent: hex(0xf9, 0x73, 0x16),
  accentDeep: hex(0xc2, 0x41, 0x0c),
  scarlet: hex(0xdc, 0x26, 0x26),
  markdownHeading: hex(0x9a, 0x34, 0x12),
  markdownCode: hex(0x7c, 0x2d, 0x12),
  markdownLink: hex(0x1d, 0x4e, 0xd8),
  reasoningBody: hex(0x8a, 0x7a, 0x66),
  reasoningHeader: hex(0x6d, 0x5a, 0x44),
  reasoningMarker: hex(0x8a, 0x5a, 0x2b),
  timerInk: hex(0xa1, 0x62, 0x07),
  ink: hex(0x2a, 0x21, 0x1a),
  inkMuted: hex(0x7a, 0x6a, 0x58),
  inkFaint: hex(0xa8, 0x96, 0x7f),
  success: hex(0x2f, 0x7d, 0x4f),
  warning: hex(0xb4, 0x53, 0x09),
  error: hex(0xdc, 0x26, 0x26),
};

/** The palette for a terminal with a dark background. */
export const DARK: Palette = {
  canvas: hex(0x0a, 0x0a, 0x0c),
  panel: hex(0x10, 0x10, 0x12),
  headerBg: hex(0x16, 0x16, 0x18),
  tabActiveBg: hex(0x2f, 0x2f, 0x31),
  rowSelectedBg: hex(0x33, 0x33, 0x35),
  rowAltBg: hex(0x0d, 0x0d, 0x0f),
  stubBg: hex(0x1c, 0x1c, 0x1e),
  inputBg: hex(0x13, 0x13, 0x15),
  badgeBg: hex(0x28, 0x28, 0x2a),
  badgeInk: hex(0xca, 0xca, 0xcc),
  userSurface: hex(0x25, 0x25, 0x27),
  userBorder: hex(0x47, 0x47, 0x49),
  userLabel: hex(0x82, 0x82, 0x84),
  selectionBg: hex(0x3a, 0x3a, 0x3c),
  toolSurface: hex(0x1f, 0x1f, 0x21),
  toolBorder: hex(0x52, 0x52, 0x54),
  toolLabel: hex(0xc2, 0xc2, 0xc4),
  toolPreview: hex(0x8a, 0x8a, 0x8c),
  noticeSurface: hex(0x22, 0x22, 0x24),
  noticeInk: hex(0xba, 0xba, 0xbc),
  scrollTrack: hex(0x2b, 0x2b, 0x2d),
  scrollThumb: hex(0x4a, 0x4a, 0x4c),
  dividerTint: hex(0x19, 0x19, 0x1b),
  border: hex(0x41, 0x41, 0x43),
  accent: hex(0xfb, 0x92, 0x3c),
  accentDeep: hex(0xfd, 0xba, 0x74),
  scarlet: hex(0x5e, 0x5e, 0x60),
  markdownHeading: hex(0xfa, 0xfa, 0xfc),
  markdownCode: hex(0xb2, 0xb2, 0xb4),
  markdownLink: hex(0xaa, 0xaa, 0xac),
  reasoningBody: hex(0x5a, 0x5a, 0x5c),
  reasoningHeader: hex(0x7a, 0x7a, 0x7c),
  reasoningMarker: hex(0xa2, 0xa2, 0xa4),
  timerInk: hex(0x9a, 0x9a, 0x9c),
  ink: hex(0xda, 0xda, 0xdc),
  inkMuted: hex(0xa6, 0xa6, 0xa8),
  inkFaint: hex(0x62, 0x62, 0x64),
  success: hex(0x6a, 0x6a, 0x6c),
  warning: hex(0x72, 0x72, 0x74),
  error: hex(0x5e, 0x5e, 0x60),
};

/** Returns the role names every rule below reads, error and scarlet apart. */
function roleNames(): ReadonlyArray<keyof Omit<Palette, "error" | "scarlet">> {
  return [
    "canvas",
    "panel",
    "headerBg",
    "tabActiveBg",
    "rowSelectedBg",
    "rowAltBg",
    "stubBg",
    "inputBg",
    "badgeBg",
    "badgeInk",
    "userSurface",
    "userBorder",
    "userLabel",
    "selectionBg",
    "toolSurface",
    "toolBorder",
    "toolLabel",
    "toolPreview",
    "noticeSurface",
    "noticeInk",
    "scrollTrack",
    "scrollThumb",
    "dividerTint",
    "border",
    "accent",
    "accentDeep",
    "markdownHeading",
    "markdownCode",
    "markdownLink",
    "reasoningBody",
    "reasoningHeader",
    "reasoningMarker",
    "timerInk",
    "ink",
    "inkMuted",
    "inkFaint",
    "success",
    "warning",
  ];
}

/** Returns the palette one preference resolves to. */
export function paletteFor(preference: "light" | "dark"): Palette {
  return preference === "light" ? LIGHT : DARK;
}
