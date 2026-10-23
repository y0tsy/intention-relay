//! The markers and separators every surface spells.
//!
//! One glyph family, all monochrome text, each one column wide under the
//! terminal's own width measurement: no variant of any of them carries an emoji
//! presentation, so no terminal paints a marker in a colour of its own. A surface
//! that needs a marker reads it from here; nothing spells one of these again.

/** The glyph a committed user message's card opens with. */
export const USER_GLYPH = "❯";

/** The label the user card's marker introduces. */
export const USER_LABEL = "you";

/**
 * The glyph that opens a committed answer block.
 *
 * U+2237 PROPORTION: a member of the reasoning header's family (U+2234
 * THEREFORE) that is visibly distinct from it, rare, monochrome, and free of any
 * emoji presentation.
 */
export const ANSWER_GLYPH = "∷";

/** The glyph that opens a reasoning block. */
export const REASONING_GLYPH = "∴";

/** The label the reasoning block's marker introduces. */
export const REASONING_LABEL = "reasoning";

/** The affordance a collapsed reasoning block's marker opens with. */
export const REASONING_EXPAND = "▸";

/** The glyph that opens a committed daemon notice. */
export const NOTICE_GLYPH = "※";

/** The glyph that opens a tool block's badge. */
export const TOOL_GLYPH = "▸";

/** The rail a structured tool-result row opens with. */
export const TOOL_LIST_GLYPH = "‣";

/** The blank columns every marker keeps before the text it opens. */
export const MARKER_GAP = 1;

/** The indent every continuation row of one wrapped logical line carries. */
export const CONTINUATION_INDENT = "  ";

/** The glyph rendered at the input cursor position. */
export const CURSOR = "█";

/** The prompt the input buffer's first line carries before its text. */
export const PROMPT = "> ";

/** The separator between two runs of one sentence. */
export const SEPARATOR = " · ";

/** The marker a list's cursor row starts with. */
export const CURSOR_MARKER = "> ";

/** The marker every other list row starts with, keeping the columns aligned. */
export const ROW_MARKER = "  ";

/** The glyph of the active browser tab. */
export const ACTIVE_TAB = "⦿";

/** The glyph of every other browser tab. */
export const INACTIVE_TAB = "◦";

/** The title the docked browser panel's frame carries. */
export const SESSIONS_TITLE = "Sessions";

/** The placeholder the browser's search bar shows while the filter is empty. */
export const SEARCH_PLACEHOLDER = "Type to filter sessions...";

/** Returns one marker's glyph and the text it opens, separated by the gap. */
export function markerText(glyph: string, text: string): string {
  return `${glyph}${" ".repeat(MARKER_GAP)}${text}`;
}
