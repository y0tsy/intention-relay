//! The input's command hint band: the commands the first word selects, or the
//! values of the argument word the caret sits in.
//!
//! The band is one region of the chat panel directly above the input block,
//! and the transcript gives up exactly the rows it takes: the panel's body
//! still sums to transcript plus band plus input plus detail, so nothing is
//! painted over anything and the one window never grows a second surface.
//!
//! The band is its own rounded frame on the header wash, with the three columns
//! `[Command] [Description] [Category]` in that order while it ranks the
//! registry, and `[Value] [Description] [Argument]` while it ranks one
//! declared argument's values: the same three cells carry a value, that value's
//! one line, and the name of the argument it fills. The highlighted row carries
//! the sessions browser's own cursor vocabulary - the `> ` marker and the
//! selected wash - so the three lists read the same way, and the first column
//! keeps the accent ink the input line's own prompt uses.

use revue::style::Color;
use revue::text::char_width;
use revue::widget::{RichText, Span, Stack, Style, vstack};

use crate::app::{AppState, ArgumentSpec, COMMANDS, MenuKind, MenuRow};
use crate::tui::palette::Palette;

/// The most rows the band shows before it reports the rest.
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

/// The label the band's top border carries while it ranks the registry.
const COMMAND_TITLE: &str = "commands";

/// The label the band's top border carries while it ranks one argument.
const ARGUMENT_TITLE: &str = "arguments";

/// The noun the overflow row counts while the band ranks the registry.
const COMMAND_NOUN: &str = "commands";

/// The noun the overflow row counts while the band ranks one argument.
const VALUE_NOUN: &str = "values";

/// Returns the rows the band occupies for the state's current word, or `0`
/// while no band is open.
#[must_use]
pub(in crate::tui) fn menu_rows(state: &AppState) -> u16 {
    state.command_menu().map_or(0, |menu| band_rows(menu.len()))
}

/// Returns the rows a band of `rows` rows occupies: its own frame, the rows it
/// shows, and the one row its overflow report takes.
#[must_use]
const fn band_rows(rows: usize) -> u16 {
    let shown = if rows > MAX_COMMAND_ROWS {
        MAX_COMMAND_ROWS
    } else {
        rows
    };
    let shown = if shown > u16::MAX as usize {
        u16::MAX
    } else {
        shown as u16
    };
    let overflow = if rows > MAX_COMMAND_ROWS { 1 } else { 0 };
    BAND_FRAME_ROWS
        .saturating_add(shown)
        .saturating_add(overflow)
}

/// Returns the band: the word's framed rows, exactly `rows` display rows tall.
///
/// `rows` is [`menu_rows`] for the same state, so the band and the panel's
/// arithmetic agree by construction - and a panel too small to hold the band
/// paints nothing at all rather than a band in the input's rows.
pub(in crate::tui) fn menu_block(
    state: &AppState,
    width: u16,
    rows: u16,
    palette: &'static Palette,
) -> Stack {
    let Some(menu) = state.command_menu() else {
        return vstack();
    };
    // @todo(hack): the band recomputes the panel's row arithmetic and refuses
    // to paint on a mismatch because revue squeezes an oversized child instead
    // of negotiating a height; the guard exists only to keep the two
    // arithmetic paths in lockstep.
    if rows != band_rows(menu.len()) {
        return vstack();
    }
    let band_rows = menu.rows().map(Row::of).collect::<Vec<_>>();
    band(
        &band_rows,
        menu.highlight(),
        menu.kind(),
        usize::from(width),
        rows,
        palette,
    )
}

/// One row of the band: the three cells one command or value contributes.
struct Row {
    /// The first cell, exactly as the band paints it.
    label: String,
    /// The row's one-line description.
    description: &'static str,
    /// The command's category or the argument's name.
    trailing: &'static str,
}

impl Row {
    /// Returns the row one menu row contributes.
    fn of(row: MenuRow) -> Self {
        Self {
            label: row.label(),
            description: row.description(),
            trailing: row.trailing(),
        }
    }
}

