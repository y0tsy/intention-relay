//! The connection and session transitions: bootstrap, reconnect, the session
//! list, and session selection.

use intention_client::SessionStreamState;
use intention_proto::{
    DaemonReadinessDto, ErrorDto, SessionId, SessionSnapshotDto, SessionSummariesDto,
};

use super::{AppState, ConnectionStatus, Effect, StreamStatus, short_identifier};

impl AppState {
    /// Records a ready daemon and asks for the list, the settings, and the
    /// selected session.
    pub(super) fn apply_bootstrapped(&mut self, readiness: DaemonReadinessDto) -> Vec<Effect> {
        self.connection = ConnectionStatus::Ready(readiness);
        self.error = None;
        // The settings read answers once the connection is ready; its reply
        // arrives as its own action, so the committed theme is the daemon's
        // value and never a terminal guess.
        let mut effects = vec![Effect::ListSessions, Effect::LoadSettings];
        if let Some(session_id) = self.session_id.or(self.initial_session) {
            effects.push(Effect::OpenSession(session_id));
        }
        effects
    }

    /// Records an unreachable daemon.
    pub(super) fn apply_bootstrap_failed(&mut self, error: &ErrorDto) -> Vec<Effect> {
        self.connection = ConnectionStatus::Failed;
        self.apply_failure(error)
    }

    /// Replaces the session window.
    ///
    /// A launch with no session request opens nothing: the list is recorded and
    /// the chat keeps its welcome state until the user asks for a session (an
    /// explicit `/new` or a row of the sessions browser). A launch that asked to
    /// continue never reaches this state with an unresolved target: the front
    /// end resolves the continue rule through the client and hands the state the
    /// session it chose, exactly like `--session`. A session already open is
    /// never re-opened by a list read.
    pub(super) fn apply_sessions_listed(&mut self, summaries: &SessionSummariesDto) -> Vec<Effect> {
        self.sessions.clear();
        self.sessions.extend_from_slice(summaries.sessions());
        self.sessions_omitted = summaries.omitted();
        self.sessions_loaded = true;
        // The browser shows the sessions the list carries, so a list that
        // arrives while it is open re-selects its rows under the same filter.
        self.browser.refresh(&self.sessions);
        self.error = None;
        Vec::new()
    }

    /// Shows one session snapshot and opens the one subscription it needs.
    ///
    /// A snapshot re-reads committed state, so the previous session's
    /// subscription never survives it: the open session's own stream is
    /// (re-)established, and it carries every committed row, the active run's
    /// projection, and the active run's transient text - whoever committed them
    /// in that session. A front end holds exactly one subscription.
    ///
    /// The snapshot is the client's read of the session's durable transcript:
    /// the state it returns is the front end's one row list, so the panes render
    /// exactly the rows the client read.
    pub(super) fn apply_session_snapshot(&mut self, snapshot: SessionSnapshotDto) -> Vec<Effect> {
        let session_id = snapshot.session_id();
        let mode = snapshot.projection().mode();
        let model = snapshot.projection().provider_model().map(str::to_owned);
        let read = SessionStreamState::from_snapshot(snapshot);
        if let Err(error) = self.merge_transcript(read) {
            // The client classified the read as another session's: the front end
            // reports that failure instead of rendering rows it cannot own.
            return self.apply_failure(&error);
        }
        self.session_id = Some(session_id);
        self.session_mode = Some(mode);
        self.session_model = model;
        // A read replaces the rows the front end held, so every display-row
        // offset moved: the selection and the reasoning expansions go with it.
        self.note_replacement();
        self.stream = StreamStatus::Inactive;
        // The elapsed time belongs to the turn the previous session was running.
        self.elapsed_millis = None;
        self.scroll = 0;
        self.error = None;
        // Opening a session records no notice: the status row already names the
        // session, and the notice line stays free for real transient events.
        self.ensure_session_stream()
    }

    /// Opens the session the daemon just created.
    pub(super) fn apply_session_created(&mut self, session_id: SessionId) -> Vec<Effect> {
        self.error = None;
        self.note(format!("session {} created", short_identifier(session_id)));
        vec![Effect::OpenSession(session_id)]
    }

    /// Records the request for a new session and asks for its creation.
    pub(super) fn request_new_session(&mut self) -> Vec<Effect> {
        self.note("creating a session".to_owned());
        vec![Effect::CreateSession { first_turn: None }]
    }

    /// Re-reads current state from the daemon after a reconnect request.
    ///
    /// The request is the ask that follows an observed stream loss: the
    /// transient observation ends - the stream that streamed the provisional
    /// text is gone - and the one reconnect the loss is owed goes out. A budget
    /// already spent asks for nothing, which is how the driver learns that the
    /// recovery is over.
    pub(super) fn request_reconnect(&mut self) -> Vec<Effect> {
        self.stream = StreamStatus::Inactive;
        if let Some(state) = self.session_state.as_mut() {
            state.end_stream();
        }
        self.error = None;
        self.reconnect_once()
    }

    /// Returns the subscription that makes the open session live, if any.
    ///
    /// A window follows the session it has open, not one run: the run a turn
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
