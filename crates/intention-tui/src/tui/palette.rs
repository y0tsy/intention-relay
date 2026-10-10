//! The one colour system of the terminal front end.
//!
//! Every pane reads its colours here and nothing constructs a colour of its
//! own, so the whole terminal has one documented surface: a warm off-white
//! canvas, a juicy orange primary accent, a scarlet secondary accent, and the
//! washes each surface is tinted with.
//!
//! | Role | Value | Used by |
//! | --- | --- | --- |
//! | [`CANVAS`] | `#F7F2E9` | the window behind every panel |
//! | [`PANEL`] | `#FFFCF5` | card and panel fills |
//! | [`HEADER_BG`] | `#F3E7D5` | the tab row and the table header band |
//! | [`TAB_ACTIVE_BG`] | `#FCE2C4` | the active tab's wash |
//! | [`ROW_SELECTED_BG`] | `#FADFC0` | the cursor row's wash |
//! | [`ROW_ALT_BG`] | `#FBF5EC` | every second row's zebra tint |
//! | [`STUB_BG`] | `#FBE4DF` | the `@todo(core)` empty state and welcome sub-blocks |
//! | [`INPUT_BG`] | `#FFF4E2` | the chat input line |
//! | [`BADGE_BG`] | `#EFE0C8` | the badge wash of the input header and the welcome version |
//! | [`BADGE_INK`] | `#5C4632` | the label ink of a badge |
//! | [`USER_SURFACE`] | `#EDF1F5` | the wash of a user message card |
//! | [`USER_BORDER`] | `#A9BCD0` | the frame of a user message card |
//! | [`USER_LABEL`] | `#2F5D7C` | the `you` label of a user message card |
//! | [`SELECTION_BG`] | `#D8E6F8` | the fill of the transcript's selected display rows |
//! | [`TOOL_SURFACE`] | `#FBEDDE` | the wash of a tool block: a light orange veil |
//! | [`TOOL_BORDER`] | `#D8A76F` | the frame of a tool block |
//! | [`TOOL_LABEL`] | `#8F4A16` | the badge ink of a tool block's header |
//! | [`TOOL_PREVIEW`] | `#7A5330` | the parsed result ink of a tool block |
//! | [`NOTICE_SURFACE`] | `#F7E8CE` | the wash of a daemon notice block |
//! | [`NOTICE_INK`] | `#8A5A0B` | the ink of a daemon notice block |
//! | [`SCROLL_TRACK`] | `#E7DCCB` | the track of the transcript's native scrollbar |
//! | [`SCROLL_THUMB`] | `#B08968` | the thumb of the transcript's native scrollbar |
//! | [`DIVIDER_TINT`] | `#EFE6D8` | the divider bands between blocks |
//! | [`BORDER`] | `#DCCFBB` | the neutral frames (chat, panes) |
//! | [`ACCENT`] | `#F97316` | the primary accent: the focused frame, hotkeys |
//! | [`ACCENT_DEEP`] | `#C2410C` | accent text on an accent wash, the welcome lockup's product word |
//! | [`SCARLET`] | `#DC2626` | the secondary accent: active runs, failures |
//! | [`MARKDOWN_HEADING`] | `#9A3412` | an assistant heading line |
//! | [`MARKDOWN_CODE`] | `#7C2D12` | inline code and code blocks in an answer |
//! | [`MARKDOWN_LINK`] | `#1D4ED8` | link text in an answer |
//! | [`REASONING_BODY`] | `#8A7A66` | the dimmed chain-of-thought block |
//! | [`REASONING_HEADER`] | `#6D5A44` | the reasoning block's glyph and header |
//! | [`REASONING_MARKER`] | `#8A5A2B` | the expand affordance of a collapsed reasoning block |
//! | [`TIMER_INK`] | `#A16207` | the measured elapsed value |
//! | [`TODO_INK`] | `#8B7AA8` | the `@todo(core)` placeholder tone |
//! | [`INK`] | `#2A211A` | body text |
//! | [`INK_MUTED`] | `#7A6A58` | secondary text, hints, and the welcome lockup's trailing word |
//! | [`INK_FAINT`] | `#A8967F` | dividers and separators |
//! | [`SUCCESS`] | `#2F7D4F` | completed runs |
//! | [`WARNING`] | `#B45309` | queued or interrupted work |
//! | [`ERROR`] | `#DC2626` | typed failures (the scarlet ink) |
//!
//! Colours degrade, words do not: a terminal without truecolor maps each role
//! to its nearest ANSI colour, so no role may be the only carrier of a state.
//! Every use pairs its colour with a glyph, a border, or a modifier - the
//! cursor row carries `> ` and bold, the active tab carries `⦿` and bold, a
//! failure carries the `failed` status word - and the render tests assert on
//! those characters instead of on colours.