/// Returns the band of one row list with one row highlighted.
fn band(
    rows: &[Row],
    highlight: usize,
    kind: MenuKind,
    width: usize,
    height: u16,
    palette: &'static Palette,
) -> Stack {
    let wash = palette.header_bg;
    let columns = Columns::of(kind, width.saturating_sub(BAND_CHROME_COLUMNS));
    let (title, noun) = match kind {
        MenuKind::Command => (COMMAND_TITLE, COMMAND_NOUN),
        MenuKind::Argument(_) => (ARGUMENT_TITLE, VALUE_NOUN),
    };
    let mut band = vstack().child_sized(frame_row(width, wash, title, true, palette), 1);
    for (row, entry) in rows.iter().take(MAX_COMMAND_ROWS).enumerate() {
        band = band.child_sized(
            command_row(entry, row == highlight, &columns, wash, palette),
            1,
        );
    }
    if let Some(hidden) = rows.len().checked_sub(MAX_COMMAND_ROWS) {
        band = band.child_sized(overflow_row(hidden, noun, width, wash, palette), 1);
    }
    debug_assert_eq!(
        band_rows(rows.len()),
        height,
        "the band paints exactly the rows the panel gave it"
    );
    band.child_sized(frame_row(width, wash, title, false, palette), 1)
}

/// The columns one band's rows are laid out in.
struct Columns {
    /// The columns the first cell takes: a command name or an argument value.
    label: usize,
    /// The columns the description cell takes; `0` hides the column.
    description: usize,
    /// The columns the trailing cell takes: a category or an argument name;
    /// `0` hides the column.
    trailing: usize,
}

impl Columns {
    /// Returns the column widths of one band inside `inner` columns.
    ///
    /// The first and trailing columns are wide enough for every row the word
    /// can show - every registered command and category, or every value the
    /// argument declares and the argument's name - so the list never shifts as
    /// the filter narrows, and the description takes whatever is left over: a
    /// cell that drops out never leaves a hole beside the border, so every row
    /// fills exactly `inner` columns and never paints past its band. A band
    /// narrower than its own marker is degenerate everywhere, and the marker
    /// keeps its columns there rather than a cell that could not be read
    /// anyway.
    fn of(kind: MenuKind, inner: usize) -> Self {
        let (label, trailing) = match kind {
            MenuKind::Command => (command_columns(), category_columns()),
            MenuKind::Argument(argument) => (value_columns(argument), display_width(argument.name)),
        };
        let fixed = Self {
            label,
            description: 0,
            trailing,
        };
        // The description needs its own gap and at least one column of its own.
        if inner > fixed.used() + COLUMN_GAP {
            return Self {
                description: inner - fixed.used() - COLUMN_GAP,
                ..fixed
            };
        }
        // Too narrow for the description: the trailing cell goes first, and the
        // first cell then takes whatever the marker leaves.
        Self {
            label: inner.saturating_sub(MARKER_COLUMNS),
            description: 0,
            trailing: 0,
        }
    }

    /// Returns the columns these cells and their gaps fill.
    const fn used(&self) -> usize {
        let description = if self.description > 0 {
            COLUMN_GAP + self.description
        } else {
            0
        };
        let trailing = if self.trailing > 0 {
            COLUMN_GAP + self.trailing
        } else {
            0
        };
        MARKER_COLUMNS + self.label + description + trailing
    }
}

/// Returns the columns the first cell is wide enough for: every registered
/// command.
fn command_columns() -> usize {
    COMMANDS
        .iter()
        .map(|command| display_width(&command.typed_name()))
        .max()
        .unwrap_or_default()
}

/// Returns the columns the trailing cell is wide enough for: every registered
/// category.
fn category_columns() -> usize {
    COMMANDS
        .iter()
        .map(|command| display_width(command.category))
        .max()
        .unwrap_or_default()
}

/// Returns the columns the first cell is wide enough for: every value the
/// argument declares.
fn value_columns(argument: ArgumentSpec) -> usize {
    argument
        .values
        .iter()
        .map(|value| display_width(value.value))
        .max()
        .unwrap_or_default()
}

