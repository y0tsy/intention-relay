//! The render test harness: one terminal screen a test draws a view into.
//!
//! A [`Screen`] owns a [`TestBackend`] of the size the test asks for, so a test
//! renders the same frame the terminal would and reads the cells back:
//! [`Screen::screen_text`] for the whole screen, [`Screen::line`] for one row,
//! [`Screen::find_text`] for a substring's position, and [`Screen::cell`] for
//! one cell's symbol, ink, and modifiers. Nothing in this module is part of the
//! front end: it exists only so the tests can look at what a frame painted.

use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Position;

use crate::app::AppState;
use crate::tui::layout::TranscriptLayoutCache;
#[cfg(test)]
use crate::tui::layout::TranscriptWindow;
use crate::tui::screens::TerminalView;
use crate::tui::surface::TranscriptSurface;

/// One terminal screen of the size a test asked for.
pub struct Screen {
    /// The terminal the frames are drawn on.
    terminal: Terminal<TestBackend>,
    /// The transcript layout cache the view renders through.
    cache: TranscriptLayoutCache,
    /// The transcript's scroll surface.
    surface: TranscriptSurface,
    /// The wall-clock second the view measures relative times from.
    now: i64,
}

impl Screen {
    /// Draws `state` into a screen of `width` by `height` cells at the epoch.
    #[must_use]
    pub fn view(state: &AppState, width: u16, height: u16) -> Self {
        Self::with_now(state, 0, width, height)
    }

    /// Draws `state` into a screen of `width` by `height` cells at `now`.
    ///
    /// `now` is the whole Unix second the view measures relative times from, so
    /// a test fixes the clock instead of reading it.
    #[must_use]
    pub fn with_now(state: &AppState, now: i64, width: u16, height: u16) -> Self {
        let mut screen = Self::empty(width, height);
        screen.now = now;
        screen.redraw(state);
        screen
    }

    /// Draws one widget into a screen of `width` by `height` cells.
    ///
    /// This is the pane-level constructor: the closure receives the frame and
    /// paints whatever the test wants to look at, exactly as a screen paints a
    /// pane.
    ///
    /// # Panics
    ///
    /// Panics when the test backend rejects the frame.
    #[must_use]
    pub fn widget(width: u16, height: u16, draw: impl FnOnce(&mut Frame)) -> Self {
        let mut screen = Self::empty(width, height);
        screen
            .terminal
            .draw(draw)
            .expect("the test backend accepts one frame");
        screen
    }

