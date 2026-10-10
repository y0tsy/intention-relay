//! The input block: the badge header, one row per buffer line, and the run
//! status row.
//!
//! The block grows with the buffer: every line the buffer carries gets its own
//! display row, so a multi-line prompt stays visible while the transcript takes
//! the rows the block does not use. The chat panel decides those rows; a buffer
//! taller than the block's rows shows the window of lines that keeps the
//! cursor's line visible, exactly as a line longer than the block shows the
//! window of columns that keeps the cursor's cell visible. The buffer itself
//! keeps every line and every character: neither its line count nor a line's
//! length is ever capped.

use revue::text::char_width;
use revue::widget::{Border, Card, RichText, Span, Stack, Style, vstack};

use crate::app::AppState;
use crate::tui::palette::Palette;
use crate::tui::panes::status;

/// The prompt the buffer's first line carries before its text.
const PROMPT: &str = "> ";

/// The columns every later line keeps before its text, aligned under the first
/// line's text.
const CONTINUATION_INDENT: &str = "  ";

/// The marker rendered at the input cursor position.
///
/// `RichText` styles one run at a time, so the cursor is a block glyph rather
/// than a reversed cell: `█` occupies the character cell the cursor points at,
/// and the cell after the last character at the line's end.
// @todo(revue): revue has no editable text surface - no cell cursor, no buffer
// scrolling, no clipping - so this pane hand-rolls the block-glyph cursor, the
// horizontal window around it, and the row arithmetic; a real text widget
// would own all three.
const CURSOR: char = '█';

/// The columns and rows the block's own frame takes.
const BLOCK_FRAME: u16 = 2;

/// The columns the block pads its content with.
const BLOCK_PADDING: u16 = 1;

/// The rows the block's badge header occupies.
const BADGE_ROWS: u16 = 1;

/// The rows the run status under the buffer occupies.
const STATUS_ROWS: u16 = 1;

/// The rows the block occupies for a one-line buffer: its frame, the badge
/// header, that line, and the run status row.
pub(in crate::tui) const MIN_BLOCK_ROWS: u16 = BLOCK_FRAME + BADGE_ROWS + 1 + STATUS_ROWS;

/// The mode badge before a session snapshot has named the mode.
///
/// The mode is durable session state and arrives with the snapshot, so the
/// badge waits for it rather than guessing `build`.
const MODE_PLACEHOLDER: &str = "mode @todo(core)";

/// The model badge: the session projection carries no provider model.
// @todo(core): the session projection carries no provider model, so this badge
// stays an explicit placeholder rather than a fabricated model name.
const MODEL_PLACEHOLDER: &str = "model @todo(core)";

/// Returns the rows the input block needs for the state's buffer.
///
/// A one-line buffer takes [`MIN_BLOCK_ROWS`] and every line break adds one
/// row, so a multi-line prompt grows the block and the transcript gives up
/// exactly those rows. No line count is capped: the chat panel bounds what it
/// can lay out, and the block windows a taller buffer.
#[must_use]
pub(in crate::tui) fn block_rows(state: &AppState) -> u16 {
    let breaks = state
        .input()
        .chars()
        .filter(|character| *character == '\n')
        .count();
    MIN_BLOCK_ROWS.saturating_add(u16::try_from(breaks).unwrap_or(u16::MAX))
}

/// Returns the input block: one focused frame around the badge header, the
/// buffer's lines, and the run status row under them.
///
/// `rows` is the block's exact row count, decided by the chat panel: one row
/// per buffer line beside the fixed chrome. A buffer taller than the rows left
/// for lines shows the window that keeps the cursor's line visible.
pub(in crate::tui) fn input_block(
    state: &AppState,
    width: u16,
    rows: u16,
    palette: &'static Palette,
) -> Border {
    let content = usize::from(width.saturating_sub(BLOCK_FRAME + BLOCK_PADDING * 2));
    let line_rows = rows.saturating_sub(BLOCK_FRAME + BADGE_ROWS + STATUS_ROWS);
    Border::rounded()
        .min_size(width, rows)
        .max_size(width, rows)
        .fg(palette.accent)
        .bg(palette.panel)
        .child(
            Card::new()
                .flat()
                .padding(BLOCK_PADDING)
                .background(palette.panel)
                .body(
                    vstack()
                        .child_sized(badge_row(state, content, palette), BADGE_ROWS)
                        .child_sized(
                            input_lines(state, content, usize::from(line_rows), palette),
                            line_rows,
                        )
                        .child_sized(status::status_row(state, palette), STATUS_ROWS),
                ),
        )
}

