use intention_client::SessionStreamState;
use intention_proto::{
    DaemonHealthDto, ErrorDto, MessageProjectionDto, RunId, RunStatusDto, SendUserTurnOutcomeDto,
    SessionFrameDto, SessionId, TextDeltaChannelDto, TextDeltaFrameDto,
};
use intention_test_support::fixtures::{
    assistant_row, full_transcript, reasoning_row, run, snapshot, stream_state, user_row,
};

use super::{Action, AppState, Effect, StreamStatus};

/// Returns the client state the front end holds for its open session.
fn client_state(state: &AppState) -> &SessionStreamState {
    state
        .session_state
        .as_ref()
        .expect("the fixture opened a session")
}

/// Applies one committed transcript row to `state` as a stream content frame.
fn commit_row(state: &mut AppState, row: MessageProjectionDto) {
    let effects = receive(state, SessionFrameDto::Content(row));
    assert!(
        effects.is_empty(),
        "a committed row asks for no client work"
    );
}

fn delta(session_id: SessionId, run_id: RunId, step: u32, text: &str) -> SessionFrameDto {
    SessionFrameDto::TextDelta(
        TextDeltaFrameDto::new(session_id, run_id, step, TextDeltaChannelDto::Answer, text)
            .expect("the fixture delta is valid"),
    )
}

fn reasoning_delta(session_id: SessionId, run_id: RunId, step: u32, text: &str) -> SessionFrameDto {
    SessionFrameDto::TextDelta(
        TextDeltaFrameDto::new(
            session_id,
            run_id,
            step,
            TextDeltaChannelDto::Reasoning,
            text,
        )
        .expect("the fixture reasoning delta is valid"),
    )
}

fn status_frame(session_id: SessionId, run_id: RunId, status: RunStatusDto) -> SessionFrameDto {
    SessionFrameDto::Status(run(session_id, run_id, status))
}

fn failure() -> ErrorDto {
    ErrorDto::validation("fixture_failure", "the fixture step failed")
}

/// Opens `session_id` with the given transcript rows and returns the state.
fn opened_session(session_id: SessionId, messages: Vec<MessageProjectionDto>) -> AppState {
    let mut state = AppState::new(None);
    let effects = state.update(Action::SessionSnapshotLoaded(snapshot(
        session_id, None, messages,
    )));
    assert_eq!(
        effects,
        vec![Effect::Subscribe { session_id }],
        "an open session follows its own live stream"
    );
    state
}

/// Opens `session_id` and attaches a live run subscription in `status`.
fn running_session(session_id: SessionId, run_id: RunId, status: RunStatusDto) -> AppState {
    let mut state = opened_session(session_id, Vec::new());
    let effects = state.update(Action::SessionStreamOpened(stream_state(
        session_id,
        run_id,
        status,
        Vec::new(),
    )));
    assert!(
        effects.is_empty(),
        "opening a stream asks for no client work"
    );
    state
}

/// Applies one frame the way the live subscription does.
///
/// The client applies every frame to the one session state it exposes and hands
/// the frame, what it changed, and its result to the front end, so a test
/// derives a frame through the same entry the production pump uses instead of
/// restating the client's rules.
fn receive(state: &mut AppState, frame: SessionFrameDto) -> Vec<Effect> {
    let mut derived = state
        .session_state
        .clone()
        .unwrap_or_else(|| SessionStreamState::new(frame_session(&frame)));
    derived
        .apply_frame(frame)
        .expect("the fixture frame belongs to the state's own session");
    state.update(Action::FrameReceived(Box::new(derived)))
}

/// Returns the session one frame belongs to.
fn frame_session(frame: &SessionFrameDto) -> SessionId {
    match frame {
        SessionFrameDto::Content(row) => row.session_id(),
        SessionFrameDto::Status(run) => run.session_id(),
        SessionFrameDto::TextDelta(delta) => delta.session_id(),
    }
}

