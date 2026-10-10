//! The one colour system of the terminal front end.
//!
//! Every pane reads its colours here and nothing constructs a colour of its
//! own, so the whole terminal has one documented surface: a warm off-white
//! light theme and a warm charcoal dark theme over the same role table. Both
//! families are the same warm base - the dark theme is the light theme read by
//! inversion, keeping every role's hue and lifting its value far enough to
//! read on charcoal - so a frame switched between the two changes its
//! luminance and never its meaning.
//!
//! | Role | Light | Dark | Used by |
//! | --- | --- | --- | --- |
//! | [`Palette::canvas`] | `#F7F2E9` | `#16130F` | the window behind every panel |
//! | [`Palette::panel`] | `#FFFCF5` | `#1E1A15` | card and panel fills |
//! | [`Palette::header_bg`] | `#F3E7D5` | `#272219` | the tab row and the table header band |
//! | [`Palette::tab_active_bg`] | `#FCE2C4` | `#3A2A18` | the active tab's wash |
//! | [`Palette::row_selected_bg`] | `#FADFC0` | `#40291A` | the cursor row's wash |
//! | [`Palette::row_alt_bg`] | `#FBF5EC` | `#221D17` | every second row's zebra tint |
//! | [`Palette::stub_bg`] | `#FBE4DF` | `#33211E` | the `@todo(core)` empty state and welcome sub-blocks |
//! | [`Palette::input_bg`] | `#FFF4E2` | `#241E17` | the chat input buffer rows |
//! | [`Palette::badge_bg`] | `#EFE0C8` | `#322A20` | the badge wash of the input header and the welcome version |
//! | [`Palette::badge_ink`] | `#5C4632` | `#E4D3BC` | the label ink of a badge |
//! | [`Palette::user_surface`] | `#EDF1F5` | `#1C2229` | the wash of a user message card |
//! | [`Palette::user_border`] | `#A9BCD0` | `#4A5A6B` | the frame of a user message card |
//! | [`Palette::user_label`] | `#2F5D7C` | `#9CC2E0` | the `you` label of a user message card |
//! | [`Palette::selection_bg`] | `#D8E6F8` | `#26384C` | the fill of the transcript's selected display rows |
//! | [`Palette::tool_surface`] | `#FBEDDE` | `#241C15` | the wash of a tool block |
//! | [`Palette::tool_border`] | `#D8A76F` | `#8A5F32` | the frame of a tool block |
//! | [`Palette::tool_label`] | `#8F4A16` | `#F0A868` | the badge ink of a tool block's header |
//! | [`Palette::tool_preview`] | `#7A5330` | `#D3A87C` | the parsed result ink of a tool block |
//! | [`Palette::notice_surface`] | `#F7E8CE` | `#2A2113` | the wash of a daemon notice block |
//! | [`Palette::notice_ink`] | `#8A5A0B` | `#E3B15C` | the ink of a daemon notice block |
//! | [`Palette::scroll_track`] | `#E7DCCB` | `#2A241C` | the track of the transcript's native scrollbar |
//! | [`Palette::scroll_thumb`] | `#B08968` | `#6B543C` | the thumb of the transcript's native scrollbar |
//! | [`Palette::divider_tint`] | `#EFE6D8` | `#2A231B` | the divider bands between blocks |
//! | [`Palette::border`] | `#DCCFBB` | `#3A3229` | the neutral frames (chat, panes) |
//! | [`Palette::accent`] | `#F97316` | `#FB923C` | the primary accent: the focused frame, hotkeys |
//! | [`Palette::accent_deep`] | `#C2410C` | `#FDBA74` | accent text on an accent wash, the welcome lockup's product word |
//! | [`Palette::scarlet`] | `#DC2626` | `#F87171` | the secondary accent: active runs, failures |
//! | [`Palette::markdown_heading`] | `#9A3412` | `#FDA47A` | an assistant heading line |
//! | [`Palette::markdown_code`] | `#7C2D12` | `#FBC08A` | inline code and code blocks in an answer |
//! | [`Palette::markdown_link`] | `#1D4ED8` | `#93B4FB` | link text in an answer |
//! | [`Palette::reasoning_body`] | `#8A7A66` | `#B7A48C` | the dimmed chain-of-thought block |
//! | [`Palette::reasoning_header`] | `#6D5A44` | `#CDB79A` | the reasoning block's glyph and header |
//! | [`Palette::reasoning_marker`] | `#8A5A2B` | `#DEA263` | the expand affordance of a collapsed reasoning block |
//! | [`Palette::timer_ink`] | `#A16207` | `#E9C46A` | the measured elapsed value |
//! | [`Palette::todo_ink`] | `#8B7AA8` | `#C4B5E0` | the `@todo(core)` placeholder tone |
//! | [`Palette::ink`] | `#2A211A` | `#F0E7DA` | body text |
//! | [`Palette::ink_muted`] | `#7A6A58` | `#B9A992` | secondary text, hints, and the welcome lockup's trailing word |
//! | [`Palette::ink_faint`] | `#A8967F` | `#8F7E69` | dividers and separators |
//! | [`Palette::success`] | `#2F7D4F` | `#6EE7A0` | completed runs |
//! | [`Palette::warning`] | `#B45309` | `#F5B45C` | queued or interrupted work |
//! | [`Palette::error`] | `#DC2626` | `#F87171` | typed failures (the scarlet ink) |
//!
//! Three rules hold per theme and one holds across them:
//!
//! - every role is fully opaque, because a terminal cannot show a transparent
//!   literal;
//! - within one theme, no two roles share a value - the one documented alias
//!   is [`Palette::error`], which spells [`Palette::scarlet`] exactly, so a
//!   failure is never a different hue from the secondary accent;
//! - within one theme every role carries its own value, so a surface is never
//!   mistaken for its neighbour;
//! - every role differs between the two themes, so a frame is wholly one theme
//!   and never a half-switched surface.
//!
//! Colours degrade, words do not: a terminal without truecolor maps each role
//! of the active theme to its nearest ANSI colour, so no role may be the only
//! carrier of a state. Every use pairs its colour with a glyph, a border, or a
//! modifier - the cursor row carries `> ` and bold, the active tab carries `⦿`
//! and bold, a failure carries the `failed` status word - and the render tests
//! assert on those characters instead of on colours.

