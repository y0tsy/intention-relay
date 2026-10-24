//! The connection and session transitions: bootstrap, reconnect, the session
//! list, and session selection.

use intention_client::SessionStreamState;
use intention_proto::{ErrorDto, SessionId, SessionSnapshotDto};

use super::{AppState, Effect, StreamStatus};

impl AppState {
    /// Records a ready daemon and asks for the selected session.
    pub(super) fn apply_bootstrapped(&mut self) -> Vec<Effect> {
        self.error = None;
        self.session_id
            .or(self.initial_session)
            .map_or_else(Vec::new, |session_id| {
                vec![Effect::OpenSession(session_id)]
            })
    }

    /// Records an unreachable daemon.
    pub(super) fn apply_bootstrap_failed(&mut self, error: &ErrorDto) -> Vec<Effect> {
        self.apply_failure(error)
    }

    /// Shows one session snapshot and opens the one subscription it needs.
    ///
    /// A snapshot re-reads committed state, so the previous session's
    /// subscription never survives it: the open session's own stream is
    /// (re-)established, and it carries every committed row, the active run's
    /// projection, and the active run's transient text - whoever committed them
    /// in that session. A front end holds exactly one subscription.
    pub(super) fn apply_session_snapshot(&mut self, snapshot: SessionSnapshotDto) -> Vec<Effect> {
        let session_id = snapshot.session_id();
        let read = SessionStreamState::from_snapshot(snapshot);
        if let Err(error) = self.merge_transcript(read) {
            // The client classified the read as another session's: the front end
            // reports that failure instead of printing rows it cannot own.
            return self.apply_failure(&error);
        }
        self.session_id = Some(session_id);
        self.stream = StreamStatus::Inactive;
        self.error = None;
        // Opening a session records no notice: the notice line stays free for
        // real transient events.
        self.ensure_session_stream()
    }

    /// Opens the session the daemon just created.
    pub(super) fn apply_session_created(&mut self, session_id: SessionId) -> Vec<Effect> {
        self.error = None;
        vec![Effect::OpenSession(session_id)]
    }

    /// Records the request for a new session and asks for its creation.
    pub(super) fn request_new_session(&mut self) -> Vec<Effect> {
        vec![Effect::CreateSession { first_turn: None }]
    }

    /// Returns the subscription that makes the open session live, if any.
    ///
    /// A front end follows the session it has open, not one run: the run a turn
    /// starts - its own or another window's - reaches the state as a status
    /// frame, so a stream that is already live needs nothing and a lost one is
    /// re-established by the caller that observes it.
    pub(super) fn ensure_session_stream(&self) -> Vec<Effect> {
        match (self.session_id, self.stream) {
            (Some(session_id), status) if status != StreamStatus::Live => {
                vec![Effect::Subscribe { session_id }]
            }
            _ => Vec::new(),
        }
    }
}
