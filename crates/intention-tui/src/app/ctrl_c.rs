//! The layered Ctrl+C: interrupt the live run, abandon the line, or exit.
//!
//! One key press means one of three things, decided by the state it finds and
//! by the arm the previous press left:
//!
//! - a run whose last known status is not terminal: the first press arms the
//!   interrupt with a notice, and the second consecutive press fires it;
//! - a non-empty input line: the press moves the line into the recallable
//!   history, clears the input and its cursor, and counts towards no sequence;
//! - an empty input line: the first press arms the exit with a notice, and
//!   the second consecutive press leaves the front end.
//!
//! The arm is consecutive over the user's own actions: [`AppState::update`]
//! clears it for every other action the user asks for, while a client report
//! (`Action::is_client_report`) leaves it standing, because a live run's
//! frames and the front end's elapsed reports arrive between two presses.

use intention_proto::run_status_is_terminal;

use super::{AppState, Effect, short_identifier};

/// The notice an exit arm shows until its consecutive press arrives.
const EXIT_ARM_NOTICE: &str = "press Ctrl+C again to exit";

/// What the last Ctrl+C press armed for the next consecutive press.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CtrlCArm {
    /// The next consecutive press interrupts the run the notice named.
    Interrupt,
    /// The next consecutive press leaves the front end.
    Exit,
}

impl AppState {
    /// Applies one Ctrl+C press: the layered interrupt, recall, and exit request.
    pub(super) fn apply_ctrl_c(&mut self) -> Vec<Effect> {
        if let Some(run) = self
            .active_run
            .filter(|run| !run_status_is_terminal(run.status()))
        {
            if self.ctrl_c_arm == Some(CtrlCArm::Interrupt) {
                self.ctrl_c_arm = None;
                return self.request_interrupt();
            }
            self.ctrl_c_arm = Some(CtrlCArm::Interrupt);
            self.note(format!(
                "press Ctrl+C again to interrupt run {}",
                short_identifier(run.run_id())
            ));
            return Vec::new();
        }
        if !self.input.trim().is_empty() {
            // The abandoned line goes to the history instead of an exit
            // sequence, so this press never counts towards a quit.
            self.ctrl_c_arm = None;
            return self.clear_input_into_history();
        }
        if self.ctrl_c_arm == Some(CtrlCArm::Exit) {
            self.ctrl_c_arm = None;
            self.quit = true;
            return Vec::new();
        }
        self.ctrl_c_arm = Some(CtrlCArm::Exit);
        self.note(EXIT_ARM_NOTICE.to_owned());
        Vec::new()
    }
}
