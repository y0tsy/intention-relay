//! The theme picker: the panel `/theme` opens above the chat's input block.
//!
//! The picker is one more row region of the one chat panel, directly above the
//! input block, exactly like the command hint band: its rows come out of the
//! transcript and no other pane gives any up, so the input block and the
//! detail line never move while it is open. A chat body too short for the panel
//! and the input paints no panel at all rather than taking the input's rows.
//!
//! The panel is drawn in the sessions browser's own vocabulary, minimal: its
//! rounded frame carries the `Theme` title, one row per preference carries the
//! preference's name, its one-line description, and - on the highlighted row -
//! the browser's `> ` cursor marker on the selected wash, and its own last
//! inner row carries the `enter apply | esc revert` legend. The first row is
//! `Auto`, the request to follow the terminal's own background, so returning to
//! detection never needs a hand-edited file.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Paragraph};

use crate::app::{AppState, ThemePreference};
use crate::tui::layout::{char_width, frame_title};
use crate::tui::palette::Palette;

/// The rows the panel's frame takes.
const PANEL_FRAME: u16 = 2;

/// The rows the preference rows take: one per preference.
const THEME_ROWS: u16 = ThemePreference::ALL.len() as u16;

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

/// Returns the rows the picker panel occupies: its frame, one row per
/// preference, and its own legend row.
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
    area: Rect,
    frame: &mut Frame,
    palette: &'static Palette,
) {
    if area.height != panel_rows() {
        return;
    }
    panel(state, area, frame, palette);
}

/// Returns the framed panel: one rounded frame titled `Theme` around the
/// preference rows and the legend.
///
/// The rows read the highlighted preference, so the marker rides the row the
/// picker is previewing: the whole window has already repainted through the
/// palette that preference names by the time this frame is drawn.
fn panel(state: &AppState, area: Rect, frame: &mut Frame, palette: &'static Palette) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(frame_title("Theme"))
        .border_style(Style::new().fg(palette.accent))
        .style(Style::new().bg(palette.panel));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.is_empty() {
        return;
    }
    let content = usize::from(area.width.saturating_sub(PANEL_CHROME_COLUMNS));
    let rows = Rect::new(
        inner.x.saturating_add(PANEL_PADDING),
        inner.y,
        inner.width.saturating_sub(PANEL_PADDING * 2),
        inner.height,
    );
    let highlighted = state.highlighted_preference();
    for (offset, preference) in ThemePreference::ALL.into_iter().enumerate() {
        let Ok(offset) = u16::try_from(offset) else {
            break;
        };
        frame.render_widget(
            Paragraph::new(preference_row(
                preference,
                preference == highlighted,
                content,
                palette,
            )),
            Rect::new(rows.x, rows.y.saturating_add(offset), rows.width, 1),
        );
    }
    frame.render_widget(
        Paragraph::new(legend(palette)),
        Rect::new(
            rows.x,
            rows.y.saturating_add(THEME_ROWS),
            rows.width,
            LEGEND_ROWS,
        ),
    );
}

/// Returns one row: its marker, its name, and its one-line description.
fn preference_row(
    preference: ThemePreference,
    highlighted: bool,
    content: usize,
    palette: &'static Palette,
) -> Line<'static> {
    let marker = if highlighted {
        CURSOR_MARKER
    } else {
        ROW_MARKER
    };
    let line = format!(
        "{marker}{}{CELL_GAP}{}",
        preference.label(),
        preference.description()
    );
    let line = pad(&truncate(&line, content), content);
    if highlighted {
        Line::from(Span::styled(
            line,
            Style::new()
                .fg(palette.accent_deep)
                .bg(palette.row_selected_bg)
                .add_modifier(Modifier::BOLD),
        ))
    } else {
        Line::from(Span::styled(line, Style::new().fg(palette.ink)))
    }
}

