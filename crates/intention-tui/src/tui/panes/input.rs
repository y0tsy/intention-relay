//! The input block: the badge header, the input line, and the run status row.

use revue::text::char_width;
use revue::widget::{Border, Card, RichText, Span, Style, vstack};

use crate::app::AppState;
use crate::tui::palette;
use crate::tui::panes::status;

/// The prompt the input line carries before its text.
const PROMPT: &str = "> ";

/// The marker rendered at the input cursor position.
///
/// `RichText` styles one run at a time, so the cursor is a block glyph rather
/// than a reversed cell: `█` occupies the character cell the cursor points at,
/// and the cell after the last character at the line's end.
const CURSOR: char = '█';

/// The columns and rows the block's own frame takes.
const BLOCK_FRAME: u16 = 2;

/// The columns the block pads its content with.
const BLOCK_PADDING: u16 = 1;

/// The rows the block's badge header occupies.
const BADGE_ROWS: u16 = 1;

/// The rows the input line occupies.
const INPUT_ROWS: u16 = 1;

/// The rows the run status under the input occupies.
const STATUS_ROWS: u16 = 1;

/// The rows the whole block occupies in the chat panel.
pub(in crate::tui) const BLOCK_ROWS: u16 = BLOCK_FRAME + BADGE_ROWS + INPUT_ROWS + STATUS_ROWS;

/// The mode badge before a session snapshot has named the mode.
///
/// The mode is durable session state and arrives with the snapshot, so the
/// badge waits for it rather than guessing `build`.
const MODE_PLACEHOLDER: &str = "mode @todo(core)";

/// The model badge: the session projection carries no provider model.
// @todo(core): the session projection carries no provider model, so this badge
// stays an explicit placeholder rather than a fabricated model name.
const MODEL_PLACEHOLDER: &str = "model @todo(core)";

/// Returns the input block: one focused frame around the badge header, the
/// input line, and the run status row under it.
pub(in crate::tui) fn input_block(state: &AppState, width: u16) -> Border {
    let content = usize::from(width.saturating_sub(BLOCK_FRAME + BLOCK_PADDING * 2));
    Border::rounded()
        .min_size(width, BLOCK_ROWS)
        .max_size(width, BLOCK_ROWS)
        .fg(palette::ACCENT)
        .bg(palette::PANEL)
        .child(
            Card::new()
                .flat()
                .padding(BLOCK_PADDING)
                .background(palette::PANEL)
                .body(
                    vstack()
                        .child_sized(badge_row(state, content), BADGE_ROWS)
                        .child_sized(input_line(state, content), INPUT_ROWS)
                        .child_sized(status::status_row(state), STATUS_ROWS),
                ),
        )
}

/// Returns the badge header: the session's real run mode and the model
/// placeholder, each on its own badge wash.
fn badge_row(state: &AppState, width: usize) -> RichText {
    let (mode, mode_ink) = state
        .session_mode()
        .map_or((MODE_PLACEHOLDER, palette::TODO_INK), |mode| {
            (mode.as_str(), palette::BADGE_INK)
        });
    let mut text = RichText::new();
    let mut used = 0;
    for (label, ink) in [(mode, mode_ink), (MODEL_PLACEHOLDER, palette::TODO_INK)] {
        let chip = format!(" {label} ");
        let chip_width = display_width(&chip);
        let gap = usize::from(used > 0);
        if used + gap + chip_width > width {
            break;
        }
        if gap > 0 {
            text = text.span(Span::styled(
                " ",
                Style::new().fg(palette::INK).bg(palette::PANEL),
            ));
            used += 1;
        }
        text = text.span(Span::styled(
            chip,
            Style::new().fg(ink).bg(palette::BADGE_BG),
        ));
        used += chip_width;
    }
    if used < width {
        text = text.span(Span::styled(
            " ".repeat(width - used),
            Style::new().bg(palette::PANEL),
        ));
    }
    text
}

/// Returns the input line the user is typing into, inside the block.
///
/// The prompt and the cursor carry the accent, the text carries the body ink,
/// and the whole line carries the input fill - so the focused line is marked by
/// colour, by the prompt glyph, and by the block cursor, and stays recognizable
/// if a terminal maps the palette to plain ANSI.
///
/// The line is windowed horizontally when it is longer than the block: the
/// window ends at the cursor, so the cursor stays visible while the text before
/// it scrolls off.
fn input_line(state: &AppState, width: usize) -> RichText {
    let columns = width.saturating_sub(PROMPT.len());
    let (before, after) = input_window(state, columns);
    let used = PROMPT.len() + display_width(&before) + 1 + display_width(&after);
    let mut text = RichText::new()
        .span(Span::styled(
            PROMPT,
            Style::new()
                .fg(palette::ACCENT)
                .bg(palette::INPUT_BG)
                .bold(),
        ))
        .span(Span::styled(
            before,
            Style::new().fg(palette::INK).bg(palette::INPUT_BG),
        ))
        .span(Span::styled(
            CURSOR.to_string(),
            Style::new()
                .fg(palette::ACCENT)
                .bg(palette::INPUT_BG)
                .bold(),
        ))
        .span(Span::styled(
            after,
            Style::new().fg(palette::INK).bg(palette::INPUT_BG),
        ));
    if used < width {
        text = text.span(Span::styled(
            " ".repeat(width - used),
            Style::new().bg(palette::INPUT_BG),
        ));
    }
    text
}

/// Returns the input characters on either side of the cursor.
///
/// The window is measured in display columns, so a wide character occupies the
/// columns revue renders it in. The cursor carries one column of its own; when
/// the text before it does not fit, the window starts so that the cursor is the
/// last column of the window.
fn input_window(state: &AppState, columns: usize) -> (String, String) {
    let characters: Vec<char> = state.input().chars().collect();
    let cursor = state.cursor().min(characters.len());
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

/// Returns the display width of one piece of text.
fn display_width(text: &str) -> usize {
    text.chars()
        .map(|character| usize::from(char_width(character)))
        .sum()
}
