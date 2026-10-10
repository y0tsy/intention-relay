//! The input's command hint menu: the commands the first word selects.
//!
//! The menu is one band of the chat panel directly above the input block, and
//! the transcript gives up exactly the rows it takes: the panel's body still
//! sums to transcript plus menu plus input plus detail, so nothing is painted
//! over anything and the one window never grows a second surface.
//!
//! The band is its own rounded frame on the header wash, with the three columns
//! `[Command] [Description] [Category]` in that order. The highlighted row
//! carries the sessions browser's own cursor vocabulary - the `> ` marker and
//! the selected wash - so the two lists read the same way, and the command
//! column keeps the accent ink the input line's own prompt uses.

use revue::style::Color;
use revue::text::char_width;
use revue::widget::{RichText, Span, Stack, Style, vstack};

use crate::app::{AppState, COMMANDS, CommandSpec};
use crate::tui::palette;

/// The most command rows the band shows before it reports the rest.
pub(in crate::tui) const MAX_COMMAND_ROWS: usize = 5;

/// The rows the band's own frame takes.
const BAND_FRAME_ROWS: u16 = 2;

/// The columns the band's frame and padding take: the two borders and one
/// padding column beside each.
const BAND_CHROME_COLUMNS: usize = 4;

/// The columns the cursor marker takes, so every other row aligns with it.
const MARKER_COLUMNS: usize = 2;

/// The blank column between two columns.
const COLUMN_GAP: usize = 1;

/// The marker the highlighted row starts with.
const CURSOR_MARKER: &str = "> ";

/// The marker every other row starts with, keeping the columns aligned.
const ROW_MARKER: &str = "  ";

/// The label the band's top border carries.
const TITLE: &str = "commands";

/// Returns the rows the menu occupies for the state's current word, or `0`
/// while no menu is open.
#[must_use]
pub(in crate::tui) fn menu_rows(state: &AppState) -> u16 {
    state.command_menu().map_or(0, |menu| band_rows(menu.len()))
}

/// Returns the rows a band of `commands` commands occupies: its own frame, the
/// rows it shows, and the one row its overflow report takes.
#[must_use]
const fn band_rows(commands: usize) -> u16 {
    let shown = if commands > MAX_COMMAND_ROWS {
        MAX_COMMAND_ROWS
    } else {
        commands
    };
    let shown = if shown > u16::MAX as usize {
        u16::MAX
    } else {
        shown as u16
    };
    let overflow = if commands > MAX_COMMAND_ROWS { 1 } else { 0 };
    BAND_FRAME_ROWS
        .saturating_add(shown)
        .saturating_add(overflow)
}

/// Returns the band: the menu's framed rows, exactly `rows` display rows tall.
///
/// `rows` is [`menu_rows`] for the same state, so the band and the panel's
/// arithmetic agree by construction - and a panel too small to hold the band
/// paints nothing at all rather than a band in the input's rows.
pub(in crate::tui) fn menu_block(state: &AppState, width: u16, rows: u16) -> Stack {
    let Some(menu) = state.command_menu() else {
        return vstack();
    };
    if rows != band_rows(menu.len()) {
        return vstack();
    }
    let commands = menu.commands().map(Row::of).collect::<Vec<_>>();
    band(&commands, menu.highlight(), usize::from(width), rows)
}

/// One row of the band: the three cells one command contributes.
struct Row<'a> {
    /// The command's name, spelled without its leading slash.
    name: &'a str,
    /// The command's one-line description.
    description: &'a str,
    /// The command's category.
    category: &'a str,
}

impl<'a> Row<'a> {
    /// Returns the row one registered command contributes.
    const fn of(command: CommandSpec) -> Self {
        Self {
            name: command.name,
            description: command.description,
            category: command.category,
        }
    }

    /// Returns the name as a user writes it, with its leading slash.
    fn typed_name(&self) -> String {
        format!("/{}", self.name)
    }
}

