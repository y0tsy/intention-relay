//! The chat screen: the transcript - or the welcome pane while no session is
//! open - the input block, the command hint menu that sits directly above it,
//! and the notice or error line, inside the framed panel the whole window is
//! built from.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, BorderType, Paragraph};

use crate::app::{AppState, Screen};
use crate::tui::layout::TranscriptLayoutCache;
use crate::tui::palette::{self, Palette};
use crate::tui::panes::{commands, input, status, theme, transcript, welcome};
use crate::tui::surface::TranscriptSurface;

/// The columns and rows the window keeps around the chat panel.
const PANEL_MARGIN: u16 = 1;

/// The rows and columns the panel's frame takes.
const PANEL_FRAME: u16 = 2;

/// The columns the panel pads its content with.
const PANEL_PADDING: u16 = 1;

/// The rows the notice or error line occupies under the input block.
const DETAIL_ROWS: u16 = 1;

/// The share of the panel's body the input block may show buffer lines for.
///
/// The block grows with its buffer up to this share of the rows it competes for
/// and then shows the window of lines that carries the cursor, so a prompt of any
/// length leaves the transcript the rows it does not need: growth is worth its
/// rows, but never the whole window. The share is a divisor, so the cap follows
/// the window: a taller terminal lets the block grow further, a shorter one caps
/// it sooner.
const INPUT_LINE_SHARE: u16 = 3;

/// The buffer lines the input block shows even in a body too short for its share.
///
/// A prompt is read as text before it is sent, so the block never shows fewer
/// lines than this: the cap yields to the floor, and the transcript lives with
/// what is left.
const INPUT_MIN_LINES: u16 = 3;

/// The geometry of the chat panel in one window.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Panel {
    /// The columns the panel occupies.
    columns: u16,
    /// The rows the panel occupies.
    rows: u16,
    /// The rows the transcript pane occupies inside the panel.
    transcript_rows: u16,
    /// The rows the region above the input block occupies: the command hint
    /// band, or the theme picker's panel while its screen owns the window.
    hint_rows: u16,
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
/// its one row under it, the hint region takes the rows it needs directly above
/// the input block while it is open, and the transcript takes every row that
/// leaves; the four inside the frame sum to the panel's body exactly, so no
/// child overflows the panel. The block's growth is bounded by
/// [`INPUT_ROW_SHARE`] of that body: a buffer taller than its share keeps those
/// rows and shows the window of lines that carries the cursor, so the block
/// never spends the whole panel on a long prompt and the transcript keeps the
/// rest.
///
/// The hint region takes its rows exactly or not at all: a body that cannot
/// hold the whole band - or the picker panel - beside the rows the input block
/// would have had without it shows none of it, because the region yields to the
/// input the user is typing in.
const fn panel(window: Rect, wanted_input_rows: u16, wanted_hint_rows: u16) -> Panel {
    let columns = window.width.saturating_sub(PANEL_MARGIN * 2);
    let rows = window.height.saturating_sub(PANEL_MARGIN * 2);
    let body = rows.saturating_sub(PANEL_FRAME);
    let detail_rows = if body > 0 { DETAIL_ROWS } else { 0 };
    let available = body.saturating_sub(detail_rows);
    // The cap bounds the buffer lines the block shows, never its own chrome: the
    // badge, the status row, and the frame survive in any window, and a block
    // taller than its share windows the lines instead of growing further.
    let share = available / INPUT_LINE_SHARE;
    let lines = if share > INPUT_MIN_LINES {
        share
    } else {
        INPUT_MIN_LINES
    };
    let wanted_cap = lines.saturating_add(input::CHROME_ROWS);
    let cap = if wanted_cap < available {
        wanted_cap
    } else {
        available
    };
    let input_rows = if wanted_input_rows < cap {
        wanted_input_rows
    } else {
        cap
    };
    let hint_rows =
        if wanted_hint_rows > 0 && wanted_hint_rows.saturating_add(input_rows) <= available {
            wanted_hint_rows
        } else {
            0
        };
    let transcript_rows = available
        .saturating_sub(hint_rows)
        .saturating_sub(input_rows);
    Panel {
        columns,
        rows,
        transcript_rows,
        hint_rows,
        input_rows,
        detail_rows,
        content_columns: columns.saturating_sub(PANEL_FRAME + PANEL_PADDING * 2),
    }
}

/// Returns the rows the region above the input block requests.
///
/// The theme picker's screen claims its own panel there, and every other screen
/// claims the command hint band: the picker cannot open beside a band, because
/// its screen takes the keyboard while the input line is empty.
fn hint_rows(state: &AppState) -> u16 {
    match state.screen() {
        Screen::Chat | Screen::Sessions => commands::menu_rows(state),
        Screen::Theme => theme::panel_rows(),
    }
}

/// Returns the region above the input block: the theme picker's panel while its
/// screen owns the window, the command hint band otherwise.
fn hint_block(state: &AppState, area: Rect, frame: &mut Frame, palette: &'static Palette) {
    match state.screen() {
        Screen::Chat | Screen::Sessions => commands::menu_block(state, area, frame, palette),
        Screen::Theme => theme::picker_panel(state, area, frame, palette),
    }
}