use revue::style::Color;

/// The window behind every panel: a warm off-white canvas.
pub const CANVAS: Color = Color::rgb(247, 242, 233);

/// The fill of a card or panel that sits on the canvas.
pub const PANEL: Color = Color::rgb(255, 252, 245);

/// The wash behind a block's header row: tabs and table columns.
pub const HEADER_BG: Color = Color::rgb(243, 231, 213);

/// The wash behind the active tab.
pub const TAB_ACTIVE_BG: Color = Color::rgb(252, 226, 196);

/// The wash behind the browser's cursor row.
pub const ROW_SELECTED_BG: Color = Color::rgb(250, 223, 192);

/// The zebra tint of every second table row.
pub const ROW_ALT_BG: Color = Color::rgb(251, 245, 236);

/// The wash behind an explanatory empty state the core cannot fill yet.
pub const STUB_BG: Color = Color::rgb(251, 228, 223);

/// The wash behind the chat input line.
pub const INPUT_BG: Color = Color::rgb(255, 244, 226);

/// The wash behind one badge of the input header.
pub const BADGE_BG: Color = Color::rgb(239, 224, 200);

/// The ink of a badge's label in the input header.
pub const BADGE_INK: Color = Color::rgb(92, 70, 50);

/// The wash behind a committed user message's card.
pub const USER_SURFACE: Color = Color::rgb(237, 241, 245);

/// The frame of a committed user message's card.
pub const USER_BORDER: Color = Color::rgb(169, 188, 208);

/// The `you` label ink of a committed user message's card.
pub const USER_LABEL: Color = Color::rgb(47, 93, 124);

/// The fill of the transcript's selected display rows.
pub const SELECTION_BG: Color = Color::rgb(216, 230, 248);

/// The wash behind one tool block.
///
/// A light orange veil built from the palette's warm hues: the tool family
/// adds no new hue, and the orange and scarlet accents stay the only saturated
/// inks.
pub const TOOL_SURFACE: Color = Color::rgb(251, 237, 222);

/// The frame of one tool block.
pub const TOOL_BORDER: Color = Color::rgb(216, 167, 111);

/// The badge ink of one tool block's header.
pub const TOOL_LABEL: Color = Color::rgb(143, 74, 22);

/// The parsed result ink of one tool block.
///
/// A muted warm brown, readably dimmer than [`INK`] and clearly apart from the
/// assistant answer's body ink, so a tool result never reads as prose.
pub const TOOL_PREVIEW: Color = Color::rgb(122, 83, 48);

/// The wash behind one daemon notice block.
pub const NOTICE_SURFACE: Color = Color::rgb(247, 232, 206);

/// The ink of one daemon notice block.
pub const NOTICE_INK: Color = Color::rgb(138, 90, 11);

/// The track of the transcript's native scrollbar.
pub const SCROLL_TRACK: Color = Color::rgb(231, 220, 203);

/// The thumb of the transcript's native scrollbar.
pub const SCROLL_THUMB: Color = Color::rgb(176, 137, 104);

/// The tint of a divider band between two blocks.
pub const DIVIDER_TINT: Color = Color::rgb(239, 230, 216);

/// The neutral frame of a chat panel or a table.
pub const BORDER: Color = Color::rgb(220, 207, 187);

/// The primary accent: the focused frame and every hotkey.
pub const ACCENT: Color = Color::rgb(249, 115, 22);

/// Accent text that has to stay readable on an accent wash.
pub const ACCENT_DEEP: Color = Color::rgb(194, 65, 12);

/// The secondary accent: a live run's marker and destructive hints.
pub const SCARLET: Color = Color::rgb(220, 38, 38);

/// The ink of an assistant heading line.
pub const MARKDOWN_HEADING: Color = Color::rgb(154, 52, 18);

/// The ink of inline code and code blocks in an assistant answer.
pub const MARKDOWN_CODE: Color = Color::rgb(124, 45, 18);

/// The ink of link text in an assistant answer.
pub const MARKDOWN_LINK: Color = Color::rgb(29, 78, 216);

/// The dimmed ink of a committed reasoning (chain-of-thought) block.
pub const REASONING_BODY: Color = Color::rgb(138, 122, 102);

/// The ink of the reasoning block's glyph and header.
pub const REASONING_HEADER: Color = Color::rgb(109, 90, 68);

/// The ink of the expand affordance a collapsed reasoning block shows.
pub const REASONING_MARKER: Color = Color::rgb(138, 90, 43);

/// The ink of the elapsed value the front end measured.
pub const TIMER_INK: Color = Color::rgb(161, 98, 7);

/// The ink of an `@todo(core)` placeholder the wire cannot fill yet.
pub const TODO_INK: Color = Color::rgb(139, 122, 168);

