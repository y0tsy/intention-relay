//! One file per pane of the revue view.
//!
//! Each pane module owns its pure builder and the title, line, or detail
//! helpers only that pane reads. The single pane list lives in the chat
//! screen's `chat_screen` function, so a new pane is a new module here plus its
//! place in that list.

pub(super) mod input;
pub(super) mod markdown;
pub(super) mod status;
pub(super) mod tools;
pub(super) mod transcript;
pub(super) mod welcome;