/// Returns the band of one command list with one row highlighted.
fn band(commands: &[Row], highlight: usize, width: usize, rows: u16) -> Stack {
    let wash = palette::HEADER_BG;
    let columns = Columns::of(width.saturating_sub(BAND_CHROME_COLUMNS));
    let mut band = vstack().child_sized(frame_row(width, wash, true), 1);
    for (row, command) in commands.iter().take(MAX_COMMAND_ROWS).enumerate() {
        band = band.child_sized(command_row(command, row == highlight, &columns, wash), 1);
    }
    if let Some(hidden) = commands.len().checked_sub(MAX_COMMAND_ROWS) {
        band = band.child_sized(overflow_row(hidden, width, wash), 1);
    }
    debug_assert_eq!(
        band_rows(commands.len()),
        rows,
        "the band paints exactly the rows the panel gave it"
    );
    band.child_sized(frame_row(width, wash, false), 1)
}

/// The columns one band's rows are laid out in.
struct Columns {
    /// The columns the command cell takes.
    command: usize,
    /// The columns the description cell takes; `0` hides the column.
    description: usize,
    /// The columns the category cell takes; `0` hides the column.
    category: usize,
}

impl Columns {
    /// Returns the column widths of one band inside `inner` columns.
    ///
    /// The command and category columns are wide enough for every registered
    /// command, so the list never shifts as the filter narrows, and the
    /// description takes whatever is left over: a cell that drops out never
    /// leaves a hole beside the border, so every row fills exactly `inner`
    /// columns and never paints past its band. A band narrower than its own
    /// marker is degenerate everywhere, and the marker keeps its columns there
    /// rather than a cell that could not be read anyway.
    fn of(inner: usize) -> Self {
        let command = command_columns();
        let category = category_columns();
        let fixed = Self {
            command,
            description: 0,
            category,
        };
        // The description needs its own gap and at least one column of its own.
        if inner > fixed.used() + COLUMN_GAP {
            return Self {
                description: inner - fixed.used() - COLUMN_GAP,
                ..fixed
            };
        }
        // Too narrow for the description: the category goes first, and the
        // command cell then takes whatever the marker leaves.
        Self {
            command: inner.saturating_sub(MARKER_COLUMNS),
            description: 0,
            category: 0,
        }
    }

    /// Returns the columns these cells and their gaps fill.
    const fn used(&self) -> usize {
        let description = if self.description > 0 {
            COLUMN_GAP + self.description
        } else {
            0
        };
        let category = if self.category > 0 {
            COLUMN_GAP + self.category
        } else {
            0
        };
        MARKER_COLUMNS + self.command + description + category
    }
}

/// Returns the columns the command column is wide enough for: every registered
/// command.
fn command_columns() -> usize {
    COMMANDS
        .iter()
        .map(|command| display_width(&command.typed_name()))
        .max()
        .unwrap_or_default()
}

/// Returns the columns the category column is wide enough for: every registered
/// category.
fn category_columns() -> usize {
    COMMANDS
        .iter()
        .map(|command| display_width(command.category))
        .max()
        .unwrap_or_default()
}

/// Returns one command row: its marker, its name, its description, and its
/// category.
fn command_row(command: &Row, highlighted: bool, columns: &Columns, wash: Color) -> RichText {
    let row_wash = if highlighted {
        palette::ROW_SELECTED_BG
    } else {
        wash
    };
    let border = border_style(row_wash);
    let command_ink = {
        let mut style = Style::new().fg(palette::ACCENT_DEEP).bg(row_wash);
        style.bold = highlighted;
        style
    };
    let marker = if highlighted {
        (
            CURSOR_MARKER,
            Style::new().fg(palette::ACCENT).bg(row_wash).bold(),
        )
    } else {
        (ROW_MARKER, Style::new().bg(row_wash))
    };
    let trailing = border_style(row_wash);
    let mut row = RichText::new()
        .default_style(Style::new().bg(row_wash))
        .span(Span::styled("│ ", border))
        .span(Span::styled(marker.0, marker.1))
        .span(Span::styled(
            cell(&command.typed_name(), columns.command),
            command_ink,
        ));
    if columns.description > 0 {
        row = row
            .span(Span::styled(" ", Style::new().bg(row_wash)))
            .span(Span::styled(
                cell(command.description, columns.description),
                Style::new().fg(palette::INK).bg(row_wash),
            ));
    }
    if columns.category > 0 {
        row = row
            .span(Span::styled(" ", Style::new().bg(row_wash)))
            .span(Span::styled(
                cell(command.category, columns.category),
                Style::new().fg(palette::INK_MUTED).bg(row_wash),
            ));
    }
    row.span(Span::styled(" │", trailing))
}

