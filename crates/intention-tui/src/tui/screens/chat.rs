//! The chat screen: the transcript, the input block, and the notice or error
//! line, inside the framed panel the whole window is built from.

use std::cell::RefCell;

use revue::layout::Rect;
use revue::widget::{Border, Card, Stack, Text, hstack, vstack};

use crate::app::AppState;
use crate::tui::layout::TranscriptLayoutCache;
use crate::tui::palette;
use crate::tui::panes::{input, status, transcript};

/// The columns and rows the window keeps around the chat panel.
const PANEL_MARGIN: u16 = 1;

/// The rows and columns the panel's frame takes.
const PANEL_FRAME: u16 = 2;

/// The columns the panel pads its content with.
const PANEL_PADDING: u16 = 1;

/// The rows the input block occupies: its frame, the badge header, the input
/// line, and the run status row under it.
const INPUT_BLOCK_ROWS: u16 = input::BLOCK_ROWS;

/// The rows the notice or error line occupies under the input block.
const DETAIL_ROWS: u16 = 1;

/// The rows above the transcript pane's first content row: the window's own
/// margin, the panel's top frame row, the card's padding, and the transcript
/// pane's own top frame row.
///
/// The transcript publishes this row with the rest of its window, so a mouse
/// event hit-tests exactly the display rows the frame painted.
const TRANSCRIPT_TOP_ROWS: u16 = PANEL_MARGIN + 1 + PANEL_PADDING + 1;

/// The geometry of the chat panel in one window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Panel {
    /// The columns the panel occupies.
    columns: u16,
    /// The rows the panel occupies.
    rows: u16,
    /// The rows the transcript pane occupies inside the panel.
    transcript_rows: u16,
    /// The columns the panel's content has.
    content_columns: u16,
}

/// Returns the geometry of the chat panel in one window.
///
/// The panel sits on the canvas with a one-cell margin, so the window's own
/// background stays visible around it. The input block and the detail line sit
/// at the bottom, and the transcript takes every row they leave.
const fn panel(window: Rect) -> Panel {
    let columns = window.width.saturating_sub(PANEL_MARGIN * 2);
    let rows = window.height.saturating_sub(PANEL_MARGIN * 2);
    Panel {
        columns,
        rows,
        transcript_rows: rows.saturating_sub(PANEL_FRAME + INPUT_BLOCK_ROWS + DETAIL_ROWS),
        content_columns: columns.saturating_sub(PANEL_FRAME + PANEL_PADDING * 2),
    }
}

/// Returns the screen row the transcript pane's first content row sits on.
const fn transcript_top(window: Rect) -> u16 {
    window.y.saturating_add(TRANSCRIPT_TOP_ROWS)
}

/// Returns the chat screen: the framed panel that holds the transcript, the
/// input block, and the notice or error line, placed on the canvas the window
/// fills.
pub(super) fn chat_screen(
    state: &AppState,
    window: Rect,
    cache: &RefCell<TranscriptLayoutCache>,
) -> Stack {
    let panel = panel(window);
    let bottom = window
        .height
        .saturating_sub(PANEL_MARGIN + panel.rows + PANEL_MARGIN);
    vstack()
        .child_sized(Text::new(""), PANEL_MARGIN)
        .child_sized(
            hstack()
                .child_sized(Text::new(""), PANEL_MARGIN)
                .child_sized(
                    panel_widget(state, panel, transcript_top(window), cache),
                    panel.columns,
                ),
            panel.rows,
        )
        .child_sized(Text::new(""), bottom)
}

/// Returns the framed panel: one rounded frame whose card carries the chat.
///
/// The card draws no border of its own, so the frame reads as a single
/// container with the panel's padding inside it: the transcript on top, the
/// input block under it, and the notice or error line last.
///
/// `top` is the screen row the transcript pane's content starts at, which the
/// pane publishes with the rest of its window for the next mouse event.
fn panel_widget(
    state: &AppState,
    panel: Panel,
    top: u16,
    cache: &RefCell<TranscriptLayoutCache>,
) -> Border {
    Border::rounded()
        .min_size(panel.columns, panel.rows)
        .max_size(panel.columns, panel.rows)
        .fg(palette::BORDER)
        .bg(palette::PANEL)
        .child(
            Card::new()
                .flat()
                .padding(PANEL_PADDING)
                .background(palette::PANEL)
                .body(
                    vstack()
                        .child_sized(
                            transcript::transcript_pane(
                                state,
                                top,
                                panel.transcript_rows,
                                panel.content_columns,
                                cache,
                            ),
                            panel.transcript_rows,
                        )
                        .child_sized(
                            input::input_block(state, panel.content_columns),
                            INPUT_BLOCK_ROWS,
                        )
                        .child_sized(status::status_detail(state), DETAIL_ROWS),
                ),
        )
}
