//! The theme picker: the panel `/theme` opens above the chat's input block.
//!
//! The picker is one more row region of the one chat panel, directly above the
//! input block, exactly like the command hint band: its rows come out of the
//! transcript and no other pane gives any up, so the input block and the
//! detail line never move while it is open. A chat body too short for the panel
//! and the input paints no panel at all rather than taking the input's rows.
//!
//! The panel is drawn in the sessions browser's own vocabulary, minimal: its
//! rounded frame carries the `Theme` title, one row per theme carries the
//! theme's name, its one-line description, and - on the effective theme's row -
//! the browser's `> ` cursor marker on the selected wash, and its own last
//! inner row carries the `enter apply | esc revert` legend.

use revue::text::char_width;
use revue::widget::{Border, Card, RichText, Span, Stack, Style, Text, vstack};

use crate::app::{AppState, Theme};
use crate::tui::palette::Palette;

/// The rows the panel's frame takes.
const PANEL_FRAME: u16 = 2;

/// The rows the theme rows take: one per theme.
const THEME_ROWS: u16 = 2;

/// The rows the panel's own legend takes.
const LEGEND_ROWS: u16 = 1;

/// The columns the panel's frame and padding take: the two borders and two
/// padding columns beside each.
const PANEL_CHROME_COLUMNS: u16 = 6;

/// The marker the effective theme's row starts with.
const CURSOR_MARKER: &str = "> ";

/// The marker every other row starts with, keeping the names aligned.
const ROW_MARKER: &str = "  ";

/// The blank columns between a theme's name and its description.
const CELL_GAP: &str = "  ";

/// The columns the panel pads its rows with.
const PANEL_PADDING: u16 = 2;

/// Returns the rows the picker panel occupies: its frame, one row per theme,
/// and its own legend row.
#[must_use]
pub(in crate::tui) const fn panel_rows() -> u16 {
    PANEL_FRAME + THEME_ROWS + LEGEND_ROWS
}

/// Returns the picker panel, exactly [`panel_rows`] display rows tall.
///
/// `rows` is [`panel_rows`] in the chat panel's own arithmetic, so a body too
/// short for the panel and the input block paints nothing at all rather than a
/// panel in the input's rows.
pub(in crate::tui) fn picker_panel(
    state: &AppState,
    width: u16,
    rows: u16,
    palette: &'static Palette,
) -> Stack {
    if rows != panel_rows() {
        return vstack();
    }
    vstack().child_sized(panel(state, width, palette), rows)
}

/// Returns the framed panel: one rounded frame titled `Theme` around the theme
/// rows and the legend.
///
/// The rows read the effective theme, so the marker rides the row the picker is
/// previewing: the whole window has already repainted through that theme by the
/// time this frame is drawn.
fn panel(state: &AppState, width: u16, palette: &'static Palette) -> Border {
    let content = usize::from(width.saturating_sub(PANEL_CHROME_COLUMNS));
    let effective = state.effective_theme();
    let mut body = vstack();
    for theme in Theme::ALL {
        body = body.child_sized(theme_row(theme, theme == effective, content, palette), 1);
    }
    Border::rounded()
        .min_size(width, panel_rows())
        .max_size(width, panel_rows())
        .fg(palette.accent)
        .bg(palette.panel)
        .title(" Theme ")
        .child(
            Card::new()
                .flat()
                .padding(PANEL_PADDING)
                .background(palette.panel)
                .body(body.child_sized(legend(palette), LEGEND_ROWS)),
        )
}

/// Returns one theme row: its marker, its name, and its one-line description.
fn theme_row(theme: Theme, effective: bool, content: usize, palette: &'static Palette) -> Text {
    let marker = if effective { CURSOR_MARKER } else { ROW_MARKER };
    let line = format!("{marker}{}{CELL_GAP}{}", theme.label(), theme.description());
    let line = pad(&truncate(&line, content), content);
    if effective {
        Text::new(line)
            .fg(palette.accent_deep)
            .bg(palette.row_selected_bg)
            .bold()
    } else {
        Text::new(line).fg(palette.ink)
    }
}

/// Returns the panel's legend: the two keys and what they do.
fn legend(palette: &'static Palette) -> RichText {
    RichText::new()
        .default_style(Style::new().bg(palette.panel))
        .span(Span::styled(
            "enter",
            Style::new().fg(palette.accent).bg(palette.panel).bold(),
        ))
        .span(Span::styled(
            " apply",
            Style::new().fg(palette.ink_muted).bg(palette.panel),
        ))
        .span(Span::styled(
            " | ",
            Style::new().fg(palette.ink_faint).bg(palette.panel),
        ))
        .span(Span::styled(
            "esc",
            Style::new().fg(palette.accent).bg(palette.panel).bold(),
        ))
        .span(Span::styled(
            " revert",
            Style::new().fg(palette.ink_muted).bg(palette.panel),
        ))
}