#[test]
fn bootstrap_opens_the_selected_session() {
    let session_id = SessionId::new();
    let mut state = AppState::new(Some(session_id));
    let effects = state.update(Action::Bootstrapped(DaemonHealthDto::ready()));
    assert_eq!(
        effects,
        vec![Effect::OpenSession(session_id)],
        "an explicitly selected session is opened at once"
    );
    assert_eq!(state.error(), None);
}

#[test]
fn bootstrap_without_a_selection_asks_for_nothing() {
    let mut state = AppState::new(None);
    let effects = state.update(Action::Bootstrapped(DaemonHealthDto::ready()));
    assert!(
        effects.is_empty(),
        "a launch with no session request reads nothing"
    );
}

#[test]
fn bootstrap_failure_is_reported_without_client_work() {
    let mut state = AppState::new(None);
    let effects = state.update(Action::BootstrapFailed(failure()));
    assert!(effects.is_empty());
    assert!(
        state
            .error()
            .is_some_and(|error| error.starts_with("fixture_failure:")),
        "the error line names the failure code"
    );
}

#[test]
fn a_session_snapshot_seeds_the_transcript_and_subscribes_to_the_session() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = AppState::new(None);
    let effects = state.update(Action::SessionSnapshotLoaded(snapshot(
        session_id,
        Some(run(session_id, run_id, RunStatusDto::Running)),
        vec![user_row(session_id, Some(run_id), "hello")],
    )));
    assert_eq!(
        effects,
        vec![Effect::Subscribe { session_id }],
        "an open session subscribes to its own stream, whatever its run is doing"
    );
    assert_eq!(state.session_id(), Some(session_id));
    assert_eq!(state.transcript().len(), 1);
    assert_eq!(state.run_status(), Some(RunStatusDto::Running));
    assert_eq!(state.stream, StreamStatus::Inactive);
    assert_eq!(client_state(&state).provisional_text(), "");
}

#[test]
fn a_terminal_run_in_the_snapshot_still_opens_the_session_stream() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = AppState::new(None);
    let effects = state.update(Action::SessionSnapshotLoaded(snapshot(
        session_id,
        Some(run(session_id, run_id, RunStatusDto::Completed)),
        Vec::new(),
    )));
    assert_eq!(
        effects,
        vec![Effect::Subscribe { session_id }],
        "a finished run is no reason to stop following the session"
    );
    assert_eq!(state.run_status(), Some(RunStatusDto::Completed));
}

#[test]
fn a_scripted_turn_leaves_as_one_turn_request() {
    // A scripted prompt is data: the slash grammar never sees it, so a
    // slash-prefixed prompt is a turn and nothing else runs.
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    let effects = state.update(Action::TurnSubmitted {
        content: "/theme light".to_owned(),
    });
    assert_eq!(
        effects,
        vec![Effect::SendTurn {
            session_id,
            content: "/theme light".to_owned(),
        }],
        "a scripted prompt leaves as one turn request"
    );
}

#[test]
fn a_prompt_with_no_session_is_one_create_command() {
    let mut state = AppState::new(None);
    let effects = state.update(Action::TurnSubmitted {
        content: "hello".to_owned(),
    });
    assert_eq!(
        effects,
        vec![Effect::CreateSession {
            first_turn: Some("hello".to_owned()),
        }],
        "a prompt with no session is one create command carrying the prompt"
    );
}

#[test]
fn creating_a_session_opens_it() {
    let session_id = SessionId::new();
    let mut state = AppState::new(None);
    let effects = state.update(Action::SessionCreated(session_id));
    assert_eq!(
        effects,
        vec![Effect::OpenSession(session_id)],
        "a created session is the one the front end shows"
    );
}

#[test]
fn a_request_for_a_new_session_asks_for_its_creation() {
    let mut state = AppState::new(None);
    let effects = state.update(Action::NewSessionRequested);
    assert_eq!(effects, vec![Effect::CreateSession { first_turn: None }]);
}