/// Returns the panel's legend: the two keys and what they do.
fn legend(palette: &'static Palette) -> Line<'static> {
    let key = |text: &'static str| {
        Span::styled(
            text,
            Style::new()
                .fg(palette.accent)
                .bg(palette.panel)
                .add_modifier(Modifier::BOLD),
        )
    };
    let word = |text: &'static str| {
        Span::styled(text, Style::new().fg(palette.ink_muted).bg(palette.panel))
    };
    Line::from(vec![
        key("enter"),
        word(" apply"),
        Span::styled(" | ", Style::new().fg(palette.ink_faint).bg(palette.panel)),
        key("esc"),
        word(" revert"),
    ])
    .style(Style::new().bg(palette.panel))
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
    use ratatui::layout::Rect;
    use ratatui::widgets::Paragraph;

    use super::{PANEL_FRAME, display_width, legend, pad, panel_rows, picker_panel};
    use crate::app::{Action, AppState, ThemePreference};
    use crate::tui::palette::Palette;
    use crate::tui::testing::Screen;

    /// The light palette every fixture renders with.
    const LIGHT: &Palette = &crate::tui::palette::LIGHT;

    /// The width every panel fixture renders into.
    const WIDTH: u16 = 60;

    /// Renders the picker panel of `state` into a screen of its own size.
    fn render(state: &AppState) -> Screen {
        Screen::widget(WIDTH, panel_rows(), |frame| {
            picker_panel(state, Rect::new(0, 0, WIDTH, panel_rows()), frame, LIGHT);
        })
    }

    #[test]
    fn the_panel_counts_its_frame_its_rows_and_its_legend() {
        assert_eq!(
            panel_rows(),
            PANEL_FRAME + 4,
            "three preference rows and the legend sit inside the frame"
        );
    }

    #[test]
    fn the_panel_paints_its_title_its_rows_and_its_legend() {
        let app = render(&AppState::new(None));
        let text = app.screen_text();
        assert!(text.contains("Theme"), "the frame carries the title");
        assert!(
            text.contains("Auto"),
            "the first row is the request to follow detection"
        );
        assert!(
            text.contains(ThemePreference::Auto.description()),
            "the first row describes what detection does"
        );
        assert!(
            text.contains("Light"),
            "the second row names the light theme"
        );
        assert!(text.contains(ThemePreference::Light.description()));
        assert!(text.contains("Dark"), "the third row names the dark theme");
        assert!(text.contains(ThemePreference::Dark.description()));
        assert!(
            text.contains("enter apply | esc revert"),
            "the legend is the panel's own last inner row"
        );
        assert!(
            app.line(1).contains("> Auto"),
            "the committed preference's row is the marked one: {}",
            app.line(1)
        );
        assert!(!text.contains("> Light"));
        assert!(!text.contains("> Dark"));
    }

    #[test]
    fn the_panel_marks_the_previewed_row() {
        let mut state = AppState::new(None);
        state.update(Action::ThemePreviewed(ThemePreference::Dark));
        let app = render(&state);
        assert!(
            app.line(3).contains("> Dark"),
            "the marker follows the preview: {}",
            app.line(3)
        );
        assert!(
            !app.line(1).contains("> "),
            "the committed row keeps the aligned blank marker"
        );
    }

    #[test]
    fn a_panel_given_any_other_height_paints_nothing() {
        let state = AppState::new(None);
        for height in [0, panel_rows() + 1] {
            let app = Screen::widget(WIDTH, panel_rows() + 1, |frame| {
                picker_panel(&state, Rect::new(0, 0, WIDTH, height), frame, LIGHT);
            });
            assert_eq!(
                app.screen_text().trim(),
                "",
                "a region of {height} rows is not the panel"
            );
        }
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
        let app = Screen::widget(40, 1, |frame| {
            frame.render_widget(Paragraph::new(legend(LIGHT)), Rect::new(0, 0, 40, 1));
        });
        assert_eq!(
            app.screen_text().trim_end(),
            "enter apply | esc revert",
            "the legend is exactly the two keys and their actions"
        );
    }
}
