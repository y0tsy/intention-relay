//! The layered Esc: close the hint menu, stop the live run, clear the typed
//! line, or exit.
//!
//! One key press on the chat screen means one of four things, decided by the
//! state it finds and by the arm the previous press left:
//!
//! - the input's command hint menu is open: the press closes it and means
//!   nothing else - no clear arm, no stop, no exit - so dismissing the hint
//!   list can never abandon the line or end the front end;
//! - a run a live subscription observes: the press stops it immediately, with
//!   no arming - a single press is the stop, the way a shell cancels a running
//!   command;
//! - a non-empty input line: the first press arms the clear with a notice, and
//!   the second consecutive press abandons the line into the recallable history
//!   and clears the input, exactly as one Ctrl+C press does;
//! - an empty input line: the press leaves the front end.
//!
//! The clear is the Ctrl+C clear: same guard (no live run), same effect, and
//! the same consecutive rule - the shared arm (`super::arm`) is disarmed for
//! every other user action, including Ctrl+C, while a client report leaves it
//! standing. The sessions browser maps Esc to its own close action and the
//! theme picker maps it to dropping its preview, each in its own keymap, so the
//! three meanings never meet: while either surface owns the keyboard, the
//! chat's stop, clear, and exit arms are unreachable.
//!
//! The run these arms read is the one [`AppState::live_run`] observes: a run
//! whose subscription died no longer stops, clears, or exits on its behalf, so
//! an unreachable daemon can never trap the front end.
//!
//! `Ctrl+Q` stays the one immediate exit; Esc exits only when there is no live
//! run and nothing typed to clear first.

use super::arm::Arm;
use super::{AppState, Effect};

/// The notice a clear arm shows until its consecutive press arrives.
const CLEAR_ARM_NOTICE: &str = "press Esc again to clear the input";

impl AppState {
    /// Applies one Esc press on the chat screen.
    pub(super) fn apply_escape(&mut self) -> Vec<Effect> {
        // The hint menu is the innermost layer: while it is open, one press
        // closes it and touches nothing else, so the clear arm, the live run,
        // and the exit are all unreachable until the menu is gone.
        if self.command_menu.take().is_some() {
            self.disarm();
            return Vec::new();
        }
        if self.live_run().is_some() {
            // The stop is a single press, so no arm survives it.
            self.disarm();
            return self.request_cancel();
        }
        if !self.input.trim().is_empty() {
            if self.is_armed(Arm::Clear) {
                self.disarm();
                // The arm's own instruction does not outlive the clear it
                // asked for.
                self.notice = None;
                return self.clear_input_into_history();
            }
            self.arm(Arm::Clear, CLEAR_ARM_NOTICE.to_owned());
            return Vec::new();
        }
        self.disarm();
        self.quit = true;
        Vec::new()
    }
}
