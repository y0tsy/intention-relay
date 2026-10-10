//! The sessions browser: one panel docked to the bottom of the chat window.
//!
//! The browser never paints over the transcript: it is a layout row at the
//! bottom of the window, and the chat container above it reflows into the rows
//! the panel leaves. One blank canvas row separates the chat frame's bottom
//! edge from the panel's top border, and the panel's own bottom edge is the
//! window's last row. The panel is its whole surface - a frame whose top border
//! carries the `Sessions` title, a tab radio row, a search bar, the session
//! table, and the hotkey legend as the panel's own last inner row - so a tall
//! window never separates the panel from its legend. It grows with the result
//! up to half the window, so the transcript keeps the majority of it, and the
//! list scrolls inside the panel from there.
//!
//! Every value here is a pure function of the application state, of the
//! wall-clock second the view received, and of the window: the rows, their
//! filter, their order, and the cursor are the core's, the relative time is the
//! one value the view derives (the core carries no clock), and the rest is
//! geometry.

use std::ops::Range;

use revue::layout::Rect;
use revue::style::Color;
use revue::text::char_width;
use revue::widget::{Border, Card, RichText, Span, Style, Text, vstack};

use crate::app::{AppState, BrowserRow, BrowserTab};
use crate::tui::palette::{self, Palette};

/// The blank canvas row the chat container keeps above the docked panel.
///
/// It is the chat panel's own bottom margin row, so the geometry only reserves
/// room for it; nothing paints it.
const PANEL_GAP_ROWS: u16 = 1;

/// The rows and columns the panel's frame takes.
const PANEL_FRAME: u16 = 2;

/// The columns the panel pads its content with.
const PANEL_PADDING: u16 = 2;

/// The body rows that are not session rows: the leading gap, the tab radio row,
/// the search bar, two divider bands, and the table's column header.
const CHROME_ROWS: u16 = 6;

/// The rows the panel's footer occupies: the row the panel reserves for its
/// footer separator, then the legend row itself.
const FOOTER_ROWS: u16 = 2;

/// The fewest session rows the panel shows, so the table always has a body.
const MIN_DATA_ROWS: u16 = 1;

/// The fewest rows the docked panel occupies, however small the result.
const MIN_PANEL_ROWS: u16 = 6;

/// The fewest rows the chat container keeps above the panel and its gap.
const MIN_CHAT_ROWS: u16 = 11;

/// The columns one session row's marker occupies.
const MARKER_WIDTH: usize = 2;

/// The one column between two table columns.
const COLUMN_GAP: usize = 1;

/// The share of the flexible table width each purpose column takes, in percent.
const MODIFIED_PERCENT: usize = 18;
const CREATED_PERCENT: usize = 12;
const SIZE_PERCENT: usize = 8;
const PATH_PERCENT: usize = 16;

/// The fewest columns each purpose column keeps, so a narrow card still names
/// its columns instead of collapsing them.
const MIN_MODIFIED: usize = 8;
const MIN_CREATED: usize = 7;
const MIN_SIZE: usize = 4;
const MIN_PATH: usize = 4;

/// The placeholder the search bar shows while the filter is empty.
const SEARCH_PLACEHOLDER: &str = "Type to filter sessions...";

/// The marker a cursor row starts with.
const CURSOR_MARKER: &str = "> ";

/// The marker every other row starts with, keeping the columns aligned.
const ROW_MARKER: &str = "  ";

/// The glyph of the active tab.
const ACTIVE_TAB: char = '⦿';

/// The glyph of every other tab.
const INACTIVE_TAB: char = '◦';

/// The value of a column the core cannot report yet.
const MISSING_VALUE: &str = "—";

/// The value of the `Created` column.
// @todo(core): the session summary carries no `created_at`.
const MISSING_CREATED: &str = MISSING_VALUE;

/// The value of the `Size` column.
// @todo(core): the session summary carries no step or message count.
const MISSING_SIZE: &str = MISSING_VALUE;

/// The value of the trailing `Path` column.
// @todo(core): the session summary carries no working folder.
const MISSING_PATH: &str = MISSING_VALUE;

