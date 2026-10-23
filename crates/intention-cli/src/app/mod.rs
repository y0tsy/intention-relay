//! Render-free application state for the line front ends.
//!
//! This module owns the session, transcript, and run decisions both line front
//! ends share: a front end feeds it typed [`Action`] values and dispatches the
//! [`Effect`] values it returns. Nothing here touches a terminal, a socket, or a
//! clock, so the whole state machine is testable without a daemon. The
//! fullscreen surface is a separate application with its own state.

mod action;
mod commands;
mod run;
mod sessions;
mod state;

#[cfg(test)]
mod tests;

pub use action::{Action, Effect, StreamStatus};
pub use commands::{CommandAction, CommandInvocation, resolve_command};
pub use state::AppState;

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