use revue::style::Color;

use crate::app::Theme;

/// The colour roles of one terminal theme.
///
/// A palette is a value, not a set of constants: a front end resolves [`of`]
/// once per frame from the state's theme, and every pane reads the roles of
/// the reference it was handed, so the same state always paints the same frame
/// and no view resolves a theme of its own.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Palette {
    /// The window behind every panel.
    pub canvas: Color,
    /// The fill of a card or panel that sits on the canvas.
    pub panel: Color,
    /// The wash behind a block's header row: tabs and table columns.
    pub header_bg: Color,
    /// The wash behind the active tab.
    pub tab_active_bg: Color,
    /// The wash behind the browser's cursor row.
    pub row_selected_bg: Color,
    /// The zebra tint of every second table row.
    pub row_alt_bg: Color,
    /// The wash behind an explanatory empty state the core cannot fill yet.
    pub stub_bg: Color,
    /// The wash behind the chat input line.
    pub input_bg: Color,
    /// The wash behind one badge of the input header.
    pub badge_bg: Color,
    /// The ink of a badge's label in the input header.
    pub badge_ink: Color,
    /// The wash behind a committed user message's card.
    pub user_surface: Color,
    /// The frame of a committed user message's card.
    pub user_border: Color,
    /// The `you` label ink of a committed user message's card.
    pub user_label: Color,
    /// The fill of the transcript's selected display rows.
    pub selection_bg: Color,
    /// The wash behind one tool block.
    ///
    /// A warm veil built from the palette's warm hues: the tool family adds no
    /// new hue, and the orange and scarlet accents stay the only saturated
    /// inks.
    pub tool_surface: Color,
    /// The frame of one tool block.
    pub tool_border: Color,
    /// The badge ink of one tool block's header.
    pub tool_label: Color,
    /// The parsed result ink of one tool block.
    ///
    /// A muted warm ink, clearly apart from [`Palette::ink`] and the assistant
    /// answer's body ink, so a tool result never reads as prose.
    pub tool_preview: Color,
    /// The wash behind one daemon notice block.
    pub notice_surface: Color,
    /// The ink of one daemon notice block.
    pub notice_ink: Color,
    /// The track of the transcript's native scrollbar.
    pub scroll_track: Color,
    /// The thumb of the transcript's native scrollbar.
    pub scroll_thumb: Color,
    /// The tint of a divider band between two blocks.
    pub divider_tint: Color,
    /// The neutral frame of a chat panel or a table.
    pub border: Color,
    /// The primary accent: the focused frame and every hotkey.
    pub accent: Color,
    /// Accent text that has to stay readable on an accent wash.
    pub accent_deep: Color,
    /// The secondary accent: a live run's marker and destructive hints.
    pub scarlet: Color,
    /// The ink of an assistant heading line.
    pub markdown_heading: Color,
    /// The ink of inline code and code blocks in an assistant answer.
    pub markdown_code: Color,
    /// The ink of link text in an assistant answer.
    pub markdown_link: Color,
    /// The dimmed ink of a committed reasoning (chain-of-thought) block.
    pub reasoning_body: Color,
    /// The ink of the reasoning block's glyph and header.
    pub reasoning_header: Color,
    /// The ink of the expand affordance a collapsed reasoning block shows.
    pub reasoning_marker: Color,
    /// The ink of the elapsed value the front end measured.
    pub timer_ink: Color,
    /// The ink of an `@todo(core)` placeholder the wire cannot fill yet.
    pub todo_ink: Color,
    /// Body ink: the ordinary text of every row.
    pub ink: Color,
    /// Secondary ink: hints, legends, and column headers.
    pub ink_muted: Color,
    /// Faint ink: separators and dividers.
    pub ink_faint: Color,
    /// Success ink: a run that completed.
    pub success: Color,
    /// Warning ink: work that is queued or was interrupted.
    pub warning: Color,
    /// Failure ink: a typed error's line.
    ///
    /// It spells [`Palette::scarlet`] exactly, in both themes: a failure is
    /// always the secondary accent's own value.
    pub error: Color,
}