/// The hotkey legend: every key, then the action it runs.
///
/// The two halves are styled and positioned apart, so a terminal that maps the
/// palette to plain ANSI still shows which word is the key and which is the
/// action. The arrow keys move the cursor but are not advertised: the wheel is
/// the familiar pattern for that, and the legend has no room for both.
const HINTS: [(&str, &str); 6] = [
    ("Enter", "select"),
    ("Tab", "switch tab"),
    ("Ctrl+R", "rename"),
    ("Ctrl+X", "archive"),
    ("Ctrl+F", "tree"),
    ("Esc", "close"),
];

/// The month labels a date older than a week shows.
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The seconds one minute holds.
const MINUTE_SECONDS: i64 = 60;

/// The seconds one hour holds.
const HOUR_SECONDS: i64 = 3_600;

/// The seconds one day holds.
const DAY_SECONDS: i64 = 86_400;

/// The age at which `Modified` reads as a date instead of a count of days.
const RELATIVE_DAYS: i64 = 7;

/// The width of the four table columns in one content width.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Columns {
    modified: usize,
    created: usize,
    size: usize,
    title: usize,
    path: usize,
}

/// One styled run of a row.
#[derive(Clone, Debug)]
struct Run {
    /// The text the run paints.
    text: String,
    /// The ink the text is written in.
    ink: Color,
    /// The wash behind the text.
    wash: Color,
    /// Whether the run is written bold.
    bold: bool,
}

impl Run {
    /// Returns one run of plain body text.
    const fn plain(text: String, palette: &'static Palette) -> Self {
        Self {
            text,
            ink: palette.ink,
            wash: palette.panel,
            bold: false,
        }
    }

    /// Returns the run written in another ink.
    const fn with_ink(mut self, ink: Color) -> Self {
        self.ink = ink;
        self
    }
}

/// The columns and rows the docked panel occupies in one window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Geometry {
    /// The columns the panel occupies.
    pub(super) columns: u16,
    /// The rows the panel occupies.
    pub(super) rows: u16,
    /// The session rows the table window shows.
    data_rows: usize,
    /// The columns the panel's content has.
    content_columns: usize,
}

/// Returns the geometry of the docked panel in one window.
///
/// The panel spans the window's width and is content-sized up to half the
/// window, so the transcript keeps the majority of it: `clamp(chrome +
/// rows_needed, 6, window_height / 2)`. A window too small to keep the chat
/// and the panel apart returns `None`, and the chat keeps the whole window
/// instead of a broken frame.
pub(super) fn geometry(state: &AppState, window: Rect) -> Option<Geometry> {
    if window.width <= PANEL_FRAME {
        return None;
    }
    let chrome = PANEL_FRAME + CHROME_ROWS + FOOTER_ROWS;
    let cap = MIN_PANEL_ROWS.max(window.height / 2);
    let data_cap = cap.saturating_sub(chrome).max(MIN_DATA_ROWS);
    let wanted = u16::try_from(state.browser_rows().len())
        .unwrap_or(u16::MAX)
        .max(MIN_DATA_ROWS);
    let data_rows = wanted.min(data_cap);
    let rows = (chrome + data_rows).clamp(MIN_PANEL_ROWS, cap);
    if window.height < rows.saturating_add(PANEL_GAP_ROWS + MIN_CHAT_ROWS) {
        return None;
    }
    Some(Geometry {
        columns: window.width,
        rows,
        data_rows: usize::from(data_rows),
        content_columns: usize::from(window.width.saturating_sub(PANEL_FRAME + PANEL_PADDING * 2)),
    })
}

