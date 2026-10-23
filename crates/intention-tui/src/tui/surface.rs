//! The transcript's scroll surface: a `ScrollView` holding the rows it paints.
//!
//! The core owns the transcript rows; this front end owns only which of them the
//! viewport shows. The surface is the join between the two: the pane lays out
//! the visible display rows, hands them here with the index of the first one,
//! and the surface paints them into a `ScrollView` buffer tall enough for the
//! whole transcript before the view's own render copies the viewport out.
//!
//! Keeping the rows in the scroll view's own buffer is what makes the scroll
//! surface more than a scrollbar: the widget's state carries the offset, the
//! viewport arithmetic, and the page size, so the front end never re-derives
//! them. The buffer is grown only in [`GROWTH_ROWS`] steps and only when the
//! transcript outgrows it, so a streamed turn does not reallocate per frame.
//!
//! The scrollbar stays the pane's: it is painted in the column the pane reserves
//! beside the text, from the same `total`/`start` pair the rows are painted
//! from, so the thumb cannot disagree with the viewport.

use ratatui::Frame;
use ratatui::layout::{Position, Rect, Size};
use ratatui::style::Style;
use ratatui::text::{Line, Text};
use ratatui::widgets::{
    Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget,
};
use tui_scrollview::{ScrollView, ScrollViewState, ScrollbarVisibility};

use crate::tui::palette::Palette;

/// The rows the scroll view's buffer grows by.
const GROWTH_ROWS: u16 = 256;

/// The transcript's own scrollbar: a hairline track and a half-block thumb.
///
/// The thumb is deliberately not the full block the input line paints its
/// cursor with, so the gutter never reads as a cursor, and the bar carries no
/// end arrows: the wheel is how the transcript moves.
const TRANSCRIPT_BAR: ratatui::symbols::scrollbar::Set<'static> =
    ratatui::symbols::scrollbar::Set {
        track: "│",
        thumb: "▐",
        begin: "",
        end: "",
    };

/// The transcript's scroll surface.
#[derive(Debug)]
pub(in crate::tui) struct TranscriptSurface {
    /// The scroll view holding the painted rows.
    view: ScrollView,
    /// Its offset, size, and page size.
    state: ScrollViewState,
    /// The content columns the buffer was allocated for.
    width: u16,
    /// The content rows the buffer was allocated for.
    rows: u16,
}

impl TranscriptSurface {
    /// Returns an empty surface; the first frame sizes it.
    pub(in crate::tui) fn new() -> Self {
        Self {
            view: ScrollView::new(Size::new(1, 1)),
            state: ScrollViewState::new(),
            width: 0,
            rows: 0,
        }
    }

