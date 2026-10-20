//! The one consecutive-press arm the layered keys share.
//!
//! A key that needs two consecutive presses to act leaves an [`Arm`] naming
//! what its next press fires and shows the notice that asks for that press; the
//! second press fires it and drops the arm. The arm is consecutive over the
//! user's own actions: [`AppState::update`] disarms it for every action that is
//! not a press of the key it belongs to, while a client report
//! (`Action::is_client_report`) leaves it standing, because a live run's frames
//! and the front end's elapsed reports arrive between two presses.
//!
//! The arm names the act, not only the key, because the act is what decides
//! whether a press fires it: a stop arm fires only while its run is still live,
//! and an exit arm fires only when the input line is still empty and no run is
//! live, so a state the press no longer matches starts a fresh sequence instead
//! of firing a stale one.

use super::{Action, AppState};

/// What the last press of a layered key armed for its next consecutive press.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Arm {
    /// The next Ctrl+C press stops the run the notice named.
    Stop,
    /// The next Ctrl+C press leaves the front end.
    Exit,
    /// The next Esc press abandons the typed line into the recallable history.
    Clear,
}

impl Arm {
    /// Returns whether `action` is the press this arm waits for.
    ///
    /// The stop and exit arms belong to Ctrl+C and the clear arm to Esc, so a
    /// press of the other layered key starts a fresh sequence instead of firing
    /// this one.
    pub(super) const fn is_pressed_by(self, action: &Action) -> bool {
        matches!(
            (self, action),
            (Self::Stop | Self::Exit, Action::CtrlCPressed) | (Self::Clear, Action::EscapePressed)
        )
    }
}

impl AppState {
    /// Returns whether the previous press armed `arm` for this one.
    pub(super) fn is_armed(&self, arm: Arm) -> bool {
        self.armed == Some(arm)
    }

    /// Arms `arm` for the next press and shows the notice that asks for it.
    pub(super) fn arm(&mut self, arm: Arm, notice: String) {
        self.armed = Some(arm);
        self.note(notice);
    }

    /// Drops the arm without acting on it.
    pub(super) fn disarm(&mut self) {
        self.armed = None;
    }
}