/// The light theme: the warm off-white canvas the terminal has always drawn.
pub static LIGHT: Palette = Palette {
    canvas: Color::rgb(0xF7, 0xF2, 0xE9),
    panel: Color::rgb(0xFF, 0xFC, 0xF5),
    header_bg: Color::rgb(0xF3, 0xE7, 0xD5),
    tab_active_bg: Color::rgb(0xFC, 0xE2, 0xC4),
    row_selected_bg: Color::rgb(0xFA, 0xDF, 0xC0),
    row_alt_bg: Color::rgb(0xFB, 0xF5, 0xEC),
    stub_bg: Color::rgb(0xFB, 0xE4, 0xDF),
    input_bg: Color::rgb(0xFF, 0xF4, 0xE2),
    badge_bg: Color::rgb(0xEF, 0xE0, 0xC8),
    badge_ink: Color::rgb(0x5C, 0x46, 0x32),
    user_surface: Color::rgb(0xED, 0xF1, 0xF5),
    user_border: Color::rgb(0xA9, 0xBC, 0xD0),
    user_label: Color::rgb(0x2F, 0x5D, 0x7C),
    selection_bg: Color::rgb(0xD8, 0xE6, 0xF8),
    tool_surface: Color::rgb(0xFB, 0xED, 0xDE),
    tool_border: Color::rgb(0xD8, 0xA7, 0x6F),
    tool_label: Color::rgb(0x8F, 0x4A, 0x16),
    tool_preview: Color::rgb(0x7A, 0x53, 0x30),
    notice_surface: Color::rgb(0xF7, 0xE8, 0xCE),
    notice_ink: Color::rgb(0x8A, 0x5A, 0x0B),
    scroll_track: Color::rgb(0xE7, 0xDC, 0xCB),
    scroll_thumb: Color::rgb(0xB0, 0x89, 0x68),
    divider_tint: Color::rgb(0xEF, 0xE6, 0xD8),
    border: Color::rgb(0xDC, 0xCF, 0xBB),
    accent: Color::rgb(0xF9, 0x73, 0x16),
    accent_deep: Color::rgb(0xC2, 0x41, 0x0C),
    scarlet: Color::rgb(0xDC, 0x26, 0x26),
    markdown_heading: Color::rgb(0x9A, 0x34, 0x12),
    markdown_code: Color::rgb(0x7C, 0x2D, 0x12),
    markdown_link: Color::rgb(0x1D, 0x4E, 0xD8),
    reasoning_body: Color::rgb(0x8A, 0x7A, 0x66),
    reasoning_header: Color::rgb(0x6D, 0x5A, 0x44),
    reasoning_marker: Color::rgb(0x8A, 0x5A, 0x2B),
    timer_ink: Color::rgb(0xA1, 0x62, 0x07),
    todo_ink: Color::rgb(0x8B, 0x7A, 0xA8),
    ink: Color::rgb(0x2A, 0x21, 0x1A),
    ink_muted: Color::rgb(0x7A, 0x6A, 0x58),
    ink_faint: Color::rgb(0xA8, 0x96, 0x7F),
    success: Color::rgb(0x2F, 0x7D, 0x4F),
    warning: Color::rgb(0xB4, 0x53, 0x09),
    error: Color::rgb(0xDC, 0x26, 0x26),
};