/// Returns the display width of one piece of text.
// @todo(hack): this width/truncate/pad trio is re-rolled per pane - the same
// three live in `panes/commands.rs`, `panes/input.rs`, and
// `screens/sessions.rs` - although `layout.rs` already exports one display
// width; one shared measurement helper should serve every pane.
fn display_width(text: &str) -> usize {
    text.chars()
        .map(|character| usize::from(char_width(character)))
        .sum()
}

/// Returns `text` in exactly `columns` display columns.
///
/// A longer text is cut and marked with an ellipsis, a shorter one is padded,
/// so a row always fills the panel's content width and the selected wash covers
/// the whole row.
fn pad(text: &str, columns: usize) -> String {
    let mut filled = truncate(text, columns);
    filled.push_str(&" ".repeat(columns.saturating_sub(display_width(&filled))));
    filled
}

/// Returns `text` cut to `columns` columns, marking a cut with an ellipsis.
fn truncate(text: &str, columns: usize) -> String {
    if display_width(text) <= columns {
        return text.to_owned();
    }
    if columns == 0 {
        return String::new();
    }
    let mut cut = String::new();
    let mut used = 0;
    for character in text.chars() {
        let width = usize::from(char_width(character));
        if used + width > columns - 1 {
            break;
        }
        cut.push(character);
        used += width;
    }
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use revue::testing::TestApp;

    use super::{PANEL_FRAME, display_width, legend, pad, panel_rows, picker_panel};
    use crate::app::{Action, AppState, Theme};
    use crate::tui::palette::Palette;

    /// The light palette every fixture renders with.
    const LIGHT: &Palette = &crate::tui::palette::LIGHT;

    /// The width every panel fixture renders into.
    const WIDTH: u16 = 60;

    #[test]
    fn the_panel_counts_its_frame_its_rows_and_its_legend() {
        assert_eq!(
            panel_rows(),
            PANEL_FRAME + 3,
            "two theme rows and the legend sit inside the frame"
        );
    }

    #[test]
    fn the_panel_paints_its_title_its_rows_and_its_legend() {
        let panel = picker_panel(&AppState::new(None), WIDTH, panel_rows(), LIGHT);
        let app = TestApp::with_size(panel, WIDTH, panel_rows());
        let text = app.screen_text();
        assert!(text.contains("Theme"), "the frame carries the title");
        assert!(
            text.contains("Light"),
            "the first row names the light theme"
        );
        assert!(
            text.contains(Theme::Light.description()),
            "the first row describes the surface it paints"
        );
        assert!(text.contains("Dark"), "the second row names the dark theme");
        assert!(text.contains(Theme::Dark.description()));
        assert!(
            text.contains("enter apply | esc revert"),
            "the legend is the panel's own last inner row"
        );
        assert!(
            app.get_line(1).contains("> Light"),
            "the committed theme's row is the marked one: {}",
            app.get_line(1)
        );
        assert!(!text.contains("> Dark"));
    }

    #[test]
    fn the_panel_marks_the_previewed_row() {
        let mut state = AppState::new(None);
        state.update(Action::ThemePreviewed(Theme::Dark));
        let panel = picker_panel(&state, WIDTH, panel_rows(), LIGHT);
        let app = TestApp::with_size(panel, WIDTH, panel_rows());
        assert!(
            app.get_line(2).contains("> Dark"),
            "the marker follows the preview: {}",
            app.get_line(2)
        );
        assert!(
            !app.get_line(1).contains("> "),
            "the committed row keeps the aligned blank marker"
        );
    }

    #[test]
    fn a_panel_given_any_other_height_paints_nothing() {
        let state = AppState::new(None);
        assert_eq!(
            picker_panel(&state, WIDTH, 0, LIGHT).len(),
            0,
            "a region the chat did not give the panel's rows stays empty"
        );
        assert_eq!(
            picker_panel(&state, WIDTH, panel_rows() + 1, LIGHT).len(),
            0,
            "a region of another height is not the panel"
        );
    }

    #[test]
    fn padding_and_truncating_fill_the_content_exactly() {
        assert_eq!(pad("theme", 8), "theme   ");
        assert_eq!(display_width(&pad("theme", 8)), 8);
        assert_eq!(pad("theme", 4), "the…");
        assert_eq!(pad("theme", 0), "");
        assert_eq!(pad("theme", 2), "t…");
    }

    #[test]
    fn the_legend_names_the_two_keys_the_picker_owns() {
        let app = TestApp::with_size(legend(LIGHT), 40, 1);
        assert_eq!(
            app.screen_text().trim_end(),
            "enter apply | esc revert",
            "the legend is exactly the two keys and their actions"
        );
    }
}
