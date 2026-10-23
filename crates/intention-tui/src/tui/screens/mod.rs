//! One file per surface of the terminal view.
//!
//! [`TerminalView`] is the pure function of one [`AppState`] the terminal
//! renders. The window has exactly one base - the chat panel on the canvas -
//! and while the core's screen carries the sessions browser, that browser is a
//! layout row docked to the bottom of the same window: the chat container
//! shrinks to the rows the browser leaves, its own bottom canvas margin row is
//! the blank gap between the two, and the browser's bottom edge is the window's
//! last row. No surface ever paints over another. The theme picker is not a
//! second docked surface: its screen keeps the one chat panel and spends the
//! transcript's rows on a panel directly above the input block, so the input
//! block and the detail line never move. Each module here owns the builders
//! only its surface reads, so a new surface is a module beside them and one arm
//! in [`TerminalView::render`].

pub(super) mod chat;
pub(super) mod sessions;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;

use crate::app::{AppState, Screen};
use crate::tui::layout::TranscriptLayoutCache;
use crate::tui::palette;
use crate::tui::surface::TranscriptSurface;

/// The terminal view: a pure function of one [`AppState`].
///
/// Every screen derives its content from the state on each frame and mutates
/// nothing of it, so the same state paints identically in a render test and in
/// a terminal. The two things a frame carries over are the front end's own: the
/// transcript's layout cache and the scroll surface the transcript is painted
/// into.
pub struct TerminalView<'a> {
    state: &'a AppState,
    now: i64,
    cache: &'a mut TranscriptLayoutCache,
    surface: &'a mut TranscriptSurface,
}

impl<'a> TerminalView<'a> {
    /// Creates the view of one application state, reading the system clock.
    #[must_use]
    pub(in crate::tui) fn new(
        state: &'a AppState,
        cache: &'a mut TranscriptLayoutCache,
        surface: &'a mut TranscriptSurface,
    ) -> Self {
        Self::with_now(state, system_now(), cache, surface)
    }

    /// Creates the view of one application state at a fixed wall-clock second.
    ///
    /// `now` is the whole Unix second the view measures relative times from, so
    /// a render test fixes the clock instead of reading it.
    #[must_use]
    pub(in crate::tui) const fn with_now(
        state: &'a AppState,
        now: i64,
        cache: &'a mut TranscriptLayoutCache,
        surface: &'a mut TranscriptSurface,
    ) -> Self {
        Self {
            state,
            now,
            cache,
            surface,
        }
    }

    /// Paints the one window: the canvas, the chat panel on it, and - while the
    /// core's screen carries the browser - the browser docked as the window's
    /// bottom rows, with the chat container above it.
    ///
    /// The window never changes: docking or closing the browser, and selecting
    /// the session it points at, all repaint this same frame, so the chat keeps
    /// its transcript, its input, and its session while it reflows into the
    /// rows the browser leaves.
    pub fn render(self, frame: &mut Frame) {
        let window = frame.area();
        frame.buffer_mut().set_style(
            window,
            Style::new().bg(palette::of(self.state.effective_theme()).canvas),
        );
        let panel = (self.state.screen() == Screen::Sessions)
            .then(|| sessions::geometry(self.state, window))
            .flatten();
        let Some(geometry) = panel else {
            chat::chat_screen(self.state, window, self.cache, self.surface, frame);
            return;
        };
        // The chat container keeps every row the panel does not use, and its
        // own bottom canvas margin row is the one gap row above the panel.
        let [chat_area, panel_area] =
            Layout::vertical([Constraint::Min(0), Constraint::Length(geometry.rows)]).areas(window);
        chat::chat_screen(self.state, chat_area, self.cache, self.surface, frame);
        sessions::panel(self.state, self.now, geometry, panel_area, frame);
    }
}

/// Returns the current wall clock in whole Unix seconds, or the epoch before it.
fn system_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
        })
}
