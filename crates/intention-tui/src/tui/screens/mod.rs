//! One file per surface of the revue view.
//!
//! [`RevueView`] is the pure function of one [`AppState`] the terminal renders.
//! The window has exactly one base - the chat panel on the canvas - and while
//! the core's screen carries the sessions browser, that browser is a layout row
//! docked to the bottom of the same window: the chat container shrinks to the
//! rows the browser leaves, its own bottom canvas margin row is the blank gap
//! between the two, and the browser's bottom edge is the window's last row. No
//! surface ever paints over another. The theme picker is not a second docked
//! surface: its screen keeps the one chat panel and spends the transcript's
//! rows on a panel directly above the input block, so the input block and the
//! detail line never move. Each module here owns the builders only its surface
//! reads, so a new surface is a module beside them and one arm in
//! [`RevueView::render`].

pub(super) mod chat;
pub(super) mod sessions;

use std::cell::RefCell;

use revue::layout::Rect;
use revue::widget::{RenderContext, Text, View, vstack};

use crate::app::{AppState, Screen};
use crate::tui::layout::TranscriptLayoutCache;
use crate::tui::palette;

/// The revue view: a pure function of one [`AppState`].
///
/// Every screen derives its content from the state on each call and mutates
/// nothing, so the same state renders identically in a revue test app and in a
/// terminal. The one cell the view can fill is the transcript's layout cache:
/// a front end owns it and renders every frame through it, while a view built
/// without one lays the transcript out through a cache of its own.
pub struct RevueView<'a> {
    state: &'a AppState,
    now: i64,
    cache: Option<&'a RefCell<TranscriptLayoutCache>>,
}

impl<'a> RevueView<'a> {
    /// Creates the view of one application state, reading the system clock.
    #[must_use]
    pub fn new(state: &'a AppState) -> Self {
        Self::with_now(state, system_now())
    }

    /// Creates the view of one application state at a fixed wall-clock second.
    ///
    /// `now` is the whole Unix second the view measures relative times from, so
    /// a render test fixes the clock instead of reading it.
    #[must_use]
    pub const fn with_now(state: &'a AppState, now: i64) -> Self {
        Self {
            state,
            now,
            cache: None,
        }
    }

    /// Creates the view of one application state through `cache`.
    ///
    /// The cache outlives the view, so the frames a front end draws reuse the
    /// rows it already laid out; this is the one constructor a running front
    /// end uses.
    #[must_use]
    pub(in crate::tui) fn cached(
        state: &'a AppState,
        cache: &'a RefCell<TranscriptLayoutCache>,
    ) -> Self {
        Self::with_cache(state, system_now(), cache)
    }

    /// Creates the view of one application state at a fixed wall-clock second,
    /// rendered through `cache`.
    ///
    /// A front end hands in the cache it owns, so its frames reuse the rows it
    /// already laid out; a view built without one renders through a cache of
    /// its own and lays the transcript out per render.
    #[must_use]
    pub(in crate::tui) const fn with_cache(
        state: &'a AppState,
        now: i64,
        cache: &'a RefCell<TranscriptLayoutCache>,
    ) -> Self {
        Self {
            state,
            now,
            cache: Some(cache),
        }
    }
}

impl View for RevueView<'_> {
    /// Renders the one window: the canvas, the chat panel on it, and - while
    /// the core's screen carries the browser - the browser docked as the
    /// window's bottom rows, with the chat container above it.
    ///
    /// The window never changes: docking or closing the browser, and selecting
    /// the session it points at, all repaint this same frame, so the chat keeps
    /// its transcript, its input, and its session while it reflows into the
    /// rows the browser leaves.
    fn render(&self, ctx: &mut RenderContext) {
        ctx.fill_box_background(palette::of(self.state.effective_theme()).canvas);
        let window = ctx.area;
        // @todo(hack): a view built without a cache - the test constructor -
        // allocates a throwaway layout cache on every render so both
        // constructors paint the same rows; the cache should be one
        // type-level state, not an `Option` with a per-frame fallback.
        let fresh = RefCell::new(TranscriptLayoutCache::new());
        let cache = self.cache.unwrap_or(&fresh);
        let panel = (self.state.screen() == Screen::Sessions)
            .then(|| sessions::geometry(self.state, window))
            .flatten();
        let Some(geometry) = panel else {
            chat::chat_screen(self.state, window, cache).render(ctx);
            return;
        };
        // @todo(revue): revue has no docking or overlay primitive, so this view
        // slices the window into the chat area and the panel's rows by hand,
        // with a blank `vstack` child as the gap; a dock or overlay layout
        // would express the same frame directly.
        // The chat container keeps every row the panel does not use, and its
        // own bottom canvas margin row is the one gap row above the panel.
        let chat_rows = window.height.saturating_sub(geometry.rows);
        let chat_area = Rect::new(window.x, window.y, window.width, chat_rows);
        chat::chat_screen(self.state, chat_area, cache).render(ctx);
        vstack()
            .child_sized(Text::new(""), chat_rows)
            .child_sized(
                sessions::panel(self.state, self.now, geometry),
                geometry.rows,
            )
            .render(ctx);
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