/// Returns the panel: the titled frame around the browser's own container.
///
/// `Card` draws its title as a row inside its frame, and the `Sessions` title
/// belongs in the top border, so the frame is a `Border` that embeds it. The
/// `Card` inside draws no border of its own and owns the padding, the
/// background fill, and the footer legend, so the two read as one container
/// with the title embedded in its top border. The caller places the panel in
/// the window's last [`Geometry::rows`] rows.
///
/// The theme is resolved here, once per frame, and handed to every row the
/// panel draws.
pub(super) fn panel(state: &AppState, now: i64, geometry: Geometry) -> Border {
    let palette = palette::of(state.theme());
    let width = geometry.content_columns;
    let columns = columns(width);
    let mut body = vstack()
        .child_sized(Text::new(""), 1)
        .child_sized(tabs_row(state, width, palette), 1)
        .child_sized(search_bar(state, width, palette), 1)
        .child_sized(divider(width, palette), 1)
        .child_sized(table_header(&columns, width, palette), 1);
    let rows = state.browser_rows();
    if rows.is_empty() {
        let reason = state.browser_empty_reason();
        if let Some(reason) = reason {
            body = body.child_sized(empty_state(reason, width, palette), 1);
        }
        let filler = geometry
            .data_rows
            .saturating_sub(usize::from(reason.is_some()));
        if filler > 0 {
            let filler = u16::try_from(filler).unwrap_or(u16::MAX);
            body = body.child_sized(Text::new(""), filler);
        }
    } else {
        let cursor = state.browser_cursor();
        for index in visible_window(rows.len(), cursor, geometry.data_rows) {
            let selected = index == cursor;
            body = body.child_sized(
                data_row(
                    &rows[index],
                    selected,
                    index % 2 == 1,
                    &columns,
                    now,
                    width,
                    palette,
                ),
                1,
            );
        }
    }
    body = body.child_sized(divider(width, palette), 1);

    Border::rounded()
        .min_size(geometry.columns, geometry.rows)
        .max_size(geometry.columns, geometry.rows)
        .fg(palette.accent)
        .bg(palette.panel)
        .title(" Sessions ")
        .child(
            Card::new()
                .flat()
                .padding(PANEL_PADDING)
                .background(palette.panel)
                .body(body)
                .footer(footer(state, geometry.data_rows, width, palette)),
        )
}

/// Returns the tab radio row: `⦿` on the active tab, `◦` on every other one.
fn tabs_row(state: &AppState, width: usize, palette: &'static Palette) -> RichText {
    let mut runs = vec![Run {
        text: " ".to_owned(),
        ink: palette.ink_muted,
        wash: palette.header_bg,
        bold: false,
    }];
    for (index, tab) in BrowserTab::ALL.into_iter().enumerate() {
        if index > 0 {
            runs.push(Run {
                text: "  |  ".to_owned(),
                ink: palette.ink_faint,
                wash: palette.header_bg,
                bold: false,
            });
        }
        let active = tab == state.browser_tab();
        runs.push(Run {
            text: format!(
                "{} {}",
                if active { ACTIVE_TAB } else { INACTIVE_TAB },
                tab.label()
            ),
            ink: if active {
                palette.accent_deep
            } else {
                palette.ink_muted
            },
            wash: if active {
                palette.tab_active_bg
            } else {
                palette.header_bg
            },
            bold: active,
        });
    }
    let filler = width.saturating_sub(run_widths(&runs));
    if filler > 0 {
        runs.push(Run {
            text: " ".repeat(filler),
            ink: palette.ink_muted,
            wash: palette.header_bg,
            bold: false,
        });
    }
    runs_text(runs)
}

/// Returns one row of styled runs as one widget.
///
/// One widget, not one child per run: a row of content-sized children is
/// squeezed proportionally when it overflows, which shreds every word into a
/// stump, while rich text keeps each run whole and clips the row at the card's
/// edge.
fn runs_text(runs: Vec<Run>) -> RichText {
    let mut text = RichText::new();
    for run in runs {
        let style = run_style(&run);
        text = text.span(Span::styled(run.text, style));
    }
    text
}

/// Returns the style one run is painted with.
fn run_style(run: &Run) -> Style {
    let style = Style::new().fg(run.ink).bg(run.wash);
    if run.bold { style.bold() } else { style }
}

/// Returns the display width of every run of one row.
fn run_widths(runs: &[Run]) -> usize {
    runs.iter().map(|run| display_width(&run.text)).sum()
}

/// Returns the search row: the filter text, or its placeholder while empty.
fn search_bar(state: &AppState, width: usize, palette: &'static Palette) -> Text {
    let (text, ink) = if state.browser_filter().is_empty() {
        (SEARCH_PLACEHOLDER.to_owned(), palette.ink_faint)
    } else {
        (state.browser_filter().to_owned(), palette.ink)
    };
    Text::new(pad(&truncate(&text, width), width))
        .fg(ink)
        .bg(palette.input_bg)
}