    /// Returns an empty screen of `width` by `height` cells.
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        Self::empty(width, height)
    }

    /// Returns an empty screen of `width` by `height` cells.
    fn empty(width: u16, height: u16) -> Self {
        Self {
            terminal: Terminal::new(TestBackend::new(width, height))
                .expect("the test backend starts a terminal"),
            cache: TranscriptLayoutCache::new(),
            surface: TranscriptSurface::new(),
            now: 0,
        }
    }

    /// Draws `state` again, so a test can read what one action changed.
    ///
    /// # Panics
    ///
    /// Panics when the test backend rejects the frame.
    pub fn redraw(&mut self, state: &AppState) {
        let now = self.now;
        let cache = &mut self.cache;
        let surface = &mut self.surface;
        self.terminal
            .draw(|frame| TerminalView::with_now(state, now, cache, surface).render(frame))
            .expect("the test backend accepts one frame");
    }

    /// Resizes the screen and draws `state` into its new size.
    pub fn resize(&mut self, state: &AppState, width: u16, height: u16) {
        self.terminal.backend_mut().resize(width, height);
        self.redraw(state);
    }

    /// Draws one pane through the screen's own cache and scroll surface.
    ///
    /// This is the pane constructor for the tests that read the layout after
    /// the frame: the cache and the surface belong to the screen, so the test
    /// can look at what the pane published.
    #[cfg(test)]
    pub(in crate::tui) fn pane(
        &mut self,
        draw: impl FnOnce(&mut Frame, &mut TranscriptLayoutCache, &mut TranscriptSurface),
    ) {
        let cache = &mut self.cache;
        let surface = &mut self.surface;
        self.terminal
            .draw(|frame| draw(frame, cache, surface))
            .expect("the test backend accepts one frame");
    }

    /// Returns the transcript window the last frame published.
    #[cfg(test)]
    #[must_use]
    pub(in crate::tui) fn window(&self) -> TranscriptWindow {
        self.cache.window()
    }

    /// Returns how many committed rows the last frame laid out.
    #[cfg(test)]
    #[must_use]
    pub fn layout_count(&self) -> u64 {
        self.cache.layout_count()
    }

    /// Returns how many display rows the committed transcript laid out.
    #[cfg(test)]
    #[must_use]
    pub(in crate::tui) fn committed_rows(&self) -> usize {
        self.cache.laid_out().len()
    }

    /// Returns the screen's buffer.
    #[must_use]
    pub fn buffer(&self) -> &Buffer {
        self.terminal.backend().buffer()
    }

    /// Returns one cell, or `None` outside the screen.
    #[must_use]
    pub fn cell(&self, column: u16, row: u16) -> Option<&Cell> {
        self.buffer().cell(Position::new(column, row))
    }

    /// Returns the first character of one cell, or `None` outside the screen.
    ///
    /// Every glyph this interface paints is one character, so a test that reads
    /// a border corner or a marker reads it here.
    #[must_use]
    pub fn glyph(&self, column: u16, row: u16) -> Option<char> {
        self.cell(column, row)
            .and_then(|cell| cell.symbol().chars().next())
    }

    /// Returns one row's text, trailing blanks trimmed.
    #[must_use]
    pub fn line(&self, row: u16) -> String {
        let buffer = self.buffer();
        if row >= buffer.area.height {
            return String::new();
        }
        let mut text = String::new();
        for column in 0..buffer.area.width {
            if let Some(cell) = self.cell(column, row) {
                text.push_str(cell.symbol());
            }
        }
        text.trim_end().to_owned()
    }

    /// Returns every row's text, joined by line breaks.
    #[must_use]
    pub fn screen_text(&self) -> String {
        let height = self.buffer().area.height;
        (0..height)
            .map(|row| self.line(row))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Returns the position of the first occurrence of `text`.
    ///
    /// The position is the cell the substring starts at: `str::find` measures
    /// bytes, and a border or a marker cell is more than one byte, so the byte
    /// offset is turned into a column through the row itself.
    #[must_use]
    pub fn find_text(&self, text: &str) -> Option<Position> {
        let height = self.buffer().area.height;
        (0..height).find_map(|row| {
            let line = self.line(row);
            let offset = line.find(text)?;
            let column = u16::try_from(line.get(..offset)?.chars().count()).ok()?;
            Some(Position::new(column, row))
        })
    }

    /// Asserts that the screen carries `text` somewhere.
    ///
    /// # Panics
    ///
    /// Panics when no row carries `text`.
    pub fn assert_contains(&self, text: &str) {
        assert!(
            self.find_text(text).is_some(),
            "the screen carries {text:?}:\n{}",
            self.screen_text()
        );
    }

    /// Asserts that the screen carries no `text` anywhere.
    ///
    /// # Panics
    ///
    /// Panics when a row carries `text`.
    pub fn assert_not_contains(&self, text: &str) {
        assert!(
            self.find_text(text).is_none(),
            "the screen carries no {text:?}:\n{}",
            self.screen_text()
        );
    }

    /// Asserts that one row carries `text`.
    ///
    /// # Panics
    ///
    /// Panics when the row carries other text.
    pub fn assert_line_contains(&self, row: u16, text: &str) {
        let line = self.line(row);
        assert!(
            line.contains(text),
            "row {row} carries {text:?}, not {line:?}"
        );
    }

    /// Asserts that one cell carries `glyph`.
    ///
    /// # Panics
    ///
    /// Panics when the cell is outside the screen or carries another glyph.
    pub fn assert_cell(&self, column: u16, row: u16, glyph: char) {
        let painted = self
            .glyph(column, row)
            .unwrap_or_else(|| panic!("the screen carries the cell ({column}, {row})"));
        assert_eq!(painted, glyph, "the cell ({column}, {row}) is {glyph:?}");
    }
}