/// The dark theme: the same warm family read on a warm charcoal canvas.
pub static DARK: Palette = Palette {
    canvas: Color::rgb(0x16, 0x13, 0x0F),
    panel: Color::rgb(0x1E, 0x1A, 0x15),
    header_bg: Color::rgb(0x27, 0x22, 0x19),
    tab_active_bg: Color::rgb(0x3A, 0x2A, 0x18),
    row_selected_bg: Color::rgb(0x40, 0x29, 0x1A),
    row_alt_bg: Color::rgb(0x22, 0x1D, 0x17),
    stub_bg: Color::rgb(0x33, 0x21, 0x1E),
    input_bg: Color::rgb(0x24, 0x1E, 0x17),
    badge_bg: Color::rgb(0x32, 0x2A, 0x20),
    badge_ink: Color::rgb(0xE4, 0xD3, 0xBC),
    user_surface: Color::rgb(0x1C, 0x22, 0x29),
    user_border: Color::rgb(0x4A, 0x5A, 0x6B),
    user_label: Color::rgb(0x9C, 0xC2, 0xE0),
    selection_bg: Color::rgb(0x26, 0x38, 0x4C),
    tool_surface: Color::rgb(0x24, 0x1C, 0x15),
    tool_border: Color::rgb(0x8A, 0x5F, 0x32),
    tool_label: Color::rgb(0xF0, 0xA8, 0x68),
    tool_preview: Color::rgb(0xD3, 0xA8, 0x7C),
    notice_surface: Color::rgb(0x2A, 0x21, 0x13),
    notice_ink: Color::rgb(0xE3, 0xB1, 0x5C),
    scroll_track: Color::rgb(0x2A, 0x24, 0x1C),
    scroll_thumb: Color::rgb(0x6B, 0x54, 0x3C),
    divider_tint: Color::rgb(0x2A, 0x23, 0x1B),
    border: Color::rgb(0x3A, 0x32, 0x29),
    accent: Color::rgb(0xFB, 0x92, 0x3C),
    accent_deep: Color::rgb(0xFD, 0xBA, 0x74),
    scarlet: Color::rgb(0xF8, 0x71, 0x71),
    markdown_heading: Color::rgb(0xFD, 0xA4, 0x7A),
    markdown_code: Color::rgb(0xFB, 0xC0, 0x8A),
    markdown_link: Color::rgb(0x93, 0xB4, 0xFB),
    reasoning_body: Color::rgb(0xB7, 0xA4, 0x8C),
    reasoning_header: Color::rgb(0xCD, 0xB7, 0x9A),
    reasoning_marker: Color::rgb(0xDE, 0xA2, 0x63),
    timer_ink: Color::rgb(0xE9, 0xC4, 0x6A),
    todo_ink: Color::rgb(0xC4, 0xB5, 0xE0),
    ink: Color::rgb(0xF0, 0xE7, 0xDA),
    ink_muted: Color::rgb(0xB9, 0xA9, 0x92),
    ink_faint: Color::rgb(0x8F, 0x7E, 0x69),
    success: Color::rgb(0x6E, 0xE7, 0xA0),
    warning: Color::rgb(0xF5, 0xB4, 0x5C),
    error: Color::rgb(0xF8, 0x71, 0x71),
};

/// Returns the palette of one theme.
///
/// A front end resolves the reference once per frame from the state's theme
/// and hands it down to every pane, so the frame paints one theme from one
/// value and no view resolves a theme of its own.
#[must_use]
pub const fn of(theme: Theme) -> &'static Palette {
    match theme {
        Theme::Light => &LIGHT,
        Theme::Dark => &DARK,
    }
}