/// Returns the badge header: the session's real run mode and the model
/// placeholder, each on its own badge wash.
fn badge_row(state: &AppState, width: usize, palette: &'static Palette) -> RichText {
    let (mode, mode_ink) = state
        .session_mode()
        .map_or((MODE_PLACEHOLDER, palette.todo_ink), |mode| {
            (mode.as_str(), palette.badge_ink)
        });
    let mut text = RichText::new();
    let mut used = 0;
    for (label, ink) in [(mode, mode_ink), (MODEL_PLACEHOLDER, palette.todo_ink)] {
        let chip = format!(" {label} ");
        let chip_width = display_width(&chip);
        let gap = usize::from(used > 0);
        if used + gap + chip_width > width {
            break;
        }
        if gap > 0 {
            text = text.span(Span::styled(
                " ",
                Style::new().fg(palette.ink).bg(palette.panel),
            ));
            used += 1;
        }
        text = text.span(Span::styled(
            chip,
            Style::new().fg(ink).bg(palette.badge_bg),
        ));
        used += chip_width;
    }
    if used < width {
        text = text.span(Span::styled(
            " ".repeat(width - used),
            Style::new().bg(palette.panel),
        ));
    }
    text
}

/// Returns the buffer's visible rows: one row per line, windowed so the
/// cursor's line is always rendered.
fn input_lines(state: &AppState, width: usize, visible: usize, palette: &'static Palette) -> Stack {
    let buffer = BufferLines::of(state.input(), state.cursor());
    let start = window_start(buffer.cursor_line, visible);
    let mut body = vstack();
    for (index, line) in buffer.lines.iter().enumerate().skip(start).take(visible) {
        let cursor = (index == buffer.cursor_line).then_some(buffer.cursor_column);
        body = body.child_sized(input_row(index, line, cursor, width, palette), 1);
    }
    body
}

/// Returns one buffer line's row: its prompt or indent, its characters, and the
/// cursor when the line carries it, all on the input fill.
///
/// The first line carries the prompt and every later line the indent, so the
/// buffer's lines align under the first line's text. The cursor's row is
/// windowed horizontally when its line is longer than the block: the window
/// ends at the cursor, so the cursor stays visible while the text before it
/// scrolls off. A line without the cursor is clipped to the row.
fn input_row(
    index: usize,
    line: &str,
    cursor: Option<usize>,
    width: usize,
    palette: &'static Palette,
) -> RichText {
    let first = index == 0;
    let prefix = if first { PROMPT } else { CONTINUATION_INDENT };
    let prefix_width = display_width(prefix);
    let prefix_style = if first {
        Style::new().fg(palette.accent).bg(palette.input_bg).bold()
    } else {
        Style::new().bg(palette.input_bg)
    };
    let columns = width.saturating_sub(prefix_width);
    let mut text = RichText::new().span(Span::styled(prefix, prefix_style));
    let used = match cursor {
        Some(cursor) => {
            let (before, after) = cursor_window(line, cursor, columns);
            let used = prefix_width + display_width(&before) + 1 + display_width(&after);
            text = text
                .span(Span::styled(
                    before,
                    Style::new().fg(palette.ink).bg(palette.input_bg),
                ))
                .span(Span::styled(
                    CURSOR.to_string(),
                    Style::new().fg(palette.accent).bg(palette.input_bg).bold(),
                ))
                .span(Span::styled(
                    after,
                    Style::new().fg(palette.ink).bg(palette.input_bg),
                ));
            used
        }
        None => {
            let (clipped, clipped_width) = clip(line, columns);
            let used = prefix_width + clipped_width;
            text = text.span(Span::styled(
                clipped,
                Style::new().fg(palette.ink).bg(palette.input_bg),
            ));
            used
        }
    };
    if used < width {
        text = text.span(Span::styled(
            " ".repeat(width - used),
            Style::new().bg(palette.input_bg),
        ));
    }
    text
}

/// Returns one line's characters on either side of the cursor.
///
/// The window is measured in display columns, so a wide character occupies the
/// columns revue renders it in. The cursor carries one column of its own; when
/// the text before it does not fit, the window starts so that the cursor is the
/// last column of the window.
fn cursor_window(line: &str, cursor: usize, columns: usize) -> (String, String) {
    let characters: Vec<char> = line.chars().collect();
    let cursor = cursor.min(characters.len());
    let width = |character: char| usize::from(char_width(character));
    let cursor_column: usize = characters[..cursor].iter().copied().map(width).sum();
    let line_column: usize = characters.iter().copied().map(width).sum();
    let budget = columns.saturating_sub(1);
    let start = if line_column <= budget {
        0
    } else {
        cursor_column.saturating_sub(budget.saturating_sub(1))
    };

    let mut before = String::with_capacity(columns);
    let mut after = String::with_capacity(columns);
    let mut column = 0;
    let mut at_cursor = false;
    for (index, character) in characters.iter().enumerate() {
        if index == cursor {
            at_cursor = true;
        }
        let character_width = width(*character);
        if column < start {
            column += character_width;
            continue;
        }
        if column + character_width > start + budget {
            break;
        }
        if at_cursor {
            after.push(*character);
        } else {
            before.push(*character);
        }
        column += character_width;
    }
    (before, after)
}