/// Returns one band row: its marker, its first cell, its description, and its
/// trailing cell.
fn command_row(
    row: &Row,
    highlighted: bool,
    columns: &Columns,
    wash: Color,
    palette: &'static Palette,
) -> RichText {
    let row_wash = if highlighted {
        palette.row_selected_bg
    } else {
        wash
    };
    let border = border_style(row_wash, palette);
    let label_ink = {
        let mut style = Style::new().fg(palette.accent_deep).bg(row_wash);
        style.bold = highlighted;
        style
    };
    let marker = if highlighted {
        (
            CURSOR_MARKER,
            Style::new().fg(palette.accent).bg(row_wash).bold(),
        )
    } else {
        (ROW_MARKER, Style::new().bg(row_wash))
    };
    let trailing = border_style(row_wash, palette);
    let mut line = RichText::new()
        .default_style(Style::new().bg(row_wash))
        .span(Span::styled("│ ", border))
        .span(Span::styled(marker.0, marker.1))
        .span(Span::styled(cell(&row.label, columns.label), label_ink));
    if columns.description > 0 {
        line = line
            .span(Span::styled(" ", Style::new().bg(row_wash)))
            .span(Span::styled(
                cell(row.description, columns.description),
                Style::new().fg(palette.ink).bg(row_wash),
            ));
    }
    if columns.trailing > 0 {
        line = line
            .span(Span::styled(" ", Style::new().bg(row_wash)))
            .span(Span::styled(
                cell(row.trailing, columns.trailing),
                Style::new().fg(palette.ink_muted).bg(row_wash),
            ));
    }
    line.span(Span::styled(" │", trailing))
}

/// Returns the row that reports the rows the band does not show.
fn overflow_row(
    hidden: usize,
    noun: &str,
    width: usize,
    wash: Color,
    palette: &'static Palette,
) -> RichText {
    let report = format!("… and {hidden} more {noun}");
    let columns = width.saturating_sub(BAND_CHROME_COLUMNS + MARKER_COLUMNS);
    RichText::new()
        .default_style(Style::new().bg(wash))
        .span(Span::styled("│ ", border_style(wash, palette)))
        .span(Span::styled(ROW_MARKER, Style::new().bg(wash)))
        .span(Span::styled(
            cell(&report, columns),
            Style::new().fg(palette.ink_muted).bg(wash),
        ))
        .span(Span::styled(" │", border_style(wash, palette)))
}

/// Returns the ink of the band's frame and padding on one wash.
fn border_style(wash: Color, palette: &'static Palette) -> Style {
    Style::new().fg(palette.accent).bg(wash)
}