#[cfg(test)]
mod tests {
    use revue::style::Color;

    use crate::app::Theme;

    use super::{DARK, LIGHT, Palette, of};

    /// The themes every rule below is asserted for, with their names.
    const THEMES: [(&str, &Palette); 2] = [("light", &LIGHT), ("dark", &DARK)];

    /// Every distinct role of one theme, named as the module documents it.
    ///
    /// [`Palette::error`] is not listed: it is the documented alias of
    /// [`Palette::scarlet`], and the alias is asserted separately.
    fn roles(palette: &Palette) -> [(&'static str, Color); 40] {
        [
            ("canvas", palette.canvas),
            ("panel", palette.panel),
            ("header_bg", palette.header_bg),
            ("tab_active_bg", palette.tab_active_bg),
            ("row_selected_bg", palette.row_selected_bg),
            ("row_alt_bg", palette.row_alt_bg),
            ("stub_bg", palette.stub_bg),
            ("input_bg", palette.input_bg),
            ("badge_bg", palette.badge_bg),
            ("badge_ink", palette.badge_ink),
            ("user_surface", palette.user_surface),
            ("user_border", palette.user_border),
            ("user_label", palette.user_label),
            ("selection_bg", palette.selection_bg),
            ("tool_surface", palette.tool_surface),
            ("tool_border", palette.tool_border),
            ("tool_label", palette.tool_label),
            ("tool_preview", palette.tool_preview),
            ("notice_surface", palette.notice_surface),
            ("notice_ink", palette.notice_ink),
            ("scroll_track", palette.scroll_track),
            ("scroll_thumb", palette.scroll_thumb),
            ("divider_tint", palette.divider_tint),
            ("border", palette.border),
            ("accent", palette.accent),
            ("accent_deep", palette.accent_deep),
            ("scarlet", palette.scarlet),
            ("markdown_heading", palette.markdown_heading),
            ("markdown_code", palette.markdown_code),
            ("markdown_link", palette.markdown_link),
            ("reasoning_body", palette.reasoning_body),
            ("reasoning_header", palette.reasoning_header),
            ("reasoning_marker", palette.reasoning_marker),
            ("timer_ink", palette.timer_ink),
            ("todo_ink", palette.todo_ink),
            ("ink", palette.ink),
            ("ink_muted", palette.ink_muted),
            ("ink_faint", palette.ink_faint),
            ("success", palette.success),
            ("warning", palette.warning),
        ]
    }

    #[test]
    fn every_role_is_fully_opaque() {
        for (theme, palette) in THEMES {
            for (name, role) in roles(palette) {
                assert!(
                    role.is_opaque(),
                    "{theme} {name} must be opaque: a terminal cannot show a \
                     transparent literal"
                );
            }
        }
    }

    #[test]
    fn no_two_roles_share_a_value_within_a_theme() {
        for (theme, palette) in THEMES {
            let roles = roles(palette);
            for (index, (name, role)) in roles.iter().enumerate() {
                for (other_name, other) in &roles[index + 1..] {
                    assert_ne!(
                        role, other,
                        "{theme} {name} and {theme} {other_name} must not share a value"
                    );
                }
            }
        }
    }

    #[test]
    fn the_error_ink_is_the_secondary_accents_value_in_both_themes() {
        for (theme, palette) in THEMES {
            assert_eq!(
                palette.error, palette.scarlet,
                "{theme} failures are written in the scarlet accent"
            );
        }
    }

    #[test]
    fn every_role_differs_between_the_two_themes() {
        let light = roles(&LIGHT);
        let dark = roles(&DARK);
        for ((name, light), (_, dark)) in light.iter().zip(dark.iter()) {
            assert_ne!(
                light, dark,
                "{name} must read as one theme's own value, not the other's: \
                 {light:?} is both themes' value"
            );
        }
    }

    #[test]
    fn the_palette_follows_the_theme() {
        assert_eq!(of(Theme::Light), &LIGHT, "light resolves the light roles");
        assert_eq!(of(Theme::Dark), &DARK, "dark resolves the dark roles");
        assert!(
            std::ptr::eq(of(Theme::Dark), &DARK),
            "the resolved palette is the one static the whole frame paints with"
        );
    }
}