#[test]
fn a_create_failure_reaches_the_error_line() {
    let mut state = AppState::new(None);
    let effects = state.update(Action::SessionCreateFailed {
        carried_first_turn: true,
        error: failure(),
    });
    assert!(effects.is_empty());
    assert!(
        state
            .error()
            .is_some_and(|error| error.starts_with("fixture_failure:"))
    );
}

#[test]
fn an_accepted_turn_opens_the_session_stream_when_none_is_live() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    let effects = state.update(Action::TurnAccepted(SendUserTurnOutcomeDto::Started {
        run_id: RunId::new(),
    }));
    assert_eq!(
        effects,
        vec![Effect::Subscribe { session_id }],
        "a run this window starts arrives through its session stream"
    );
}

#[test]
fn an_accepted_turn_on_a_live_session_stream_asks_for_nothing() {
    let session_id = SessionId::new();
    let mut state = running_session(session_id, RunId::new(), RunStatusDto::Running);
    let effects = state.update(Action::TurnAccepted(SendUserTurnOutcomeDto::Started {
        run_id: RunId::new(),
    }));
    assert!(
        effects.is_empty(),
        "the run a turn starts reaches the stream this window already follows"
    );
}

#[test]
fn an_accepted_pending_turn_waits_for_the_active_run() {
    let session_id = SessionId::new();
    let mut state = running_session(session_id, RunId::new(), RunStatusDto::Running);
    let effects = state.update(Action::TurnAccepted(SendUserTurnOutcomeDto::Pending));
    assert!(
        effects.is_empty(),
        "a queued turn opens no stream of its own"
    );
}

#[test]
fn a_turn_failure_sets_the_error_line() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    state.update(Action::TurnFailed(failure()));
    assert!(
        state
            .error()
            .is_some_and(|error| error.starts_with("fixture_failure:"))
    );
}

#[test]
fn a_text_delta_shows_provisional_text_and_a_committed_row_clears_it() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    receive(&mut state, delta(session_id, run_id, 0, "hel"));
    receive(&mut state, delta(session_id, run_id, 0, "lo"));
    assert_eq!(client_state(&state).provisional_text(), "hello");
    assert!(
        state.transcript().is_empty(),
        "a delta is not committed state"
    );

    receive(
        &mut state,
        SessionFrameDto::Content(assistant_row(session_id, run_id, "hello")),
    );
    assert_eq!(
        client_state(&state).provisional_text(),
        "",
        "the committed row replaces the delta tail"
    );
    assert_eq!(state.transcript().len(), 1);
    assert_eq!(state.transcript()[0].text(), "hello");
}

#[test]
fn a_new_model_step_replaces_the_provisional_tail() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    receive(&mut state, delta(session_id, run_id, 0, "first"));
    receive(&mut state, delta(session_id, run_id, 1, "second"));
    assert_eq!(client_state(&state).provisional_text(), "second");
}

#[test]
fn the_reasoning_channel_buffers_apart_from_the_answer_it_precedes() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    receive(&mut state, reasoning_delta(session_id, run_id, 0, "weigh"));
    receive(&mut state, reasoning_delta(session_id, run_id, 0, "ing"));
    receive(&mut state, delta(session_id, run_id, 0, "the answer"));
    assert_eq!(client_state(&state).provisional_reasoning(), "weighing");
    assert_eq!(client_state(&state).provisional_text(), "the answer");

    receive(
        &mut state,
        SessionFrameDto::Content(assistant_row(session_id, run_id, "the answer")),
    );
    assert_eq!(
        client_state(&state).provisional_reasoning(),
        "",
        "the committed assistant row supersedes the live reasoning segment"
    );
    assert_eq!(client_state(&state).provisional_text(), "");
}