/// Returns the band's own top or bottom frame row.
///
/// The top row carries the band's title between its corners, exactly like the
/// transcript's framed blocks; the bottom row is its plain counterpart. A band
/// too narrow for the title keeps its corners and fills the row instead.
fn frame_row(
    width: usize,
    wash: Color,
    title: &str,
    top: bool,
    palette: &'static Palette,
) -> RichText {
    let mut row = RichText::new().default_style(Style::new().bg(wash));
    if !top {
        return row
            .span(Span::styled("╰", border_style(wash, palette)))
            .span(Span::styled(
                "─".repeat(width.saturating_sub(2)),
                border_style(wash, palette),
            ))
            .span(Span::styled("╯", border_style(wash, palette)));
    }
    let label = format!(" {title} ");
    let labelled = display_width("╭─") + display_width(&label);
    if labelled + 1 > width {
        return row
            .span(Span::styled("╭─", border_style(wash, palette)))
            .span(Span::styled(
                "─".repeat(width.saturating_sub(3)),
                border_style(wash, palette),
            ))
            .span(Span::styled("╮", border_style(wash, palette)));
    }
    row = row
        .span(Span::styled("╭─", border_style(wash, palette)))
        .span(Span::styled(
            label,
            Style::new().fg(palette.accent_deep).bg(wash).bold(),
        ));
    row.span(Span::styled(
        "─".repeat(width - labelled - 1),
        border_style(wash, palette),
    ))
    .span(Span::styled("╮", border_style(wash, palette)))
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
    use crate::app::{ArgumentSpec, MenuKind, ValueSpec};
    use crate::tui::palette::Palette;

    /// The light palette every fixture renders with.
    const LIGHT: &Palette = &crate::tui::palette::LIGHT;

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

    /// Two synthetic values for the argument-band fixture.
    static VALUES: [ValueSpec; 2] = [
        ValueSpec {
            value: "round",
            description: "the round fixture shape",
        },
        ValueSpec {
            value: "square",
            description: "the square fixture shape",
        },
    ];

    /// The synthetic argument the value-band fixture ranks.
    static ARGUMENT: [ArgumentSpec; 1] = [ArgumentSpec {
        name: "shape",
        values: &VALUES,
        required: false,
    }];

    /// Returns the rows of the synthetic command list.
    fn filler_rows() -> Vec<Row> {
        FILLER
            .iter()
            .map(|(name, description, category)| Row {
                label: format!("/{name}"),
                description,
                trailing: category,
            })
            .collect()
    }

    /// Returns the rows of the synthetic value list.
    fn value_rows() -> Vec<Row> {
        VALUES
            .iter()
            .map(|value| Row {
                label: value.value.to_owned(),
                description: value.description,
                trailing: ARGUMENT[0].name,
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
            "the first hidden row costs one row: the report"
        );
        assert_eq!(
            band_rows(FILLER.len()),
            BAND_FRAME_ROWS + 6,
            "every further hidden row is reported on that same row"
        );
    }

    #[test]
    fn the_columns_and_their_gaps_fill_the_band_exactly() {
        for modifier in [MenuKind::Command, MenuKind::Argument(ARGUMENT[0])] {
            for inner in [MARKER_COLUMNS, MARKER_COLUMNS + 1, 12, 13, 20, 21, 40, 100] {
                assert_eq!(
                    Columns::of(modifier, inner).used(),
                    inner,
                    "the cells and their gaps fill {inner} columns exactly for {modifier:?}"
                );
            }
        }
    }

    #[test]
    fn a_wide_band_shows_all_three_columns_and_a_narrow_one_drops_them_whole() {
        let wide = Columns::of(MenuKind::Command, 80);
        assert!(
            wide.label >= display_width("/sessions"),
            "the first column fits every registered command"
        );
        assert!(wide.description > 0, "a wide band shows the description");
        assert!(wide.trailing > 0, "a wide band shows the trailing column");

        let two_columns = Columns::of(MenuKind::Command, MARKER_COLUMNS + wide.label);
        assert_eq!(
            two_columns.trailing, 0,
            "the trailing cell is the first column a narrow band drops"
        );
        assert_eq!(
            two_columns.description, 0,
            "a band with no room for the description drops it whole"
        );
        assert_eq!(
            two_columns.label, wide.label,
            "the first column keeps every registered command visible"
        );

        let degenerate = Columns::of(MenuKind::Command, MARKER_COLUMNS - 1);
        assert_eq!(
            degenerate.label, 0,
            "below the marker's own width no cell can be shown"
        );
    }

    #[test]
    fn an_argument_band_sizes_its_cells_from_the_declared_values() {
        let columns = Columns::of(MenuKind::Argument(ARGUMENT[0]), 80);
        assert!(
            columns.label >= display_width("square"),
            "the first column fits every declared value"
        );
        assert!(
            columns.trailing >= display_width("shape"),
            "the trailing column names the argument"
        );
        assert!(
            columns.label < display_width("/sessions"),
            "a value band does not reserve the command vocabulary's columns"
        );
    }

    #[test]
    fn the_band_paints_exactly_the_rows_it_counts_and_fills_every_one() {
        let rows = &filler_rows()[..2];
        let band = band(
            rows,
            0,
            MenuKind::Command,
            usize::from(BAND_WIDTH),
            band_rows(rows.len()),
            LIGHT,
        );
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
        let band = band(
            &rows,
            1,
            MenuKind::Command,
            usize::from(BAND_WIDTH),
            band_rows(rows.len()),
            LIGHT,
        );
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
        assert!(text.contains("Fixture"), "the trailing column");
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

    #[test]
    fn a_value_band_lists_the_arguments_values_in_its_own_three_cells() {
        let rows = value_rows();
        let band = band(
            &rows,
            1,
            MenuKind::Argument(ARGUMENT[0]),
            usize::from(BAND_WIDTH),
            band_rows(rows.len()),
            LIGHT,
        );
        let app = TestApp::with_size(band, BAND_WIDTH, band_rows(rows.len()));
        let text = app.screen_text();
        assert!(
            text.contains("arguments"),
            "the frame titles the argument band"
        );
        assert!(text.contains("round"), "the first value is shown");
        assert!(text.contains("square"), "the second value is shown");
        assert!(
            text.contains("the round fixture shape"),
            "the description column"
        );
        assert!(
            text.contains("shape"),
            "the trailing column names the argument"
        );
        assert!(
            app.get_line(2).contains("> square"),
            "the highlight follows the state"
        );
    }
}
