//! The markers and separators every surface spells.
//!
//! One glyph family, all monochrome text, each one column wide under the
//! terminal's own width measurement: no variant of any of them carries an emoji
//! presentation, so no terminal paints a marker in a colour of its own. A surface
//! that needs a marker reads it from here; nothing spells one of these again.

/** The label a committed user message opens with. */
export const USER_LABEL = "you";

/** The label a committed answer opens with. */
export const ASSISTANT_LABEL = "assistant";

/** The glyph that opens the row a finished run closes with. */
export const RUN_GLYPH = "▸";

/** The glyph that opens a reasoning block. */
export const REASONING_GLYPH = "∴";

/** The label the reasoning block's marker introduces. */
export const REASONING_LABEL = "reasoning";

/** The glyph that opens a committed daemon notice. */
export const NOTICE_GLYPH = "※";

/** The glyph that opens a tool block's badge. */
export const TOOL_GLYPH = "▸";

/** The rail a structured tool-result row opens with. */
export const TOOL_LIST_GLYPH = "‣";

/** The separator between two runs of one sentence. */
export const SEPARATOR = " · ";

/** The star that flanks the product's trailing word. */
export const LOGO_STAR = "✦";

/** The marker a list's cursor row starts with. */
export const CURSOR_MARKER = "> ";

/** The marker every other list row starts with, keeping the columns aligned. */
export const ROW_MARKER = "  ";

/** The glyph of the active browser tab. */
export const ACTIVE_TAB = "⦿";

/** The glyph of every other browser tab. */
export const INACTIVE_TAB = "◦";

/** The placeholder the browser's search bar shows while the filter is empty. */
export const SEARCH_PLACEHOLDER = "Filter by id or path";
