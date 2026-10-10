//! Render-free application state for every terminal front end.
//!
//! This module owns every session, transcript, input, and status decision: a
//! front end feeds it typed [`Action`] values and dispatches the [`Effect`] values
//! it returns. Nothing here touches a terminal, a socket, or a clock, so the whole
//! state machine is testable without a daemon, and the terminal view is a pure
//! function of this state.

mod action;
mod browser;
mod commands;
mod ctrl_c;
mod escape;
mod input;
mod run;
mod sessions;
mod state;
mod theme;
mod transcript;

#[cfg(test)]
mod tests;

pub use action::{
    Action, BrowserCursorMove, ConnectionStatus, Effect, InputCursorMove, InputHistoryMove,
    RunPhase, StreamStatus, TranscriptScroll,
};
pub use browser::{BrowserRow, BrowserTab};
pub use commands::{
    ArgumentSpec, COMMANDS, CommandInvocation, CommandMenu, CommandResolution, CommandSpec,
    MenuKind, MenuMove, MenuRow, ValueSpec,
};
pub use input::TRANSCRIPT_DRAG_ROWS;
pub use state::AppState;
pub use theme::Theme;
pub use transcript::TranscriptSelection;

use intention_proto::ErrorDto;

/// The surface of the one window that owns the keyboard.
///
/// The terminal front end has exactly one window: the chat is always drawn, and
/// [`Screen::Sessions`] draws the sessions browser as a modal card over it.
/// Selecting a session, creating one, or closing the browser returns the state
/// to [`Screen::Chat`] without the window ever changing, so the chat keeps its
/// transcript, its input line, and its session throughout. [`Screen::Theme`]
/// keeps the same window too: the theme picker is one more row region of the
/// chat panel, directly above the input block, so the input block and the
/// detail line never move while it is open.
///
/// The screen is a value of the render-free core: a front end renders the one
/// the state carries and never tracks a surface of its own.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Screen {
    /// The chat surface: transcript, input line, and status.
    Chat,
    /// The sessions browser card behind `/sessions`, over the chat.
    Sessions,
    /// The theme picker behind `/theme`, above the chat's input block.
    Theme,
}

/// How many leading characters of a durable identifier compact terminal text shows.
const SHORT_IDENTIFIER_CHARS: usize = 8;

/// Returns the leading characters of one display identifier for compact text.
#[must_use]
pub fn short_identifier(identifier: impl std::fmt::Display) -> String {
    identifier
        .to_string()
        .chars()
        .take(SHORT_IDENTIFIER_CHARS)
        .collect()
}

/// Returns the single-line status text of one safe typed error.
fn error_text(error: &ErrorDto) -> String {
    format!("{}: {}", error.code(), error.message())
}
