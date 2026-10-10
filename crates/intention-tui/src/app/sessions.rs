//! The connection and session transitions: bootstrap, reconnect, the session
//! list, and session selection.

use intention_proto::{
    DaemonReadinessDto, ErrorDto, SessionId, SessionSnapshotDto, SessionSummariesDto,
    run_status_is_terminal,
};

use super::{AppState, ConnectionStatus, Effect, StreamStatus, short_identifier};

impl AppState {
    /// Records a ready daemon and asks for the list and the selected session.
    pub(super) fn apply_bootstrapped(&mut self, readiness: DaemonReadinessDto) -> Vec<Effect> {
        self.connection = ConnectionStatus::Ready(readiness);
        self.error = None;
        let mut effects = vec![Effect::ListSessions];
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

    /// Replaces the session window and opens the session a front end starts on.
    pub(super) fn apply_sessions_listed(&mut self, summaries: &SessionSummariesDto) -> Vec<Effect> {
        self.sessions.clear();
        self.sessions.extend_from_slice(summaries.sessions());
        self.sessions_omitted = summaries.omitted();
        self.sessions_loaded = true;
        // The browser shows the sessions the list carries, so a list that
        // arrives while it is open re-selects its rows under the same filter.
        self.browser.refresh(&self.sessions);
        self.error = None;
        // The daemon orders summaries by recency, so the first one is the session
        // a front end opens when the caller selected none. A session the caller
        // selected wins: its snapshot may arrive before or after this list, and
        // queuing the most recent one could then open over it.
        if self.session_id.is_none()
            && self.initial_session.is_none()
            && let Some(most_recent) = self.sessions.first()
        {
            return vec![Effect::OpenSession(most_recent.session_id())];
        }
        Vec::new()
    }

    /// Shows one session snapshot and opens the one subscription it needs.
    ///
    /// A snapshot re-reads committed state, so the previous session's
    /// subscription never survives it: the one live run the snapshot carries is
    /// subscribed to, and every other snapshot closes whatever stream the front
    /// end still holds. Both effects are exclusive: a front end holds exactly
    /// one subscription, and a session that opens either re-establishes it or
    /// drops it.
    pub(super) fn apply_session_snapshot(&mut self, snapshot: SessionSnapshotDto) -> Vec<Effect> {
        let session_id = snapshot.session_id();
        self.session_id = Some(session_id);
        self.session_mode = Some(snapshot.projection().mode());
        self.replace_transcript(snapshot.messages());
        self.active_run = snapshot.projection().active_run();
        // A snapshot re-reads committed state: a mirror of an earlier subscription
        // is stale, and its transient provisional text must not outlive it.
        self.run_stream = None;
        self.stream = StreamStatus::Inactive;
        // The elapsed time belongs to the turn the previous session was running.
        self.elapsed_millis = None;
        self.scroll = 0;
        self.error = None;
        // Opening a session records no notice: the status row already names the
        // session, and the notice line stays free for real transient events.
        let subscription = self.subscribe_to_active_run(session_id);
        if subscription.is_empty() {
            vec![Effect::CloseStream]
        } else {
            subscription
        }
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
        vec![Effect::CreateSession]
    }

    /// Re-reads current state from the daemon after a reconnect request.
    pub(super) fn request_reconnect(&mut self) -> Vec<Effect> {
        self.connection = ConnectionStatus::Connecting;
        // Transient stream state never survives a reconnect: the fresh snapshot
        // and subscription re-read current committed state.
        self.run_stream = None;
        self.stream = StreamStatus::Inactive;
        self.error = None;
        self.note("reconnecting".to_owned());
        vec![Effect::Connect]
    }

    /// Returns the subscription of a session whose run is still live, if any.
    fn subscribe_to_active_run(&self, session_id: SessionId) -> Vec<Effect> {
        let live_run = self
            .active_run
            .filter(|run| !run_status_is_terminal(run.status()));
        live_run.map_or_else(Vec::new, |run| {
            vec![Effect::Subscribe {
                session_id,
                run_id: run.run_id(),
            }]
        })
    }
}