/// Returns the row that reports the commands the band does not show.
fn overflow_row(hidden: usize, width: usize, wash: Color) -> RichText {
    let report = format!("… and {hidden} more commands");
    let columns = width.saturating_sub(BAND_CHROME_COLUMNS + MARKER_COLUMNS);
    RichText::new()
        .default_style(Style::new().bg(wash))
        .span(Span::styled("│ ", border_style(wash)))
        .span(Span::styled(ROW_MARKER, Style::new().bg(wash)))
        .span(Span::styled(
            cell(&report, columns),
            Style::new().fg(palette::INK_MUTED).bg(wash),
        ))
        .span(Span::styled(" │", border_style(wash)))
}

/// Returns the ink of the band's frame and padding on one wash.
fn border_style(wash: Color) -> Style {
    Style::new().fg(palette::ACCENT).bg(wash)
}

/// Returns the band's own top or bottom frame row.
///
/// The top row carries the band's title between its corners, exactly like the
/// transcript's framed blocks; the bottom row is its plain counterpart. A band
/// too narrow for the title keeps its corners and fills the row instead.
fn frame_row(width: usize, wash: Color, top: bool) -> RichText {
    let mut row = RichText::new().default_style(Style::new().bg(wash));
    if !top {
        return row
            .span(Span::styled("╰", border_style(wash)))
            .span(Span::styled(
                "─".repeat(width.saturating_sub(2)),
                border_style(wash),
            ))
            .span(Span::styled("╯", border_style(wash)));
    }
    let label = format!(" {TITLE} ");
    let labelled = display_width("╭─") + display_width(&label);
    if labelled + 1 > width {
        return row
            .span(Span::styled("╭─", border_style(wash)))
            .span(Span::styled(
                "─".repeat(width.saturating_sub(3)),
                border_style(wash),
            ))
            .span(Span::styled("╮", border_style(wash)));
    }
    row = row
        .span(Span::styled("╭─", border_style(wash)))
        .span(Span::styled(
            label,
            Style::new().fg(palette::ACCENT_DEEP).bg(wash).bold(),
        ));
    row.span(Span::styled(
        "─".repeat(width - labelled - 1),
        border_style(wash),
    ))
    .span(Span::styled("╮", border_style(wash)))
}

/// Returns the display width of one piece of text.
fn display_width(text: &str) -> usize {
    text.chars()
        .map(|character| usize::from(char_width(character)))
        .sum()
}