/// Returns one divider band across the card's content.
fn divider(width: usize, palette: &'static Palette) -> Text {
    Text::new("─".repeat(width))
        .fg(palette.ink_faint)
        .bg(palette.divider_tint)
}

/// Returns the table's column header row.
fn table_header(columns: &Columns, width: usize, palette: &'static Palette) -> Text {
    let cells = Cells {
        marker: ROW_MARKER,
        modified: "Modified",
        created: "Created",
        size: "Size",
        title: "Title",
        path: "Path",
    };
    Text::new(row_text(columns, &cells, width))
        .fg(palette.ink_muted)
        .bg(palette.header_bg)
        .bold()
}

/// Returns one session row, washed and inked by its state.
fn data_row(
    row: &BrowserRow,
    selected: bool,
    zebra: bool,
    columns: &Columns,
    now: i64,
    width: usize,
    palette: &'static Palette,
) -> Text {
    let modified = relative_time(row.updated_at(), now);
    let cells = Cells {
        marker: if selected { CURSOR_MARKER } else { ROW_MARKER },
        modified: &modified,
        created: MISSING_CREATED,
        size: MISSING_SIZE,
        title: row.title(),
        path: MISSING_PATH,
    };
    let line = row_text(columns, &cells, width);
    if selected {
        Text::new(line)
            .fg(palette.accent_deep)
            .bg(palette.row_selected_bg)
            .bold()
    } else if zebra {
        Text::new(line).fg(palette.ink).bg(palette.row_alt_bg)
    } else {
        Text::new(line).fg(palette.ink)
    }
}

/// Returns the explanatory row of an empty table.
fn empty_state(reason: &str, width: usize, palette: &'static Palette) -> Text {
    Text::new(pad(&truncate(reason, width), width))
        .fg(palette.accent_deep)
        .bg(palette.stub_bg)
}

/// Returns the footer: the notice or the hotkey legend, then the row counter.
///
/// The legend gives way before the counter does: hints that do not fit beside
/// the counter are dropped whole, so the row never ends in half a key name.
fn footer(state: &AppState, data_rows: usize, width: usize, palette: &'static Palette) -> RichText {
    let counter = counter(state, data_rows);
    let counter_width = display_width(&counter);
    let mut runs = state.notice().map_or_else(
        || legend_runs(width.saturating_sub(counter_width + 1), palette),
        |notice| {
            vec![Run {
                text: truncate(notice, width),
                ink: palette.warning,
                wash: palette.panel,
                bold: true,
            }]
        },
    );
    let filler = width.saturating_sub(run_widths(&runs) + counter_width);
    if filler > 0 {
        runs.push(Run::plain(" ".repeat(filler), palette));
    }
    runs.push(Run {
        text: counter,
        ink: palette.ink_muted,
        wash: palette.panel,
        bold: false,
    });
    runs_text(runs)
}

/// Returns the legend runs that fit in `limit` columns.
fn legend_runs(limit: usize, palette: &'static Palette) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut used = 0;
    for (index, (key, action)) in HINTS.iter().enumerate() {
        let gap = if index > 0 { "  " } else { "" };
        let width = display_width(gap) + display_width(key) + 1 + display_width(action);
        if used + width > limit {
            break;
        }
        used += width;
        if !gap.is_empty() {
            runs.push(Run {
                text: gap.to_owned(),
                ink: palette.ink_faint,
                wash: palette.panel,
                bold: false,
            });
        }
        runs.push(Run {
            text: (*key).to_owned(),
            ink: palette.accent,
            wash: palette.panel,
            bold: true,
        });
        runs.push(Run::plain(format!(" {action}"), palette).with_ink(palette.ink_muted));
    }
    runs
}

/// One table line's cell values, in the order the columns show them.
struct Cells<'a> {
    /// The marker the row starts with.
    marker: &'a str,
    /// The `Modified` value.
    modified: &'a str,
    /// The `Created` value.
    created: &'a str,
    /// The `Size` value.
    size: &'a str,
    /// The `Title` value.
    title: &'a str,
    /// The `Path` value.
    path: &'a str,
}