/// Paints the chat screen: the framed panel that holds the transcript, the
/// input block, and the notice or error line, placed on the canvas the window
/// fills.
///
/// The theme is resolved here, once per frame, from the effective theme - the
/// picker's preview while one is active, the committed theme otherwise - and
/// handed to every pane down the tree: the panel, the transcript (or the
/// welcome), the command band or the picker panel, the input block, and the
/// detail line all paint the same palette.
pub(super) fn chat_screen(
    state: &AppState,
    window: Rect,
    cache: &mut TranscriptLayoutCache,
    surface: &mut TranscriptSurface,
    frame: &mut Frame,
) {
    let palette = palette::of(state.effective_theme());
    let panel = panel(window, input::block_rows(state), hint_rows(state));
    let outer = Rect::new(
        window.x.saturating_add(PANEL_MARGIN),
        window.y.saturating_add(PANEL_MARGIN),
        panel.columns,
        panel.rows,
    );
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(palette.border))
        .style(Style::new().bg(palette.panel));
    let inner = block.inner(outer);
    frame.render_widget(block, outer);
    if inner.is_empty() {
        return;
    }
    let content = Rect::new(
        inner.x.saturating_add(PANEL_PADDING),
        inner.y,
        inner.width.saturating_sub(PANEL_PADDING * 2),
        inner.height,
    );
    let transcript_area = Rect::new(content.x, content.y, content.width, panel.transcript_rows);
    if welcome::is_welcome(state) {
        welcome::welcome_pane(transcript_area, frame, palette);
    } else {
        transcript::transcript_pane(state, transcript_area, cache, surface, frame, palette);
    }
    let hint_area = Rect::new(
        content.x,
        content.y.saturating_add(panel.transcript_rows),
        content.width,
        panel.hint_rows,
    );
    hint_block(state, hint_area, frame, palette);
    let input_area = Rect::new(
        content.x,
        hint_area.y.saturating_add(panel.hint_rows),
        content.width,
        panel.input_rows,
    );
    input::input_block(state, input_area, frame, palette);
    let detail_area = Rect::new(
        content.x,
        input_area.y.saturating_add(panel.input_rows),
        content.width,
        panel.detail_rows,
    );
    if !detail_area.is_empty() {
        frame.render_widget(
            Paragraph::new(status::status_detail(state, palette)),
            detail_area,
        );
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Rect;

    use super::{DETAIL_ROWS, PANEL_FRAME, panel};
    use crate::tui::panes::theme;

    #[test]
    fn the_panel_spends_its_body_on_the_transcript_the_menu_the_input_block_and_the_detail_line() {
        let window = Rect::new(0, 0, 80, 24);
        let one = panel(window, 5, 0);
        assert_eq!(one.detail_rows, DETAIL_ROWS);
        assert_eq!(one.hint_rows, 0, "a closed band takes no row");
        assert_eq!(one.transcript_rows, 14, "a one-line buffer keeps its rows");
        assert_eq!(
            one.transcript_rows + one.hint_rows + one.input_rows + one.detail_rows,
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
            two.transcript_rows + two.hint_rows + two.input_rows + two.detail_rows,
            20
        );

        let tall = panel(window, 100, 0);
        assert_eq!(
            tall.input_rows, 10,
            "the block stops at its share of the lines - a third of nineteen - plus its own chrome"
        );
        assert_eq!(
            tall.transcript_rows, 9,
            "the rows the block does not take stay with the transcript"
        );
        assert_eq!(
            tall.transcript_rows + tall.hint_rows + tall.input_rows + tall.detail_rows,
            20
        );

        // The open menu is the transcript's loss, never the input's: the block
        // keeps every row its buffer needs, and the band above it takes the
        // rows the transcript gives up.
        let menu = panel(window, 6, 8);
        assert_eq!(menu.hint_rows, 8, "the band takes exactly the rows it asks");
        assert_eq!(menu.input_rows, two.input_rows, "the input keeps its rows");
        assert_eq!(
            menu.transcript_rows + 8,
            two.transcript_rows,
            "the transcript gives up exactly the menu's rows"
        );
        assert_eq!(
            menu.transcript_rows + menu.hint_rows + menu.input_rows + menu.detail_rows,
            20
        );

        // The theme picker's panel takes the same region, fixed at its own
        // height: the input block and the detail line keep their rows exactly
        // as they do without it.
        let picker = panel(window, 5, theme::panel_rows());
        assert_eq!(
            picker.hint_rows,
            theme::panel_rows(),
            "the picker takes exactly its own rows"
        );
        assert_eq!(
            picker.transcript_rows + theme::panel_rows(),
            one.transcript_rows
        );
        assert_eq!(
            picker.transcript_rows + picker.hint_rows + picker.input_rows + picker.detail_rows,
            20
        );

        // A body too short for the band and the rows the input block needs
        // keeps the input: the band yields rather than taking the input's rows,
        // and the block's floor keeps the lines every prompt is read in.
        let cramped = panel(Rect::new(0, 0, 80, 12), 6, 4);
        assert_eq!(cramped.hint_rows, 0, "the band yields to the input block");
        assert_eq!(
            cramped.input_rows, 6,
            "the input keeps every row it asked for"
        );
        assert_eq!(
            cramped.transcript_rows + cramped.hint_rows + cramped.input_rows + cramped.detail_rows,
            8
        );

        // The same rule applies to the taller picker panel: a window too short
        // for it plus the input shows no panel instead of stealing input rows.
        let cramped = panel(Rect::new(0, 0, 80, 12), 5, theme::panel_rows());
        assert_eq!(cramped.hint_rows, 0, "the picker yields to the input block");
        assert_eq!(cramped.input_rows, 5);
    }

    #[test]
    fn a_window_with_no_body_spends_no_rows() {
        let tiny = panel(Rect::new(0, 0, 80, PANEL_FRAME), 5, 8);
        assert_eq!(
            tiny.transcript_rows + tiny.hint_rows + tiny.input_rows + tiny.detail_rows,
            0,
            "a body too small for the menu and the detail line keeps neither"
        );
    }
}