#[test]
fn a_content_frame_reports_every_row_appended_since_an_epoch() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    let epoch = client_state(&state).transcript_epoch();
    assert_eq!(
        client_state(&state).transcript_appended_since(epoch),
        Some(0),
        "a transcript that changed nothing appended nothing"
    );

    receive(
        &mut state,
        SessionFrameDto::Content(assistant_row(session_id, run_id, "first")),
    );
    assert_eq!(
        client_state(&state).transcript_appended_since(epoch),
        Some(1)
    );
    receive(
        &mut state,
        SessionFrameDto::Content(assistant_row(session_id, run_id, "second")),
    );
    assert_eq!(
        client_state(&state).transcript_appended_since(epoch),
        Some(2)
    );
}

#[test]
fn a_trim_of_the_oldest_rows_reports_no_append_since_the_previous_epoch() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = opened_session(session_id, Vec::new());
    state.update(Action::SessionStreamOpened(full_transcript(
        session_id,
        run_id,
        |index| reasoning_row(session_id, run_id, &format!("row {index}"), "thought"),
    )));
    let retained = state.transcript().len();
    let epoch = client_state(&state).transcript_epoch();

    commit_row(
        &mut state,
        assistant_row(session_id, run_id, "the newest row"),
    );
    assert_eq!(
        state.transcript().len(),
        retained,
        "the client's window keeps its newest rows"
    );
    assert_eq!(
        client_state(&state).transcript_appended_since(epoch),
        None,
        "a front drain is a replacement, never an append"
    );
}

#[test]
fn a_row_another_window_committed_joins_the_transcript() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    commit_row(&mut state, user_row(session_id, None, "typed elsewhere"));
    assert_eq!(state.transcript().len(), 1);
    assert_eq!(state.transcript()[0].text(), "typed elsewhere");
}

#[test]
fn a_frame_without_a_subscription_is_dropped() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = opened_session(session_id, Vec::new());
    assert_eq!(state.stream, StreamStatus::Inactive);
    let mut derived = SessionStreamState::new(session_id);
    let frame = SessionFrameDto::Content(assistant_row(session_id, run_id, "stale"));
    derived
        .apply_frame(frame)
        .expect("the fixture frame belongs to the state's own session");
    let effects = state.update(Action::FrameReceived(Box::new(derived)));
    assert!(effects.is_empty());
    assert!(
        state.transcript().is_empty(),
        "a frame for a subscription this window no longer holds is stale"
    );
}

#[test]
fn a_terminal_status_frame_reports_the_final_status() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    receive(
        &mut state,
        status_frame(session_id, run_id, RunStatusDto::Completed),
    );
    assert_eq!(state.run_status(), Some(RunStatusDto::Completed));
    assert_eq!(state.live_run(), None, "a finished run is not a live run");
}

#[test]
fn a_closed_stream_drops_the_provisional_tail() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    receive(&mut state, delta(session_id, run_id, 0, "half"));
    let effects = state.update(Action::SessionStreamEnded);
    assert_eq!(state.stream, StreamStatus::Ended);
    assert_eq!(
        client_state(&state).provisional_text(),
        "",
        "transient text does not outlive its stream"
    );
    assert_eq!(
        effects,
        vec![Effect::Connect],
        "the stream this window lost is the one it asks to re-establish"
    );
    assert_eq!(
        state.live_run(),
        None,
        "a run without a live subscription is not a live run"
    );
}

#[test]
fn a_failed_stream_reports_the_failure() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    let effects = state.update(Action::SessionStreamFailed(failure()));
    assert_eq!(state.stream, StreamStatus::Failed);
    assert_eq!(client_state(&state).provisional_text(), "");
    assert!(
        state.error().is_some(),
        "the failure stays on the error line while the reconnect runs"
    );
    assert_eq!(
        effects,
        vec![Effect::Connect],
        "a failed stream asks for the same one reconnect"
    );
    assert_eq!(
        state.live_run(),
        None,
        "a lost subscription never keeps offering to stop a run"
    );
}