/// Returns `text` in exactly `columns` display columns.
///
/// A longer text is cut and marked with an ellipsis, a shorter one is padded,
/// so a column is always exactly as wide as it claims.
fn cell(text: &str, columns: usize) -> String {
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

    use super::{
        BAND_FRAME_ROWS, Columns, MARKER_COLUMNS, MAX_COMMAND_ROWS, Row, band, band_rows,
        display_width,
    };

    /// Eight synthetic commands, so a band can outgrow its row cap.
    const FILLER: [(&str, &str, &str); 8] = [
        ("alpha", "the first synthetic command", "Fixture"),
        ("bravo", "the second synthetic command", "Fixture"),
        ("charlie", "the third synthetic command", "Fixture"),
        ("delta", "the fourth synthetic command", "Fixture"),
        ("echo", "the fifth synthetic command", "Fixture"),
        ("foxtrot", "the sixth synthetic command", "Fixture"),
        ("golf", "the seventh synthetic command", "Fixture"),
        ("hotel", "the eighth synthetic command", "Fixture"),
    ];

    /// The columns every band in these tests is drawn in.
    const BAND_WIDTH: u16 = 72;

    /// Returns the rows of the synthetic command list.
    fn filler_rows() -> Vec<Row<'static>> {
        FILLER
            .iter()
            .map(|(name, description, category)| Row {
                name,
                description,
                category,
            })
            .collect()
    }

    #[test]
    fn band_rows_counts_the_frame_the_shown_rows_and_the_overflow_report() {
        assert_eq!(band_rows(0), BAND_FRAME_ROWS, "a band is a frame");
        assert_eq!(band_rows(2), BAND_FRAME_ROWS + 2);
        assert_eq!(band_rows(MAX_COMMAND_ROWS), BAND_FRAME_ROWS + 5);
        assert_eq!(
            band_rows(MAX_COMMAND_ROWS + 1),
            BAND_FRAME_ROWS + 6,
            "the first hidden command costs one row: the report"
        );
        assert_eq!(
            band_rows(FILLER.len()),
            BAND_FRAME_ROWS + 6,
            "every further hidden command is reported on that same row"
        );
    }

    #[test]
    fn the_columns_and_their_gaps_fill_the_band_exactly() {
        for inner in [MARKER_COLUMNS, MARKER_COLUMNS + 1, 12, 13, 20, 21, 40, 100] {
            assert_eq!(
                Columns::of(inner).used(),
                inner,
                "the cells and their gaps fill {inner} columns exactly"
            );
        }
    }

    #[test]
    fn a_wide_band_shows_all_three_columns_and_a_narrow_one_drops_them_whole() {
        let wide = Columns::of(80);
        assert!(
            wide.command >= display_width("/sessions"),
            "the command column fits every registered command"
        );
        assert!(wide.description > 0, "a wide band shows the description");
        assert!(wide.category > 0, "a wide band shows the category");

        let two_columns = Columns::of(MARKER_COLUMNS + wide.command);
        assert_eq!(
            two_columns.category, 0,
            "the category is the first column a narrow band drops"
        );
        assert_eq!(
            two_columns.description, 0,
            "a band with no room for the description drops it whole"
        );
        assert_eq!(
            two_columns.command, wide.command,
            "the command column keeps every registered command visible"
        );

        let degenerate = Columns::of(MARKER_COLUMNS - 1);
        assert_eq!(
            degenerate.command, 0,
            "below the marker's own width no cell can be shown"
        );
    }

    #[test]
    fn the_band_paints_exactly_the_rows_it_counts_and_fills_every_one() {
        let rows = &filler_rows()[..2];
        let band = band(rows, 0, usize::from(BAND_WIDTH), band_rows(rows.len()));
        let app = TestApp::with_size(band, BAND_WIDTH, band_rows(rows.len()));
        let last = BAND_WIDTH - 1;
        assert_eq!(app.get_cell(0, 0), Some('╭'));
        assert_eq!(app.get_cell(last, 0), Some('╮'));
        assert_eq!(app.get_cell(0, 1), Some('│'));
        assert_eq!(app.get_cell(last, 1), Some('│'));
        assert_eq!(app.get_cell(0, 2), Some('│'));
        assert_eq!(app.get_cell(last, 2), Some('│'));
        assert_eq!(app.get_cell(0, 3), Some('╰'));
        assert_eq!(app.get_cell(last, 3), Some('╯'));
        assert_eq!(app.get_cell(0, 4), None, "the band paints no row below it");
    }

    #[test]
    fn the_band_highlights_one_row_and_marks_the_hidden_commands() {
        let rows = filler_rows();
        let band = band(&rows, 1, usize::from(BAND_WIDTH), band_rows(rows.len()));
        let app = TestApp::with_size(band, BAND_WIDTH, band_rows(rows.len()));
        let text = app.screen_text();
        assert!(text.contains("commands"), "the frame titles the band");
        assert!(text.contains("/alpha"), "the first command is shown");
        assert!(
            text.contains("/echo"),
            "the fifth command is the last shown"
        );
        assert!(
            !text.contains("/foxtrot"),
            "the row cap hides the sixth command"
        );
        assert!(
            text.contains("… and 3 more commands"),
            "the hidden commands are reported on the row the cap leaves"
        );
        assert!(
            text.contains("the first synthetic command"),
            "the description column"
        );
        assert!(text.contains("Fixture"), "the category column");
        assert_eq!(
            text.matches("> ").count(),
            1,
            "exactly one row carries the highlight marker"
        );
        assert!(
            app.get_line(2).contains("> /bravo"),
            "the highlight follows the state"
        );
        assert!(
            !app.get_line(1).contains("> "),
            "the other rows keep the aligned blank marker"
        );
    }
}