    /// Paints `lines` as the window `start..start + lines.len()` of `total`
    /// display rows in `area`, then copies the viewport out of the scroll view.
    ///
    /// The caller owns the layout: it passes the rows the viewport shows and the
    /// index of the first one, so the surface never decides what is on screen.
    /// It only guarantees that the row it is handed lands in the buffer at that
    /// index, and that the viewport the state reports starts there.
    pub(in crate::tui) fn show(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        lines: &[Line<'static>],
        start: u16,
        total: u16,
    ) {
        if area.is_empty() {
            return;
        }
        self.reserve(area.width, total);
        for (offset, line) in lines.iter().enumerate() {
            let Some(row) = start.checked_add(u16::try_from(offset).unwrap_or(u16::MAX)) else {
                break;
            };
            if row >= self.rows {
                break;
            }
            let target = Rect::new(0, row, area.width, 1);
            // The buffer keeps the previous frame's symbols, so a row that got
            // shorter would otherwise keep its stale tail.
            for x in target.x..target.right() {
                if let Some(cell) = self.view.buf_mut().cell_mut(Position::new(x, row)) {
                    cell.reset();
                }
            }
            Paragraph::new(Text::from(line.clone())).render(target, self.view.buf_mut());
        }
        // A transcript that shrank leaves rows the previous frame painted below
        // the window, so the rest of the viewport is cleared before it is
        // copied out.
        let painted = start.saturating_add(u16::try_from(lines.len()).unwrap_or(u16::MAX));
        let end = start.saturating_add(area.height).min(self.rows);
        for row in painted..end {
            for x in 0..area.width {
                if let Some(cell) = self.view.buf_mut().cell_mut(Position::new(x, row)) {
                    cell.reset();
                }
            }
        }
        self.state.set_offset(Position::new(0, start));
        (&self.view).render(area, frame.buffer_mut(), &mut self.state);
    }

    /// Grows the scroll view's buffer to hold `total` rows at `width` columns.
    ///
    /// The buffer grows in [`GROWTH_ROWS`] steps, so a streamed turn appends into
    /// the slack it already has and a long transcript reallocates only when it
    /// crosses a step boundary.
    fn reserve(&mut self, width: u16, total: u16) {
        let rows = total.max(1).div_ceil(GROWTH_ROWS) * GROWTH_ROWS;
        if width == self.width && rows <= self.rows {
            return;
        }
        self.width = width;
        self.rows = self.rows.max(rows);
        // The pane paints its own scrollbar beside the text, so the view keeps
        // neither of its own: a bar here would steal a column and a row from
        // the rows the pane laid out.
        self.view = ScrollView::new(Size::new(width, self.rows))
            .scrollbars_visibility(ScrollbarVisibility::Never);
    }
}

/// Paints the transcript's scrollbar in `area`.
///
/// The thumb is one cell tall at minimum and moves with `start` through `total`
/// rows, so the column reports the same window the rows do.
pub(in crate::tui) fn scrollbar(
    frame: &mut Frame,
    area: Rect,
    total: u16,
    start: u16,
    palette: &'static Palette,
) {
    if area.is_empty() {
        return;
    }
    let mut state = ScrollbarState::new(usize::from(total))
        .position(usize::from(start))
        .viewport_content_length(usize::from(area.height));
    let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .symbols(TRANSCRIPT_BAR)
        .style(Style::new().bg(palette.panel))
        .thumb_style(Style::new().fg(palette.scroll_thumb))
        .track_style(Style::new().fg(palette.scroll_track));
    frame.render_stateful_widget(bar, area, &mut state);
}

#[cfg(test)]
mod tests {
    use super::{GROWTH_ROWS, TranscriptSurface};
    use crate::tui::testing::Screen;
    use ratatui::layout::Rect;
    use ratatui::text::Line;

    /// Returns `count` numbered display rows.
    fn rows(count: u16) -> Vec<Line<'static>> {
        (0..count)
            .map(|index| Line::from(format!("row {index}")))
            .collect()
    }

    #[test]
    fn a_transcript_that_shrinks_leaves_no_stale_rows_below_its_last_row() {
        let mut surface = TranscriptSurface::new();
        let area = Rect::new(0, 0, 12, 6);
        let tall = rows(6);
        let screen = Screen::widget(12, 6, |frame| {
            surface.show(frame, area, &tall, 0, 6);
        });
        assert_eq!(screen.line(5), "row 5");
        let short = rows(2);
        let screen = Screen::widget(12, 6, |frame| {
            surface.show(frame, area, &short, 0, 2);
        });
        assert_eq!(screen.line(1), "row 1");
        for row in 2..6 {
            assert_eq!(
                screen.line(row),
                "",
                "row {row} keeps nothing from the taller transcript"
            );
        }
    }

    #[test]
    fn the_buffer_grows_in_steps_and_keeps_its_allocation_while_it_fits() {
        let mut surface = TranscriptSurface::new();
        let area = Rect::new(0, 0, 12, 6);
        let first = rows(1);
        let _ = Screen::widget(12, 6, |frame| {
            surface.show(frame, area, &first, 0, 1);
        });
        assert_eq!(surface.rows, GROWTH_ROWS);
        let within = rows(6);
        let _ = Screen::widget(12, 6, |frame| {
            surface.show(frame, area, &within, 0, GROWTH_ROWS);
        });
        assert_eq!(
            surface.rows, GROWTH_ROWS,
            "a transcript inside the slack does not reallocate"
        );
        let _ = Screen::widget(12, 6, |frame| {
            surface.show(frame, area, &within, 0, GROWTH_ROWS + 1);
        });
        assert_eq!(
            surface.rows,
            GROWTH_ROWS * 2,
            "one row past the slack grows the buffer by a step"
        );
    }
}