#[test]
fn a_second_lost_stream_asks_for_no_reconnect() {
    // The budget is one reconnect per lost stream: the first loss asks for it,
    // and a second loss inside the same recovery asks for nothing, so the front
    // end reports its typed failure instead of retrying in a loop. Nothing here
    // restores the budget, because only a frame the state accepts does.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    assert_eq!(
        state.update(Action::SessionStreamFailed(failure())),
        vec![Effect::Connect],
        "the first loss asks for the one reconnect"
    );
    assert_eq!(
        state.update(Action::SessionStreamEnded),
        Vec::<Effect>::new(),
        "a second loss inside one recovery asks for no second reconnect"
    );
    assert!(
        state.error().is_some(),
        "the typed failure stays visible on the error line"
    );
}

#[test]
fn a_stream_that_closed_again_without_delivering_a_frame_asks_for_no_reconnect() {
    // The bound: a peer that accepts every subscription and closes it before
    // delivering a frame leaves the budget spent, so the loss that follows the
    // bare open asks for nothing and the front end reports its typed failure
    // instead of bootstrapping in a loop.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    assert_eq!(
        state.update(Action::SessionStreamEnded),
        vec![Effect::Connect],
        "the first loss spends the one reconnect"
    );
    state.update(Action::SessionStreamOpened(stream_state(
        session_id,
        run_id,
        RunStatusDto::Running,
        Vec::new(),
    )));
    assert_eq!(
        state.update(Action::SessionStreamEnded),
        Vec::<Effect>::new(),
        "a stream that opened and delivered no frame restores nothing"
    );
    assert_eq!(
        state.update(Action::SessionStreamFailed(failure())),
        Vec::<Effect>::new(),
        "the budget stays spent across a further loss"
    );
}

#[test]
fn a_stream_delivering_a_frame_restores_the_reconnect_budget() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    assert_eq!(
        state.update(Action::SessionStreamEnded),
        vec![Effect::Connect]
    );
    state.update(Action::SessionStreamOpened(stream_state(
        session_id,
        run_id,
        RunStatusDto::Running,
        Vec::new(),
    )));
    commit_row(&mut state, assistant_row(session_id, run_id, "traffic"));
    assert_eq!(
        state.update(Action::SessionStreamEnded),
        vec![Effect::Connect],
        "a stream that carried a frame owes the next loss its reconnect"
    );
}

#[test]
fn a_live_run_is_stopped_by_an_accepted_stop() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    let effects = state.update(Action::CancelRunRequested);
    assert_eq!(
        effects,
        vec![Effect::CancelRun { session_id, run_id }],
        "the stop names the run this window observes"
    );
    assert_eq!(
        state.update(Action::RunCancelAccepted),
        Vec::<Effect>::new()
    );
}

#[test]
fn a_stop_without_a_live_run_asks_for_nothing() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    assert_eq!(
        state.update(Action::CancelRunRequested),
        Vec::<Effect>::new(),
        "a front end with no live run asks the daemon for no stop"
    );
}

#[test]
fn a_failed_stop_reaches_the_error_line() {
    let session_id = SessionId::new();
    let mut state = running_session(session_id, RunId::new(), RunStatusDto::Running);
    state.update(Action::RunCancelFailed(failure()));
    assert!(
        state
            .error()
            .is_some_and(|error| error.starts_with("fixture_failure:"))
    );
}

#[test]
fn a_lost_stream_re_reads_the_session_after_the_reconnect() {
    // The reconnect is the one resynchronization this protocol has: the client
    // bootstraps again, and the snapshot it reads is what the state shows.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    assert_eq!(
        state.update(Action::SessionStreamFailed(failure())),
        vec![Effect::Connect]
    );
    let effects = state.update(Action::Bootstrapped(DaemonHealthDto::ready()));
    assert_eq!(
        effects,
        vec![Effect::OpenSession(session_id)],
        "the reconnect re-reads the session it was following"
    );
}