/// Body ink: the dark warm neutral every ordinary row is written in.
pub const INK: Color = Color::rgb(42, 33, 26);

/// Secondary ink: hints, legends, and column headers.
pub const INK_MUTED: Color = Color::rgb(122, 106, 88);

/// Faint ink: separators and dividers.
pub const INK_FAINT: Color = Color::rgb(168, 150, 127);

/// Success ink: a run that completed.
pub const SUCCESS: Color = Color::rgb(47, 125, 79);

/// Warning ink: work that is queued or was interrupted.
pub const WARNING: Color = Color::rgb(180, 83, 9);

/// Failure ink: a typed error's line.
pub const ERROR: Color = SCARLET;

#[cfg(test)]
mod tests {
    use revue::style::Color;

    use super::{
        ACCENT, ACCENT_DEEP, BADGE_BG, BADGE_INK, BORDER, CANVAS, DIVIDER_TINT, ERROR, HEADER_BG,
        INK, INK_FAINT, INK_MUTED, INPUT_BG, MARKDOWN_CODE, MARKDOWN_HEADING, MARKDOWN_LINK,
        NOTICE_INK, NOTICE_SURFACE, PANEL, REASONING_BODY, REASONING_HEADER, REASONING_MARKER,
        ROW_ALT_BG, ROW_SELECTED_BG, SCARLET, SCROLL_THUMB, SCROLL_TRACK, SELECTION_BG, STUB_BG,
        SUCCESS, TAB_ACTIVE_BG, TIMER_INK, TODO_INK, TOOL_BORDER, TOOL_LABEL, TOOL_PREVIEW,
        TOOL_SURFACE, USER_BORDER, USER_LABEL, USER_SURFACE, WARNING,
    };

    /// Every distinct role, named as the module documents it.
    const ROLES: [(&str, Color); 40] = [
        ("CANVAS", CANVAS),
        ("PANEL", PANEL),
        ("HEADER_BG", HEADER_BG),
        ("TAB_ACTIVE_BG", TAB_ACTIVE_BG),
        ("ROW_SELECTED_BG", ROW_SELECTED_BG),
        ("ROW_ALT_BG", ROW_ALT_BG),
        ("STUB_BG", STUB_BG),
        ("INPUT_BG", INPUT_BG),
        ("BADGE_BG", BADGE_BG),
        ("BADGE_INK", BADGE_INK),
        ("USER_SURFACE", USER_SURFACE),
        ("USER_BORDER", USER_BORDER),
        ("USER_LABEL", USER_LABEL),
        ("SELECTION_BG", SELECTION_BG),
        ("TOOL_SURFACE", TOOL_SURFACE),
        ("TOOL_BORDER", TOOL_BORDER),
        ("TOOL_LABEL", TOOL_LABEL),
        ("TOOL_PREVIEW", TOOL_PREVIEW),
        ("NOTICE_SURFACE", NOTICE_SURFACE),
        ("NOTICE_INK", NOTICE_INK),
        ("SCROLL_TRACK", SCROLL_TRACK),
        ("SCROLL_THUMB", SCROLL_THUMB),
        ("DIVIDER_TINT", DIVIDER_TINT),
        ("BORDER", BORDER),
        ("ACCENT", ACCENT),
        ("ACCENT_DEEP", ACCENT_DEEP),
        ("SCARLET", SCARLET),
        ("MARKDOWN_HEADING", MARKDOWN_HEADING),
        ("MARKDOWN_CODE", MARKDOWN_CODE),
        ("MARKDOWN_LINK", MARKDOWN_LINK),
        ("REASONING_BODY", REASONING_BODY),
        ("REASONING_HEADER", REASONING_HEADER),
        ("REASONING_MARKER", REASONING_MARKER),
        ("TIMER_INK", TIMER_INK),
        ("TODO_INK", TODO_INK),
        ("INK", INK),
        ("INK_MUTED", INK_MUTED),
        ("INK_FAINT", INK_FAINT),
        ("SUCCESS", SUCCESS),
        ("WARNING", WARNING),
    ];

    #[test]
    fn every_role_is_fully_opaque() {
        for (name, role) in ROLES {
            assert!(
                role.is_opaque(),
                "{name} must be opaque: a terminal cannot show a transparent literal"
            );
        }
    }

    #[test]
    fn no_two_roles_share_a_value() {
        for (index, (name, role)) in ROLES.iter().enumerate() {
            for (other_name, other) in &ROLES[index + 1..] {
                assert_ne!(
                    role, other,
                    "{name} and {other_name} must not share a value"
                );
            }
        }
    }

    #[test]
    fn the_error_ink_is_the_secondary_accents_value() {
        assert_eq!(ERROR, SCARLET, "failures are written in the scarlet accent");
    }
}
