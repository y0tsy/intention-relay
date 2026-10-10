//! The layered Esc: close the hint menu, cancel the live run, clear the typed
//! line, or exit.
//!
//! One key press on the chat screen means one of four things, decided by the
//! state it finds and by the arm the previous press left:
//!
//! - the input's command hint menu is open: the press closes it and means
//!   nothing else - no clear arm, no cancel, no exit - so dismissing the hint
//!   list can never abandon the line or end the front end;
//! - a run whose last known status is not terminal: the press cancels it
//!   immediately, with no arming - a single press is the cancel, the way a
//!   shell cancels a running command;
//! - a non-empty input line: the first press arms the clear with a notice, and
//!   the second consecutive press abandons the line into the recallable history
//!   and clears the input, exactly as one Ctrl+C press does;
//! - an empty input line: the press leaves the front end.
//!
//! The clear is the Ctrl+C clear: same guard (no live run), same effect, and
//! the same consecutive rule - [`AppState::update`] disarms it for every other
//! user action, including Ctrl+C, while a client report leaves it standing.
//! The sessions browser maps Esc to its own close action and the theme picker
//! maps it to dropping its preview, each in its own keymap, so the three
//! meanings never meet: while either surface owns the keyboard, the chat's
//! cancel, clear, and exit arms are unreachable.
//!
//! `Ctrl+Q` stays the one immediate exit; Esc exits only when there is no live
//! run and nothing typed to clear first.

use intention_proto::run_status_is_terminal;

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
            self.escape_arm = false;
            return Vec::new();
        }
        if self
            .active_run
            .is_some_and(|run| !run_status_is_terminal(run.status()))
        {
            // The cancel is a single press, so no arm survives it.
            self.escape_arm = false;
            return self.request_interrupt();
        }
        if !self.input.trim().is_empty() {
            if self.escape_arm {
                self.escape_arm = false;
                // The arm's own instruction does not outlive the clear it
                // asked for.
                self.notice = None;
                return self.clear_input_into_history();
            }
            self.escape_arm = true;
            self.note(CLEAR_ARM_NOTICE.to_owned());
            return Vec::new();
        }
        self.escape_arm = false;
        self.quit = true;
        Vec::new()
    }
}