/// Returns one table line with every column padded to its width.
fn row_text(columns: &Columns, cells: &Cells<'_>, width: usize) -> String {
    let line = format!(
        "{marker}{modified} {created} {size} {title} {path}",
        marker = cells.marker,
        modified = pad(cells.modified, columns.modified),
        created = pad(cells.created, columns.created),
        size = pad(cells.size, columns.size),
        title = pad(&truncate(cells.title, columns.title), columns.title),
        path = pad_start(cells.path, columns.path),
    );
    pad(&line, width)
}

/// Returns the column widths of one content width.
///
/// The purpose columns take a share of the flexible width each, so the table
/// stretches with the card; the title column takes what they leave and is the
/// only one that shrinks with the card.
fn columns(width: usize) -> Columns {
    let flexible = width.saturating_sub(MARKER_WIDTH + COLUMN_GAP * 4);
    let modified = share(flexible, MODIFIED_PERCENT).max(MIN_MODIFIED);
    let created = share(flexible, CREATED_PERCENT).max(MIN_CREATED);
    let size = share(flexible, SIZE_PERCENT).max(MIN_SIZE);
    let path = share(flexible, PATH_PERCENT).max(MIN_PATH);
    Columns {
        modified,
        created,
        size,
        title: flexible.saturating_sub(modified + created + size + path),
        path,
    }
}

/// Returns the percentage of one flexible width a column takes.
const fn share(width: usize, percent: usize) -> usize {
    width * percent / 100
}

/// Returns the row counter: the shown window, the matched total, and the
/// sessions the bounded list omitted.
fn counter(state: &AppState, data_rows: usize) -> String {
    let rows = state.browser_rows();
    let mut text = if rows.is_empty() {
        "0 of 0".to_owned()
    } else {
        let window = visible_window(rows.len(), state.browser_cursor(), data_rows);
        format!("{}-{} of {}", window.start + 1, window.end, rows.len())
    };
    // @todo(core): the session list is bounded, so the sessions past its window
    // cannot be paged to yet; the counter names them instead of hiding them.
    if state.sessions_omitted() > 0 {
        let omitted = state.sessions_omitted();
        text.push_str(&format!(" +{omitted} omitted (core: paged list @todo)"));
    }
    text
}

/// Returns the rows one pane of `height` rows shows, following the cursor.
///
/// The window keeps the cursor visible and moves as little as possible: a
/// cursor above the window pulls it up, a cursor below pulls it down, and a
/// result shorter than the pane fills the top of the pane.
fn visible_window(rows: usize, cursor: usize, height: usize) -> Range<usize> {
    if rows == 0 {
        return 0..0;
    }
    let height = height.clamp(1, rows);
    let start = cursor.saturating_sub(height - 1).min(rows - height);
    start..start + height
}

/// Returns the age of one durable update as the `Modified` column shows it.
///
/// Under a minute reads `now`, then whole minutes, hours, and days read
/// `Nm ago`, `Nh ago`, and `Nd ago`; a week and older reads as its month and
/// day (`Sep 19`). A timestamp ahead of the clock reads `now`.
fn relative_time(updated_at: i64, now: i64) -> String {
    let age = now.saturating_sub(updated_at);
    if age < MINUTE_SECONDS {
        return "now".to_owned();
    }
    if age < HOUR_SECONDS {
        return format!("{}m ago", age / MINUTE_SECONDS);
    }
    if age < DAY_SECONDS {
        return format!("{}h ago", age / HOUR_SECONDS);
    }
    if age < RELATIVE_DAYS * DAY_SECONDS {
        return format!("{}d ago", age / DAY_SECONDS);
    }
    let (month, day) = civil_month_day(updated_at);
    format!("{} {day}", MONTHS[month - 1])
}

