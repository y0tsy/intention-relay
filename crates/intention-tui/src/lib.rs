//! Terminal front ends for the shared local client.
//!
//! [`app`] is the render-free state machine the terminal front end drives: typed
//! client events in, state and client effects out. [`client_task`] is the one
//! async mapper from those effects to shared client calls. [`tui`] is the
//! fullscreen terminal front end over that state. This crate creates no daemon
//! implementation, accesses no domain service, and retains no durable state.

pub mod app;
pub mod client_task;
pub mod tui;
