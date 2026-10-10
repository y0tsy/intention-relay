//! The chat screen: the transcript - or the welcome pane while no session is
//! open - the input block, the command hint menu that sits directly above it,
//! and the notice or error line, inside the framed panel the whole window is
//! built from.

use std::cell::RefCell;

use revue::layout::Rect;
use revue::widget::{Border, Card, Stack, Text, hstack, vstack};

use crate::app::AppState;
use crate::tui::layout::TranscriptLayoutCache;
use crate::tui::palette;
use crate::tui::panes::{commands, input, status, transcript, welcome};

/// The columns and rows the window keeps around the chat panel.
const PANEL_MARGIN: u16 = 1;

/// The rows and columns the panel's frame takes.
const PANEL_FRAME: u16 = 2;

/// The columns the panel pads its content with.
const PANEL_PADDING: u16 = 1;

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
    /// The rows the command hint menu occupies above the input block.
    menu_rows: u16,
    /// The rows the input block occupies inside the panel.
    input_rows: u16,
    /// The rows the notice or error line occupies.
    detail_rows: u16,
    /// The columns the panel's content has.
    content_columns: u16,
}

/// Returns the geometry of the chat panel in one window.
///
/// The panel sits on the canvas with a one-cell margin, so the window's own
/// background stays visible around it. The input block takes the rows its
/// buffer needs - one per line beside its fixed chrome - the notice line keeps
/// its one row under it, the command hint menu takes the rows it needs directly
/// above the input block while it is open, and the transcript takes every row
/// that leaves; the four inside the frame sum to the panel's body exactly, so
/// no child overflows the panel. A buffer taller than the panel can hold is
/// bounded by the rows left after the detail line and the menu, and the block
/// windows it.
///
/// The menu takes its rows exactly or not at all: a body that cannot hold the
/// whole band beside the rows the input block would have had without it shows
/// no band at all, because the band yields to the input the user is typing in.
const fn panel(window: Rect, wanted_input_rows: u16, wanted_menu_rows: u16) -> Panel {
    let columns = window.width.saturating_sub(PANEL_MARGIN * 2);
    let rows = window.height.saturating_sub(PANEL_MARGIN * 2);
    let body = rows.saturating_sub(PANEL_FRAME);
    let detail_rows = if body > 0 { DETAIL_ROWS } else { 0 };
    let available = body.saturating_sub(detail_rows);
    let input_rows = if wanted_input_rows < available {
        wanted_input_rows
    } else {
        available
    };
    let menu_rows =
        if wanted_menu_rows > 0 && wanted_menu_rows.saturating_add(input_rows) <= available {
            wanted_menu_rows
        } else {
            0
        };
    let transcript_rows = available
        .saturating_sub(menu_rows)
        .saturating_sub(input_rows);
    Panel {
        columns,
        rows,
        transcript_rows,
        menu_rows,
        input_rows,
        detail_rows,
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
    let panel = panel(window, input::block_rows(state), commands::menu_rows(state));
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
/// container with the panel's padding inside it: the transcript - or the
/// welcome pane while no session is open - on top, the command hint menu
/// directly under it and directly above the input block, the input block under
/// that, and the notice or error line last. The menu is a band of this panel,
/// never a surface painted over another: while it is open the transcript gives
/// up exactly its rows, so the panel's four children still sum to its body. The
/// welcome keeps the same region, so the input block and the detail line never
/// move between the two states.
///
/// `top` is the screen row the transcript pane's content starts at, which the
/// pane publishes with the rest of its window for the next mouse event.
fn panel_widget(
    state: &AppState,
    panel: Panel,
    top: u16,
    cache: &RefCell<TranscriptLayoutCache>,
) -> Border {
    let mut content = vstack();
    content = if welcome::is_welcome(state) {
        content.child_sized(
            welcome::welcome_pane(panel.transcript_rows, panel.content_columns),
            panel.transcript_rows,
        )
    } else {
        content.child_sized(
            transcript::transcript_pane(
                state,
                top,
                panel.transcript_rows,
                panel.content_columns,
                cache,
            ),
            panel.transcript_rows,
        )
    };
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
                    content
                        .child_sized(
                            commands::menu_block(state, panel.content_columns, panel.menu_rows),
                            panel.menu_rows,
                        )
                        .child_sized(
                            input::input_block(state, panel.content_columns, panel.input_rows),
                            panel.input_rows,
                        )
                        .child_sized(status::status_detail(state), panel.detail_rows),
                ),
        )
}

#[cfg(test)]
mod tests {
    use revue::layout::Rect;

    use super::{DETAIL_ROWS, PANEL_FRAME, panel};

    #[test]
    fn the_panel_spends_its_body_on_the_transcript_the_menu_the_input_block_and_the_detail_line() {
        let window = Rect::new(0, 0, 80, 24);
        let one = panel(window, 5, 0);
        assert_eq!(one.detail_rows, DETAIL_ROWS);
        assert_eq!(one.menu_rows, 0, "a closed menu takes no row");
        assert_eq!(one.transcript_rows, 14, "a one-line buffer keeps its rows");
        assert_eq!(
            one.transcript_rows + one.menu_rows + one.input_rows + one.detail_rows,
            20,
            "the body is spent exactly"
        );

        let two = panel(window, 6, 0);
        assert_eq!(two.input_rows, 6, "the block takes one row per buffer line");
        assert_eq!(
            two.transcript_rows + 1,
            one.transcript_rows,
            "the transcript gives up exactly the row the line added"
        );
        assert_eq!(
            two.transcript_rows + two.menu_rows + two.input_rows + two.detail_rows,
            20
        );

        let tall = panel(window, 100, 0);
        assert_eq!(tall.input_rows, 19, "the detail line's row is kept first");
        assert_eq!(tall.transcript_rows, 0);
        assert_eq!(
            tall.transcript_rows + tall.menu_rows + tall.input_rows + tall.detail_rows,
            20
        );

        // The open menu is the transcript's loss, never the input's: the block
        // keeps every row its buffer needs, and the band above it takes the
        // rows the transcript gives up.
        let menu = panel(window, 6, 8);
        assert_eq!(menu.menu_rows, 8, "the band takes exactly the rows it asks");
        assert_eq!(menu.input_rows, two.input_rows, "the input keeps its rows");
        assert_eq!(
            menu.transcript_rows + 8,
            two.transcript_rows,
            "the transcript gives up exactly the menu's rows"
        );
        assert_eq!(
            menu.transcript_rows + menu.menu_rows + menu.input_rows + menu.detail_rows,
            20
        );

        // A body too short for the band and the rows the input block needs
        // keeps the input: the band yields rather than taking the input's rows.
        let cramped = panel(Rect::new(0, 0, 80, 12), 6, 4);
        assert_eq!(cramped.menu_rows, 0, "the band yields to the input block");
        assert_eq!(
            cramped.input_rows, 6,
            "the input keeps every row it asked for"
        );
        assert_eq!(
            cramped.transcript_rows + cramped.menu_rows + cramped.input_rows + cramped.detail_rows,
            8
        );
    }

    #[test]
    fn a_window_with_no_body_spends_no_rows() {
        let tiny = panel(Rect::new(0, 0, 80, PANEL_FRAME), 5, 8);
        assert_eq!(
            tiny.transcript_rows + tiny.menu_rows + tiny.input_rows + tiny.detail_rows,
            0,
            "a body too small for the menu and the detail line keeps neither"
        );
    }
}