/// Returns the calendar month and day of one Unix second.
///
/// The conversion is the civil-from-days algorithm: `days` counts from
/// 1970-01-01, the epoch is shifted to 0000-03-01 so a leap day closes a
/// 400-year era, and the era and the year inside it give the day of the year.
fn civil_month_day(unix_seconds: i64) -> (usize, u32) {
    let days = unix_seconds.div_euclid(DAY_SECONDS) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    (
        usize::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}

/// Returns the display width of one piece of text.
fn display_width(text: &str) -> usize {
    text.chars()
        .map(|character| usize::from(char_width(character)))
        .sum()
}

/// Returns `text` padded with spaces to exactly `width` columns.
fn pad(text: &str, width: usize) -> String {
    let mut padded = text.to_owned();
    padded.push_str(&" ".repeat(width.saturating_sub(display_width(text))));
    padded
}

/// Returns `text` right-aligned in `width` columns.
fn pad_start(text: &str, width: usize) -> String {
    let mut padded = " ".repeat(width.saturating_sub(display_width(text)));
    padded.push_str(text);
    padded
}

/// Returns `text` cut to `width` columns, marking a cut with an ellipsis.
fn truncate(text: &str, width: usize) -> String {
    if display_width(text) <= width {
        return text.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut cut = String::new();
    let mut used = 0;
    for character in text.chars() {
        let character_width = usize::from(char_width(character));
        if used + character_width > width - 1 {
            break;
        }
        cut.push(character);
        used += character_width;
    }
    cut.push('…');
    cut
}

#[cfg(test)]
mod tests {
    use super::{
        civil_month_day, columns, display_width, pad, pad_start, relative_time, truncate,
        visible_window,
    };

    #[test]
    fn the_relative_time_reads_now_minutes_hours_and_days() {
        assert_eq!(relative_time(1_000, 1_000), "now");
        assert_eq!(relative_time(1_000, 1_030), "now");
        assert_eq!(
            relative_time(2_000, 1_000),
            "now",
            "an update ahead of the clock reads now"
        );
        assert_eq!(relative_time(1_000, 1_380 + 1_000), "23m ago");
        assert_eq!(relative_time(1_000, 7_200 + 1_000), "2h ago");
        assert_eq!(relative_time(1_000, 3 * 86_400 + 1_000), "3d ago");
    }

    #[test]
    fn the_relative_time_reads_a_week_and_older_as_a_date() {
        let now = 1_759_000_000;
        assert_eq!(relative_time(now - 7 * 86_400, now), "Sep 20");
        assert_eq!(relative_time(now - 8 * 86_400, now), "Sep 19");
        assert_eq!(relative_time(0, now), "Jan 1");
    }

    #[test]
    fn the_civil_date_follows_the_epoch_and_leap_days() {
        assert_eq!(civil_month_day(0), (1, 1));
        assert_eq!(civil_month_day(1_759_000_000), (9, 27));
        assert_eq!(civil_month_day(951_782_400), (2, 29));
    }

    #[test]
    fn truncation_marks_a_cut_with_an_ellipsis() {
        assert_eq!(truncate("session 11111111", 16), "session 11111111");
        assert_eq!(truncate("session 11111111", 10), "session 1…");
        assert_eq!(truncate("abc", 1), "…");
        assert_eq!(truncate("abc", 0), "");
    }

    #[test]
    fn padding_and_alignment_fill_the_column_exactly() {
        assert_eq!(pad("ab", 4), "ab  ");
        assert_eq!(pad("abcd", 2), "abcd");
        assert_eq!(pad_start("ab", 4), "  ab");
        assert_eq!(display_width("ab"), 2);
    }

    #[test]
    fn the_window_follows_the_cursor_and_fills_a_short_result() {
        assert_eq!(visible_window(0, 0, 3), 0..0);
        assert_eq!(visible_window(2, 1, 20), 0..2);
        assert_eq!(visible_window(10, 0, 3), 0..3);
        assert_eq!(visible_window(10, 2, 3), 0..3);
        assert_eq!(visible_window(10, 4, 3), 2..5);
        assert_eq!(visible_window(10, 9, 3), 7..10);
    }

    #[test]
    fn the_title_column_takes_what_the_purpose_columns_leave() {
        let narrow = columns(30);
        let wide = columns(110);
        assert!(
            wide.title > narrow.title && wide.path > narrow.path && wide.modified > narrow.modified,
            "the table stretches with its card: {narrow:?} then {wide:?}"
        );
        for table in [narrow, wide] {
            assert!(
                table.modified >= 8 && table.created >= 7 && table.size >= 4 && table.path >= 4,
                "every purpose column keeps its minimum: {table:?}"
            );
            assert!(
                table.title >= 1,
                "the title column never collapses: {table:?}"
            );
        }
    }
}