/// Returns one line's text clipped to `columns` display columns, and its width.
fn clip(line: &str, columns: usize) -> (String, usize) {
    let mut text = String::with_capacity(columns);
    let mut used = 0;
    for character in line.chars() {
        let character_width = usize::from(char_width(character));
        if used + character_width > columns {
            break;
        }
        text.push(character);
        used += character_width;
    }
    (text, used)
}

/// Returns the first buffer line a window of `visible` rows shows.
///
/// The window ends at the cursor's line, exactly as the horizontal window ends
/// at the cursor's column, so the cursor always rides a rendered row. A window
/// taller than the lines above the cursor starts at the buffer's first line.
const fn window_start(cursor_line: usize, visible: usize) -> usize {
    cursor_line.saturating_sub(visible.saturating_sub(1))
}

/// The buffer split into its lines, with the cursor's place among them.
struct BufferLines<'a> {
    /// The buffer's lines, in order; the buffer always carries at least one.
    lines: Vec<&'a str>,
    /// The line the cursor rides.
    cursor_line: usize,
    /// The cursor's column among that line's characters.
    cursor_column: usize,
}

impl<'a> BufferLines<'a> {
    /// Splits `input` on its line breaks and locates `cursor` among them.
    fn of(input: &'a str, cursor: usize) -> Self {
        let cursor = cursor.min(input.chars().count());
        let mut lines = Vec::new();
        let mut cursor_line = 0;
        let mut cursor_column = 0;
        let mut consumed = 0;
        for (index, line) in input.split('\n').enumerate() {
            let length = line.chars().count();
            if cursor >= consumed && cursor <= consumed + length {
                cursor_line = index;
                cursor_column = cursor - consumed;
            }
            consumed += length + 1;
            lines.push(line);
        }
        Self {
            lines,
            cursor_line,
            cursor_column,
        }
    }
}

/// Returns the display width of one piece of text.
fn display_width(text: &str) -> usize {
    text.chars()
        .map(|character| usize::from(char_width(character)))
        .sum()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "The pane unit tests assert typed fixture values directly."
    )]

    use super::{BufferLines, MIN_BLOCK_ROWS, block_rows, clip, window_start};
    use crate::app::{Action, AppState};

    #[test]
    fn the_buffer_splits_on_its_line_breaks_and_locates_the_cursor() {
        let buffer = BufferLines::of("ab\ncd", 0);
        assert_eq!(buffer.lines, vec!["ab", "cd"]);
        assert_eq!((buffer.cursor_line, buffer.cursor_column), (0, 0));
        let buffer = BufferLines::of("ab\ncd", 2);
        assert_eq!(
            (buffer.cursor_line, buffer.cursor_column),
            (0, 2),
            "the cursor before a break rides the line it ends"
        );
        let buffer = BufferLines::of("ab\ncd", 3);
        assert_eq!(
            (buffer.cursor_line, buffer.cursor_column),
            (1, 0),
            "the cursor after a break rides the next line"
        );
        let buffer = BufferLines::of("ab\ncd", 5);
        assert_eq!((buffer.cursor_line, buffer.cursor_column), (1, 2));
        let buffer = BufferLines::of("ab\n", 3);
        assert_eq!(
            (buffer.cursor_line, buffer.cursor_column),
            (1, 0),
            "a trailing break opens one empty last line"
        );
        let buffer = BufferLines::of("", 0);
        assert_eq!((buffer.cursor_line, buffer.cursor_column), (0, 0));
    }

    #[test]
    fn the_line_window_ends_at_the_cursor_line() {
        assert_eq!(window_start(0, 3), 0);
        assert_eq!(
            window_start(2, 3),
            0,
            "a window taller than the lines above starts at the first"
        );
        assert_eq!(
            window_start(5, 3),
            3,
            "a taller buffer shows the window ending at the cursor"
        );
        assert_eq!(window_start(5, 0), 5, "a window without rows shows none");
    }

    #[test]
    fn a_line_longer_than_its_row_is_clipped_to_it() {
        assert_eq!(clip("hello", 3), ("hel".to_owned(), 3));
        assert_eq!(clip("hi", 8), ("hi".to_owned(), 2));
        assert_eq!(clip("", 4), (String::new(), 0));
    }

    #[test]
    fn a_buffer_line_grows_the_block_by_one_row() {
        let mut state = AppState::new(None);
        assert_eq!(block_rows(&state), MIN_BLOCK_ROWS);
        state.update(Action::InputChar('\n'));
        assert_eq!(block_rows(&state), MIN_BLOCK_ROWS + 1);
        state.update(Action::InputChar('x'));
        assert_eq!(
            block_rows(&state),
            MIN_BLOCK_ROWS + 1,
            "a character joins the line it is typed on"
        );
    }
}
