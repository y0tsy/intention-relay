#![allow(
    clippy::expect_used,
    reason = "Unit tests build typed fixture DTOs directly and assert them for diagnostics."
)]

use intention_client::{RETAINED_TRANSCRIPT_MESSAGES, RunStreamState};
use intention_proto::{
    ConfigRevisionId, DaemonHealthDto, ErrorDto, MessageKindDto, MessageProjectionDto, ProjectId,
    RunId, RunModeDto, RunProjectionDto, RunStatusDto, RunStreamFrameDto,
    RunSubscriptionSnapshotDto, SendUserTurnOutcomeDto, SessionId, SessionProjectionDto,
    SessionSnapshotDto, SessionSummariesDto, SessionSummaryDto, TextDeltaChannelDto,
    TextDeltaFrameDto, TurnId, WorkspaceId, WorkspaceRootDto,
};

use super::{
    Action, AppState, BrowserCursorMove, BrowserTab, ConnectionStatus, Effect, InputCursorMove,
    InputHistoryMove, Screen, StreamStatus, TRANSCRIPT_DRAG_ROWS, TranscriptScroll,
};

/// The canonical id of the first fixture session.
const FIRST_SESSION: &str = "11111111-1111-4111-8111-111111111111";

/// The canonical id of the second fixture session.
const SECOND_SESSION: &str = "22222222-2222-4222-8222-222222222222";

/// Returns the fixture session id spelled by one canonical UUID literal.
fn fixture_session(value: &str) -> SessionId {
    SessionId::parse(value).expect("the fixture session id is canonical")
}

fn workspace_root() -> WorkspaceRootDto {
    WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy().into_owned())
        .expect("the process temporary directory is an absolute workspace root")
}

fn run(session_id: SessionId, run_id: RunId, status: RunStatusDto) -> RunProjectionDto {
    RunProjectionDto::new(
        session_id,
        run_id,
        TurnId::new(),
        status,
        ConfigRevisionId::new(),
    )
}

fn session_projection(
    session_id: SessionId,
    active_run: Option<RunProjectionDto>,
) -> SessionProjectionDto {
    SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        RunModeDto::Build,
        None,
        active_run,
        Vec::new(),
    )
    .expect("the fixture session projection is coherent")
}

fn snapshot(
    session_id: SessionId,
    active_run: Option<RunProjectionDto>,
    messages: Vec<MessageProjectionDto>,
) -> SessionSnapshotDto {
    SessionSnapshotDto::with_projection(
        session_id,
        session_projection(session_id, active_run),
        messages,
    )
    .expect("the fixture session snapshot is coherent")
}

/// Returns one session snapshot whose projection declares `mode`.
fn snapshot_in_mode(
    session_id: SessionId,
    mode: RunModeDto,
    messages: Vec<MessageProjectionDto>,
) -> SessionSnapshotDto {
    let projection = SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        mode,
        None,
        None,
        Vec::new(),
    )
    .expect("the fixture session projection is coherent");
    SessionSnapshotDto::with_projection(session_id, projection, messages)
        .expect("the fixture session snapshot is coherent")
}

fn summary(session_id: SessionId, updated_at: i64) -> SessionSummaryDto {
    SessionSummaryDto::new(
        session_id,
        ProjectId::new(),
        WorkspaceId::new(),
        RunModeDto::Build,
        updated_at,
        None,
    )
}

fn summaries(sessions: Vec<SessionSummaryDto>) -> SessionSummariesDto {
    SessionSummariesDto::new(sessions, 0).expect("the fixture session list is coherent")
}

fn stream_state(
    session_id: SessionId,
    run_id: RunId,
    status: RunStatusDto,
    messages: Vec<MessageProjectionDto>,
) -> RunStreamState {
    let snapshot = RunSubscriptionSnapshotDto::new(run(session_id, run_id, status), messages)
        .expect("the fixture run snapshot is coherent");
    let mut state = RunStreamState::new(session_id, run_id);
    state
        .apply_initial(snapshot)
        .expect("the fixture run snapshot applies to its own scope");
    state
}

fn user_row(session_id: SessionId, run_id: Option<RunId>, text: &str) -> MessageProjectionDto {
    MessageProjectionDto::new(
        session_id,
        run_id,
        MessageKindDto::User,
        text,
        None,
        None,
        None,
    )
    .expect("the fixture user row is coherent")
}

fn assistant_row(session_id: SessionId, run_id: RunId, text: &str) -> MessageProjectionDto {
    MessageProjectionDto::new(
        session_id,
        Some(run_id),
        MessageKindDto::Assistant,
        text,
        None,
        None,
        None,
    )
    .expect("the fixture assistant row is coherent")
}

/// Returns one committed assistant row carrying `reasoning` above its text.
fn reasoning_row(
    session_id: SessionId,
    run_id: RunId,
    text: &str,
    reasoning: &str,
) -> MessageProjectionDto {
    MessageProjectionDto::new(
        session_id,
        Some(run_id),
        MessageKindDto::Assistant,
        text,
        Some(reasoning.to_owned()),
        None,
        None,
    )
    .expect("the fixture reasoning row is coherent")
}

fn delta(session_id: SessionId, run_id: RunId, step: u32, text: &str) -> RunStreamFrameDto {
    RunStreamFrameDto::TextDelta(
        TextDeltaFrameDto::new(session_id, run_id, step, TextDeltaChannelDto::Answer, text)
            .expect("the fixture delta is valid"),
    )
}

fn reasoning_delta(
    session_id: SessionId,
    run_id: RunId,
    step: u32,
    text: &str,
) -> RunStreamFrameDto {
    RunStreamFrameDto::TextDelta(
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

fn status_frame(session_id: SessionId, run_id: RunId, status: RunStatusDto) -> RunStreamFrameDto {
    RunStreamFrameDto::Status(run(session_id, run_id, status))
}

fn failure() -> ErrorDto {
    ErrorDto::validation("fixture_failure", "the fixture step failed")
}

/// Types `text` into the input line, one character action per character.
fn type_text(state: &mut AppState, text: &str) {
    for character in text.chars() {
        state.update(Action::InputChar(character));
    }
}

/// Types `text` into the sessions browser filter, one character action each.
fn type_browser_filter(state: &mut AppState, text: &str) {
    for character in text.chars() {
        state.update(Action::BrowserFilterChar(character));
    }
}

/// Returns the session identity of every browser row, in the order shown.
fn browser_row_ids(state: &AppState) -> Vec<SessionId> {
    state
        .browser_rows()
        .iter()
        .map(|row| row.session_id())
        .collect()
}

/// Opens `session_id` with the given transcript rows and returns the state.
fn opened_session(session_id: SessionId, messages: Vec<MessageProjectionDto>) -> AppState {
    let mut state = AppState::new(None);
    let effects = state.update(Action::SessionSnapshotLoaded(snapshot(
        session_id, None, messages,
    )));
    assert_eq!(
        effects,
        vec![Effect::CloseStream],
        "a session without a live run closes any previous live subscription"
    );
    state
}

/// Opens `session_id` and the sessions browser over `sessions`.
fn opened_browser(session_id: SessionId, sessions: Vec<SessionSummaryDto>) -> AppState {
    let mut state = opened_session(session_id, Vec::new());
    state.update(Action::SessionsListed(summaries(sessions)));
    state.update(Action::SessionsBrowserRequested);
    state
}

/// Opens `session_id` and attaches a live run subscription in `status`.
fn running_session(session_id: SessionId, run_id: RunId, status: RunStatusDto) -> AppState {
    let mut state = opened_session(session_id, Vec::new());
    let effects = state.update(Action::RunStreamOpened(stream_state(
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

#[test]
fn start_connects_the_shared_client() {
    let state = AppState::new(None);
    assert_eq!(state.start(), vec![Effect::Connect]);
    assert_eq!(state.connection(), ConnectionStatus::Connecting);
    assert!(!state.should_quit());
    assert_eq!(state.session_id(), None);
    assert!(state.transcript().is_empty());
    assert_eq!(state.input(), "");
}

#[test]
fn bootstrap_lists_sessions_and_opens_the_selected_session() {
    let session_id = SessionId::new();
    let mut state = AppState::new(Some(session_id));
    let effects = state.update(Action::Bootstrapped(DaemonHealthDto::ready()));
    assert_eq!(
        effects,
        vec![Effect::ListSessions, Effect::OpenSession(session_id)],
        "an explicitly selected session is opened without waiting for the list"
    );
    assert_eq!(
        state.connection(),
        ConnectionStatus::Ready(intention_proto::DaemonReadinessDto::Ready)
    );
    assert_eq!(state.error(), None);
}

#[test]
fn bootstrap_failure_is_reported_without_client_work() {
    let mut state = AppState::new(None);
    let effects = state.update(Action::BootstrapFailed(failure()));
    assert!(effects.is_empty());
    assert_eq!(state.connection(), ConnectionStatus::Failed);
    assert!(
        state
            .error()
            .is_some_and(|error| error.starts_with("fixture_failure:")),
        "the error line names the failure code"
    );
}

#[test]
fn listing_without_a_session_request_opens_nothing() {
    let recent = SessionId::new();
    let older = SessionId::new();
    let mut state = AppState::new(None);
    state.update(Action::Bootstrapped(DaemonHealthDto::ready()));
    let effects = state.update(Action::SessionsListed(summaries(vec![
        summary(recent, 20),
        summary(older, 10),
    ])));
    assert!(
        effects.is_empty(),
        "a launch that asked for no session opens none"
    );
    assert_eq!(state.session_id(), None);
    assert!(state.sessions_loaded());
    assert_eq!(state.sessions_omitted(), 0);
    assert_eq!(state.sessions().len(), 2, "the list stays readable");
}

#[test]
fn listing_opens_the_most_recent_session_when_the_caller_asked_to_continue() {
    let recent = SessionId::new();
    let older = SessionId::new();
    let mut state = AppState::new(None).continuing(true);
    state.update(Action::Bootstrapped(DaemonHealthDto::ready()));
    let effects = state.update(Action::SessionsListed(summaries(vec![
        summary(recent, 20),
        summary(older, 10),
    ])));
    assert_eq!(
        effects,
        vec![Effect::OpenSession(recent)],
        "an explicit continuation opens the newest session the daemon reports"
    );
}

#[test]
fn a_continuation_with_no_sessions_to_continue_opens_nothing() {
    let mut state = AppState::new(None).continuing(true);
    state.update(Action::Bootstrapped(DaemonHealthDto::ready()));
    let effects = state.update(Action::SessionsListed(summaries(Vec::new())));
    assert!(effects.is_empty());
    assert_eq!(state.session_id(), None);
    assert!(state.sessions_loaded());
}

#[test]
fn listing_never_auto_opens_over_an_explicitly_selected_session() {
    let selected = SessionId::new();
    let most_recent = SessionId::new();
    let mut state = AppState::new(Some(selected));
    assert_eq!(
        state.update(Action::Bootstrapped(DaemonHealthDto::ready())),
        vec![Effect::ListSessions, Effect::OpenSession(selected)],
        "the selected session is opened without waiting for the list"
    );
    // The list reply can overtake the selected session's snapshot, so the list
    // must not queue the most recent session while one was selected.
    let effects = state.update(Action::SessionsListed(summaries(vec![
        summary(most_recent, 20),
        summary(selected, 10),
    ])));
    assert!(
        effects.is_empty(),
        "the most recent session is never queued over a selected one"
    );
    assert_eq!(state.session_id(), None);

    state.update(Action::SessionSnapshotLoaded(snapshot(
        selected,
        None,
        Vec::new(),
    )));
    assert_eq!(
        state.session_id(),
        Some(selected),
        "the selected session stays current"
    );
}

#[test]
fn listing_keeps_an_open_session_and_reports_its_omitted_count() {
    let open = SessionId::new();
    let mut state = opened_session(open, Vec::new());
    let effects = state.update(Action::SessionsListed(
        SessionSummariesDto::new(vec![summary(open, 20)], 7)
            .expect("the fixture session list is coherent"),
    ));
    assert!(
        effects.is_empty(),
        "an open session is not re-opened by a list read"
    );
    assert_eq!(state.sessions_omitted(), 7);
}

#[test]
fn a_session_list_failure_keeps_the_last_known_list() {
    let session_id = SessionId::new();
    let mut state = AppState::new(None);
    state.update(Action::SessionsListed(summaries(vec![summary(
        session_id, 20,
    )])));
    state.update(Action::SessionsListFailed(failure()));
    assert_eq!(
        state.sessions().len(),
        1,
        "the last known list stays readable"
    );
    assert!(state.sessions_loaded());
    assert!(state.error().is_some());
}

#[test]
fn a_session_snapshot_seeds_the_transcript_and_subscribes_to_a_live_run() {
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
        vec![Effect::Subscribe { session_id, run_id }],
        "an open session with a live run subscribes to it"
    );
    assert_eq!(state.session_id(), Some(session_id));
    assert_eq!(state.transcript().len(), 1);
    assert_eq!(state.run_status(), Some(RunStatusDto::Running));
    assert_eq!(state.stream(), StreamStatus::Inactive);
    assert_eq!(state.provisional_text(), "");
    assert_eq!(
        state.notice(),
        None,
        "opening a session needs no notice: the status row names the session"
    );
}

#[test]
fn a_terminal_run_in_the_snapshot_is_never_subscribed() {
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
        vec![Effect::CloseStream],
        "a terminal run opens no subscription while the previous stream closes"
    );
    assert_eq!(state.run_status(), Some(RunStatusDto::Completed));
}

#[test]
fn submitting_a_turn_sends_its_trimmed_content_and_clears_the_input() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    for character in "  hi  ".chars() {
        state.update(Action::InputChar(character));
    }
    assert_eq!(state.input(), "  hi  ");
    let effects = state.update(Action::InputSubmitted);
    assert_eq!(
        effects,
        vec![Effect::SendTurn {
            session_id,
            content: "hi".to_owned(),
        }]
    );
    assert_eq!(state.input(), "");
}

#[test]
fn submitting_without_a_session_starts_one_for_the_prompt() {
    let mut state = AppState::new(None);
    type_text(&mut state, "hello");
    let effects = state.update(Action::InputSubmitted);
    assert_eq!(
        effects,
        vec![Effect::CreateSession],
        "a prompt with no session starts the session it needs"
    );
    assert_eq!(state.input(), "");
    assert_eq!(state.cursor(), 0);
    assert_eq!(state.notice(), Some("creating a session"));
}

#[test]
fn a_launch_with_no_session_request_keeps_the_welcome_and_the_first_prompt_creates_a_session() {
    let listed = SessionId::new();
    let mut state = AppState::new(None);
    state.update(Action::Bootstrapped(DaemonHealthDto::ready()));
    // The daemon reports a session, and the launch opens none of it: no
    // session request was made.
    let effects = state.update(Action::SessionsListed(summaries(vec![summary(listed, 20)])));
    assert!(effects.is_empty());
    assert_eq!(state.session_id(), None);
    assert!(state.transcript().is_empty());

    // The first prompt starts the session it needs and is sent as its first
    // turn once the created session opens.
    type_text(&mut state, "hello");
    assert_eq!(
        state.update(Action::InputSubmitted),
        vec![Effect::CreateSession]
    );
    let created = SessionId::new();
    assert_eq!(
        state.update(Action::SessionCreated(created)),
        vec![Effect::OpenSession(created)]
    );
    assert_eq!(
        state.update(Action::SessionSnapshotLoaded(snapshot(
            created,
            None,
            Vec::new(),
        ))),
        vec![
            Effect::CloseStream,
            Effect::SendTurn {
                session_id: created,
                content: "hello".to_owned(),
            },
        ],
        "the created session carries the prompt as its first turn"
    );
}

#[test]
fn a_failed_session_creation_drops_the_prompt_it_was_starting() {
    let mut state = AppState::new(None);
    type_text(&mut state, "hello");
    state.update(Action::InputSubmitted);
    let effects = state.update(Action::SessionCreateFailed(failure()));
    assert!(effects.is_empty());
    assert!(
        state.error().is_some(),
        "the failure reaches the error line"
    );
    // The session a later snapshot opens is not the one the prompt asked for,
    // so the prompt is gone and never fires against it.
    let other = SessionId::new();
    let effects = state.update(Action::SessionSnapshotLoaded(snapshot(
        other,
        None,
        Vec::new(),
    )));
    assert_eq!(
        effects,
        vec![Effect::CloseStream],
        "a session opened after the failure carries no prompt"
    );
    state.update(Action::NavigateInputHistory(InputHistoryMove::Previous));
    assert_eq!(
        state.input(),
        "hello",
        "the prompt stays recallable through the input history"
    );
}

#[test]
fn submitting_blank_input_sends_nothing() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    state.update(Action::InputChar(' '));
    let effects = state.update(Action::InputSubmitted);
    assert!(effects.is_empty());
    assert_eq!(state.input(), " ");
}

#[test]
fn a_slash_new_command_creates_a_session() {
    let mut state = AppState::new(None);
    type_text(&mut state, "/new");
    let effects = state.update(Action::InputSubmitted);
    assert_eq!(effects, vec![Effect::CreateSession]);
    assert_eq!(state.input(), "");
    assert_eq!(state.cursor(), 0);
    assert_eq!(state.notice(), Some("creating a session"));
}

#[test]
fn an_unknown_slash_command_lists_the_known_commands() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    type_text(&mut state, "/watch");
    let effects = state.update(Action::InputSubmitted);
    assert!(effects.is_empty());
    assert_eq!(state.input(), "");
    assert_eq!(
        state.notice(),
        Some("unknown command /watch; known commands: /new /sessions")
    );
}

#[test]
fn a_slash_command_never_enters_the_input_history() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    type_text(&mut state, "/new");
    state.update(Action::InputSubmitted);
    state.update(Action::NavigateInputHistory(InputHistoryMove::Previous));
    assert_eq!(state.input(), "", "a command line is never recallable");
}

#[test]
fn text_that_merely_contains_a_slash_is_still_a_turn() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    type_text(&mut state, "read src/lib.rs");
    let effects = state.update(Action::InputSubmitted);
    assert_eq!(
        effects,
        vec![Effect::SendTurn {
            session_id,
            content: "read src/lib.rs".to_owned(),
        }],
        "only a leading slash makes a line a command"
    );
}

#[test]
fn the_sessions_command_opens_the_browser_and_refreshes_the_list() {
    let session_id = fixture_session(FIRST_SESSION);
    let mut state = opened_session(session_id, Vec::new());
    type_text(&mut state, "/sessions");
    let effects = state.update(Action::InputSubmitted);
    assert_eq!(
        effects,
        vec![Effect::ListSessions],
        "the browser refreshes the list it shows"
    );
    assert_eq!(state.screen(), Screen::Sessions);
    assert_eq!(
        state.browser_tab(),
        BrowserTab::All,
        "the browser starts on the only tab the core can fill"
    );
    assert_eq!(state.browser_filter(), "");
    assert_eq!(state.input(), "", "a command never stays in the input line");
}

#[test]
fn escape_closes_the_browser_without_changing_the_session() {
    let open = fixture_session(FIRST_SESSION);
    let other = fixture_session(SECOND_SESSION);
    let mut state = opened_browser(open, vec![summary(other, 20)]);
    type_browser_filter(&mut state, "other");
    assert_eq!(state.browser_filter(), "other");

    let effects = state.update(Action::SessionsBrowserClosed);
    assert!(
        effects.is_empty(),
        "closing the browser asks for no client work"
    );
    assert_eq!(state.screen(), Screen::Chat);
    assert_eq!(
        state.session_id(),
        Some(open),
        "the chat keeps the session it showed"
    );

    state.update(Action::SessionsBrowserRequested);
    assert_eq!(state.screen(), Screen::Sessions);
    assert_eq!(
        state.browser_filter(),
        "",
        "a reopened browser starts from an empty filter"
    );
    assert_eq!(state.browser_tab(), BrowserTab::All);
}

#[test]
fn the_browser_tabs_cycle_forward_and_backward() {
    let open = fixture_session(FIRST_SESSION);
    let mut state = opened_browser(open, vec![summary(open, 20)]);
    for tab in [
        BrowserTab::Exec,
        BrowserTab::Favorites,
        BrowserTab::Archived,
        BrowserTab::CurrentFolder,
        BrowserTab::All,
    ] {
        state.update(Action::BrowserTabNext);
        assert_eq!(state.browser_tab(), tab);
    }
    for tab in [
        BrowserTab::CurrentFolder,
        BrowserTab::Archived,
        BrowserTab::Favorites,
        BrowserTab::Exec,
        BrowserTab::All,
    ] {
        state.update(Action::BrowserTabPrevious);
        assert_eq!(state.browser_tab(), tab);
    }
}

#[test]
fn a_stub_tab_shows_no_rows_and_names_the_missing_core_fact() {
    let open = fixture_session(FIRST_SESSION);
    let mut state = opened_browser(open, vec![summary(open, 20)]);
    assert_eq!(
        browser_row_ids(&state),
        vec![open],
        "the `All` tab selects every listed session"
    );
    state.update(Action::BrowserTabNext);
    assert_eq!(state.browser_tab(), BrowserTab::Exec);
    assert!(state.browser_rows().is_empty(), "a stub tab shows no row");
    let reason = state
        .browser_empty_reason()
        .expect("a stub tab explains its empty state");
    assert!(
        reason.starts_with("@todo(core):"),
        "the reason names the missing core fact: {reason}"
    );
    assert_eq!(
        state.browser_cursor(),
        0,
        "a result without rows has no cursor"
    );
}

#[test]
fn the_browser_filter_appends_and_deletes_characters() {
    let first = fixture_session(FIRST_SESSION);
    let second = fixture_session(SECOND_SESSION);
    let mut state = opened_browser(first, vec![summary(first, 30), summary(second, 20)]);
    type_browser_filter(&mut state, "bu");
    assert_eq!(state.browser_filter(), "bu");
    assert_eq!(
        browser_row_ids(&state),
        vec![first, second],
        "both fixture sessions run in build mode"
    );
    state.update(Action::BrowserFilterBackspace);
    assert_eq!(state.browser_filter(), "b");
    state.update(Action::BrowserFilterBackspace);
    state.update(Action::BrowserFilterBackspace);
    assert_eq!(
        state.browser_filter(),
        "",
        "backspace stops at the empty filter"
    );
    assert_eq!(browser_row_ids(&state), vec![first, second]);
}

#[test]
fn the_browser_filter_matches_a_title_subsequence_and_the_mode() {
    let first = fixture_session(FIRST_SESSION);
    let second = fixture_session(SECOND_SESSION);
    for (filter, expected) in [
        ("1111", vec![first]),
        ("2222", vec![second]),
        ("111111112222", vec![]),
        ("BUIL", vec![first, second]),
    ] {
        let mut state = opened_browser(first, vec![summary(first, 30), summary(second, 20)]);
        type_browser_filter(&mut state, filter);
        assert_eq!(browser_row_ids(&state), expected, "filter {filter}");
    }
}

#[test]
fn a_filter_that_matches_no_row_explains_the_empty_result() {
    let first = fixture_session(FIRST_SESSION);
    let mut state = opened_browser(first, vec![summary(first, 30)]);
    type_browser_filter(&mut state, "zzz");
    assert!(state.browser_rows().is_empty());
    assert_eq!(
        state.browser_empty_reason(),
        Some("no session matches the filter")
    );
    assert_eq!(state.browser_cursor(), 0);
}

#[test]
fn the_browser_cursor_stays_clamped_to_the_filtered_rows() {
    let first = fixture_session(FIRST_SESSION);
    let second = fixture_session(SECOND_SESSION);
    let mut state = opened_browser(first, vec![summary(first, 30), summary(second, 20)]);
    assert_eq!(state.browser_cursor(), 0);
    state.update(Action::BrowserCursorMove(BrowserCursorMove::Up));
    assert_eq!(
        state.browser_cursor(),
        0,
        "the cursor cannot leave the newest row"
    );
    state.update(Action::BrowserCursorMove(BrowserCursorMove::Down));
    assert_eq!(state.browser_cursor(), 1);
    state.update(Action::BrowserCursorMove(BrowserCursorMove::Down));
    assert_eq!(
        state.browser_cursor(),
        1,
        "the cursor cannot leave the oldest row"
    );

    type_browser_filter(&mut state, "1111");
    assert_eq!(browser_row_ids(&state), vec![first]);
    assert_eq!(
        state.browser_cursor(),
        0,
        "a filter that shortens the result clamps the cursor to it"
    );
}

#[test]
fn enter_opens_the_row_the_cursor_points_at_and_returns_to_the_chat() {
    let first = fixture_session(FIRST_SESSION);
    let second = fixture_session(SECOND_SESSION);
    let mut state = opened_browser(first, vec![summary(first, 30), summary(second, 20)]);
    state.update(Action::BrowserCursorMove(BrowserCursorMove::Down));
    let effects = state.update(Action::BrowserSelectionRequested);
    assert_eq!(
        effects,
        vec![Effect::OpenSession(second)],
        "Enter opens the cursor row"
    );
    assert_eq!(state.screen(), Screen::Chat);

    // A session without a live run carries no subscription, so the front end
    // drops the one it still held instead of leaving it subscribed.
    let effects = state.update(Action::SessionSnapshotLoaded(snapshot(
        second,
        None,
        Vec::new(),
    )));
    assert_eq!(effects, vec![Effect::CloseStream]);
    assert_eq!(state.session_id(), Some(second));
    assert_eq!(state.stream(), StreamStatus::Inactive);
}

#[test]
fn entering_a_session_with_a_live_run_subscribes_instead_of_closing() {
    let first = fixture_session(FIRST_SESSION);
    let second = fixture_session(SECOND_SESSION);
    let run_id = RunId::new();
    let mut state = opened_browser(first, vec![summary(first, 30), summary(second, 20)]);
    state.update(Action::BrowserCursorMove(BrowserCursorMove::Down));
    assert_eq!(
        state.update(Action::BrowserSelectionRequested),
        vec![Effect::OpenSession(second)]
    );
    let effects = state.update(Action::SessionSnapshotLoaded(snapshot(
        second,
        Some(run(second, run_id, RunStatusDto::Running)),
        Vec::new(),
    )));
    assert_eq!(
        effects,
        vec![Effect::Subscribe {
            session_id: second,
            run_id,
        }],
        "a snapshot with a live run re-establishes the one subscription"
    );
}

#[test]
fn enter_without_a_row_notes_it_and_keeps_the_browser_open() {
    let first = fixture_session(FIRST_SESSION);
    let mut state = opened_browser(first, vec![summary(first, 30)]);
    type_browser_filter(&mut state, "zzz");
    let effects = state.update(Action::BrowserSelectionRequested);
    assert!(effects.is_empty());
    assert_eq!(state.screen(), Screen::Sessions);
    assert_eq!(state.notice(), Some("no session to select"));
}

#[test]
fn the_unsupported_browser_requests_note_and_change_nothing() {
    let first = fixture_session(FIRST_SESSION);
    let mut state = opened_browser(first, vec![summary(first, 30)]);
    for action in [
        Action::BrowserRenameRequested,
        Action::BrowserArchiveRequested,
        Action::BrowserTreeRequested,
    ] {
        let effects = state.update(action.clone());
        assert!(effects.is_empty(), "{action:?} asks for no client work yet");
        assert!(
            state
                .notice()
                .is_some_and(|notice| notice.contains("needs core support")),
            "{action:?} reports why it is unsupported"
        );
        assert_eq!(state.screen(), Screen::Sessions);
        assert_eq!(state.session_id(), Some(first));
        assert_eq!(browser_row_ids(&state), vec![first]);
    }
}

#[test]
fn a_list_that_arrives_while_the_browser_is_open_re_selects_its_rows() {
    let first = fixture_session(FIRST_SESSION);
    let second = fixture_session(SECOND_SESSION);
    let mut state = opened_browser(first, vec![summary(first, 30)]);
    assert_eq!(browser_row_ids(&state), vec![first]);
    state.update(Action::SessionsListed(summaries(vec![
        summary(first, 30),
        summary(second, 40),
    ])));
    assert_eq!(
        browser_row_ids(&state),
        vec![second, first],
        "a fresh list is selected again, most recently updated first"
    );
}

#[test]
fn input_editing_keeps_the_buffer_in_order() {
    let mut state = AppState::new(None);
    for character in "abc".chars() {
        state.update(Action::InputChar(character));
    }
    state.update(Action::InputBackspace);
    assert_eq!(state.input(), "ab");
}

#[test]
fn typing_inserts_at_the_cursor() {
    let mut state = AppState::new(None);
    type_text(&mut state, "ac");
    state.update(Action::MoveInputCursor(InputCursorMove::Left));
    assert_eq!(state.cursor(), 1);
    state.update(Action::InputChar('b'));
    assert_eq!(state.input(), "abc");
    assert_eq!(state.cursor(), 2);
}

#[test]
fn cursor_movement_stops_at_both_ends_of_the_line() {
    let mut state = AppState::new(None);
    state.update(Action::MoveInputCursor(InputCursorMove::Left));
    assert_eq!(
        state.cursor(),
        0,
        "the cursor cannot leave the line's start"
    );
    type_text(&mut state, "ab");
    state.update(Action::MoveInputCursor(InputCursorMove::Right));
    assert_eq!(state.cursor(), 2, "the cursor cannot leave the line's end");
}

#[test]
fn home_and_end_stay_inside_the_cursor_line() {
    let mut state = AppState::new(None);
    type_text(&mut state, "ab\ncdef");
    state.update(Action::MoveInputCursor(InputCursorMove::Home));
    assert_eq!(state.cursor(), 3, "Home reaches the line's first character");
    state.update(Action::MoveInputCursor(InputCursorMove::Left));
    assert_eq!(state.cursor(), 2, "Left steps across the line break");
    state.update(Action::MoveInputCursor(InputCursorMove::End));
    assert_eq!(state.cursor(), 2, "End stops at the break ending the line");
    state.update(Action::MoveInputCursor(InputCursorMove::Right));
    assert_eq!(state.cursor(), 3);
    state.update(Action::MoveInputCursor(InputCursorMove::End));
    assert_eq!(
        state.cursor(),
        7,
        "End reaches the last line's last character"
    );
}

#[test]
fn a_backslash_before_enter_inserts_a_line_break_instead_of_submitting() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    type_text(&mut state, "line one\\");
    let effects = state.update(Action::InputSubmitted);
    assert!(effects.is_empty(), "the break does not submit the prompt");
    assert_eq!(state.input(), "line one\n");
    assert!(
        !state.input().contains('\\'),
        "the escape itself never reaches the buffer"
    );
    type_text(&mut state, "line two");
    assert_eq!(state.input(), "line one\nline two");
    let effects = state.update(Action::InputSubmitted);
    assert_eq!(
        effects,
        vec![Effect::SendTurn {
            session_id,
            content: "line one\nline two".to_owned(),
        }],
        "submitting sends every line unchanged"
    );
}

#[test]
fn backspace_deletes_the_character_before_the_cursor() {
    let mut state = AppState::new(None);
    type_text(&mut state, "abc");
    state.update(Action::MoveInputCursor(InputCursorMove::Left));
    state.update(Action::InputBackspace);
    assert_eq!(state.input(), "ac", "the character before the cursor goes");
    assert_eq!(state.cursor(), 1);
    state.update(Action::InputBackspace);
    assert_eq!(state.input(), "c");
    assert_eq!(state.cursor(), 0);
    state.update(Action::InputBackspace);
    assert_eq!(state.input(), "c", "nothing is deleted before the start");
}

#[test]
fn history_recall_walks_newest_first_and_restores_the_draft() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    type_text(&mut state, "first");
    state.update(Action::InputSubmitted);
    type_text(&mut state, "second");
    state.update(Action::InputSubmitted);
    type_text(&mut state, "draft");

    state.update(Action::NavigateInputHistory(InputHistoryMove::Previous));
    assert_eq!(state.input(), "second");
    assert_eq!(state.cursor(), "second".chars().count());
    state.update(Action::NavigateInputHistory(InputHistoryMove::Previous));
    assert_eq!(state.input(), "first");
    state.update(Action::NavigateInputHistory(InputHistoryMove::Previous));
    assert_eq!(state.input(), "first", "the oldest entry is the floor");

    state.update(Action::NavigateInputHistory(InputHistoryMove::Next));
    assert_eq!(state.input(), "second");
    state.update(Action::NavigateInputHistory(InputHistoryMove::Next));
    assert_eq!(
        state.input(),
        "draft",
        "walking past the newest entry restores the in-progress line"
    );
}

#[test]
fn clearing_the_input_into_history_keeps_it_recallable() {
    let mut state = AppState::new(None);
    type_text(&mut state, "abandoned");
    state.update(Action::MoveInputCursor(InputCursorMove::Left));
    let effects = state.update(Action::ClearInput);
    assert!(effects.is_empty());
    assert_eq!(state.input(), "");
    assert_eq!(state.cursor(), 0);
    state.update(Action::NavigateInputHistory(InputHistoryMove::Previous));
    assert_eq!(state.input(), "abandoned");
}

#[test]
fn clearing_a_blank_input_records_nothing() {
    let mut state = AppState::new(None);
    state.update(Action::ClearInput);
    state.update(Action::NavigateInputHistory(InputHistoryMove::Previous));
    assert_eq!(state.input(), "", "a blank line never enters the history");
}

#[test]
fn an_accepted_turn_subscribes_to_the_run_it_started() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = opened_session(session_id, Vec::new());
    let effects = state.update(Action::TurnAccepted(SendUserTurnOutcomeDto::Started {
        run_id,
        config_revision_id: ConfigRevisionId::new(),
    }));
    assert_eq!(effects, vec![Effect::Subscribe { session_id, run_id }]);
    let expected = format!("run {} started", super::short_identifier(run_id));
    assert_eq!(state.notice(), Some(expected.as_str()));
}

#[test]
fn an_accepted_pending_turn_waits_for_the_active_run() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    let effects = state.update(Action::TurnAccepted(SendUserTurnOutcomeDto::Pending));
    assert!(
        effects.is_empty(),
        "a queued turn opens no stream of its own"
    );
    assert_eq!(state.notice(), Some("turn queued behind the active run"));
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
    state.update(Action::FrameReceived(delta(session_id, run_id, 0, "hel")));
    state.update(Action::FrameReceived(delta(session_id, run_id, 0, "lo")));
    assert_eq!(state.provisional_text(), "hello");
    assert!(
        state.transcript().is_empty(),
        "a delta is not committed state"
    );

    state.update(Action::FrameReceived(RunStreamFrameDto::Content(
        assistant_row(session_id, run_id, "hello"),
    )));
    assert_eq!(
        state.provisional_text(),
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
    state.update(Action::FrameReceived(delta(session_id, run_id, 0, "first")));
    state.update(Action::FrameReceived(delta(
        session_id, run_id, 1, "second",
    )));
    assert_eq!(state.provisional_text(), "second");
}

#[test]
fn the_reasoning_channel_buffers_apart_from_the_answer_it_precedes() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    state.update(Action::FrameReceived(reasoning_delta(
        session_id, run_id, 0, "weigh",
    )));
    state.update(Action::FrameReceived(reasoning_delta(
        session_id, run_id, 0, "ing",
    )));
    state.update(Action::FrameReceived(delta(
        session_id,
        run_id,
        0,
        "the answer",
    )));
    assert_eq!(state.provisional_reasoning(), "weighing");
    assert_eq!(state.provisional_text(), "the answer");

    state.update(Action::FrameReceived(RunStreamFrameDto::Content(
        assistant_row(session_id, run_id, "the answer"),
    )));
    assert_eq!(
        state.provisional_reasoning(),
        "",
        "the committed assistant row supersedes the live reasoning segment"
    );
    assert_eq!(state.provisional_text(), "");
}

#[test]
fn the_key_expansion_targets_the_live_reasoning_segment_while_it_streams() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    let row = state.live_reasoning_row();
    state.update(Action::ExpandReasoning { row: None });
    assert_eq!(
        state.reasoning_expansion(row),
        0,
        "a step that streamed no reasoning has nothing to expand"
    );

    state.update(Action::FrameReceived(reasoning_delta(
        session_id,
        run_id,
        0,
        "a thought",
    )));
    state.update(Action::ExpandReasoning { row: None });
    assert_eq!(
        state.reasoning_expansion(state.live_reasoning_row()),
        100,
        "the live segment is the newest block that carries reasoning"
    );
    assert_eq!(
        state.live_reasoning_row(),
        row,
        "the committed row the live segment becomes is the row it was anchored at"
    );
}

#[test]
fn the_stream_mirror_reuses_snapshot_rows_without_repeating_them() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let rows = vec![
        user_row(session_id, Some(run_id), "hello"),
        assistant_row(session_id, run_id, "hi"),
    ];
    let mut state = opened_session(session_id, rows.clone());
    state.update(Action::RunStreamOpened(stream_state(
        session_id,
        run_id,
        RunStatusDto::Running,
        rows,
    )));
    assert_eq!(
        state.transcript().len(),
        2,
        "rows the transcript already carries are not appended twice"
    );

    let mut state = opened_session(session_id, Vec::new());
    state.update(Action::RunStreamOpened(stream_state(
        session_id,
        run_id,
        RunStatusDto::Running,
        vec![
            user_row(session_id, Some(run_id), "hello"),
            assistant_row(session_id, run_id, "hi"),
        ],
    )));
    assert_eq!(
        state.transcript().len(),
        2,
        "rows committed after the session read arrive with the subscription"
    );
    assert_eq!(state.stream(), StreamStatus::Live);
}

#[test]
fn a_content_frame_reports_every_row_appended_since_an_epoch() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    let epoch = state.transcript_epoch();
    assert_eq!(
        state.transcript_appended_since(epoch),
        Some(0),
        "a transcript that changed nothing appended nothing"
    );

    state.update(Action::FrameReceived(RunStreamFrameDto::Content(
        assistant_row(session_id, run_id, "first"),
    )));
    assert!(
        state.transcript_epoch() > epoch,
        "a committed row moves the epoch"
    );
    assert_eq!(state.transcript_appended_since(epoch), Some(1));
    state.update(Action::FrameReceived(RunStreamFrameDto::Content(
        assistant_row(session_id, run_id, "second"),
    )));
    assert_eq!(state.transcript_appended_since(epoch), Some(2));
    assert_eq!(
        state.transcript_appended_since(state.transcript_epoch()),
        Some(0),
        "the newest epoch has nothing appended after it"
    );
}

#[test]
fn the_stream_snapshot_appends_only_the_rows_the_session_read_missed() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let seeded = vec![user_row(session_id, Some(run_id), "hello")];
    let mut state = opened_session(session_id, seeded.clone());
    let epoch = state.transcript_epoch();
    let mut rows = seeded;
    rows.push(assistant_row(session_id, run_id, "hi"));

    state.update(Action::RunStreamOpened(stream_state(
        session_id,
        run_id,
        RunStatusDto::Running,
        rows,
    )));
    assert_eq!(state.transcript().len(), 2);
    assert_eq!(
        state.transcript_appended_since(epoch),
        Some(1),
        "only the row the session read missed is appended"
    );
}

#[test]
fn a_session_snapshot_reports_no_append_since_the_previous_epoch() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, vec![user_row(session_id, None, "hello")]);
    let epoch = state.transcript_epoch();

    state.update(Action::SessionSnapshotLoaded(snapshot(
        session_id,
        None,
        vec![
            user_row(session_id, None, "hello"),
            user_row(session_id, None, "again"),
        ],
    )));
    assert_eq!(
        state.transcript_appended_since(epoch),
        None,
        "a snapshot replaces the transcript instead of appending to it"
    );
    assert_eq!(
        state.transcript_appended_since(state.transcript_epoch()),
        Some(0)
    );
}

#[test]
fn a_trim_of_the_oldest_rows_reports_no_append_since_the_previous_epoch() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let rows: Vec<_> = (0..RETAINED_TRANSCRIPT_MESSAGES)
        .map(|index| assistant_row(session_id, run_id, &format!("row {index}")))
        .collect();
    let mut state = opened_session(session_id, rows.clone());
    state.update(Action::RunStreamOpened(stream_state(
        session_id,
        run_id,
        RunStatusDto::Running,
        rows,
    )));
    let epoch = state.transcript_epoch();

    state.update(Action::FrameReceived(RunStreamFrameDto::Content(
        assistant_row(session_id, run_id, "the newest row"),
    )));
    assert_eq!(state.transcript().len(), RETAINED_TRANSCRIPT_MESSAGES);
    assert_eq!(
        state.transcript_appended_since(epoch),
        None,
        "a front drain is a replacement, never an append"
    );
}

#[test]
fn a_closed_stream_leaves_the_transcript_epoch_where_it_was() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    state.update(Action::FrameReceived(RunStreamFrameDto::Content(
        assistant_row(session_id, run_id, "hi"),
    )));
    let epoch = state.transcript_epoch();

    state.update(Action::RunStreamEnded);
    assert_eq!(
        state.transcript_epoch(),
        epoch,
        "closing a stream never touches the transcript"
    );
    assert_eq!(state.transcript_appended_since(epoch), Some(0));
}

#[test]
fn a_frame_without_a_subscription_is_dropped() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = AppState::new(None);
    state.update(Action::FrameReceived(delta(session_id, run_id, 0, "stale")));
    assert_eq!(state.provisional_text(), "");
    assert_eq!(state.error(), None);
}

#[test]
fn a_frame_from_another_run_reports_a_scope_failure() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let other_run = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    state.update(Action::FrameReceived(delta(
        session_id, other_run, 0, "other",
    )));
    assert!(
        state.error().is_some(),
        "a frame outside the scope is never applied"
    );
    assert_eq!(state.provisional_text(), "");
}

#[test]
fn interrupt_requests_the_run_the_status_frame_reported() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    let effects = state.update(Action::InterruptRequested);
    assert_eq!(effects, vec![Effect::Interrupt { session_id, run_id }]);
    state.update(Action::InterruptAccepted);
    assert_eq!(state.notice(), Some("interrupt accepted"));
}

#[test]
fn interrupt_without_a_live_run_asks_for_nothing() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    let effects = state.update(Action::InterruptRequested);
    assert!(effects.is_empty());
    assert_eq!(state.notice(), Some("no active run to interrupt"));
}

#[test]
fn the_first_ctrl_c_press_arms_the_interrupt_and_the_second_interrupts_the_run() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    let effects = state.update(Action::CtrlCPressed);
    assert!(
        effects.is_empty(),
        "the first press only arms the interrupt"
    );
    let expected = format!(
        "press Ctrl+C again to interrupt run {}",
        super::short_identifier(run_id)
    );
    assert_eq!(state.notice(), Some(expected.as_str()));
    let effects = state.update(Action::CtrlCPressed);
    assert_eq!(effects, vec![Effect::Interrupt { session_id, run_id }]);
}

#[test]
fn a_ctrl_c_press_moves_a_typed_line_into_the_history_and_clears_it() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    type_text(&mut state, "abandoned");
    state.update(Action::MoveInputCursor(InputCursorMove::Left));
    let effects = state.update(Action::CtrlCPressed);
    assert!(
        effects.is_empty(),
        "abandoning a line asks for no client work"
    );
    assert!(!state.should_quit(), "a cleared line never starts the exit");
    assert_eq!(state.input(), "");
    assert_eq!(state.cursor(), 0);
    assert_eq!(state.notice(), None, "a cleared line shows no notice");
    state.update(Action::NavigateInputHistory(InputHistoryMove::Previous));
    assert_eq!(state.input(), "abandoned");
    assert_eq!(state.cursor(), "abandoned".chars().count());
}

#[test]
fn the_first_ctrl_c_press_with_an_empty_line_arms_the_exit_and_the_second_quits() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    let effects = state.update(Action::CtrlCPressed);
    assert!(effects.is_empty());
    assert!(!state.should_quit(), "the first press only arms the exit");
    assert_eq!(state.notice(), Some("press Ctrl+C again to exit"));
    let effects = state.update(Action::CtrlCPressed);
    assert!(
        effects.is_empty(),
        "leaving the front end asks for no client work"
    );
    assert!(state.should_quit());
}

#[test]
fn any_other_user_action_disarms_the_ctrl_c_press() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    state.update(Action::CtrlCPressed);
    state.update(Action::ScrollTranscript(TranscriptScroll::Older));
    let effects = state.update(Action::CtrlCPressed);
    assert!(
        effects.is_empty(),
        "the wheel cancelled the armed interrupt and re-armed it"
    );
    let expected = format!(
        "press Ctrl+C again to interrupt run {}",
        super::short_identifier(run_id)
    );
    assert_eq!(state.notice(), Some(expected.as_str()));

    let mut state = opened_session(session_id, Vec::new());
    state.update(Action::CtrlCPressed);
    type_text(&mut state, "x");
    state.update(Action::InputBackspace);
    state.update(Action::CtrlCPressed);
    assert!(
        !state.should_quit(),
        "the typed line cancelled the armed exit"
    );
    assert_eq!(state.notice(), Some("press Ctrl+C again to exit"));
}

#[test]
fn the_runs_own_reports_leave_the_armed_interrupt_standing() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    state.update(Action::CtrlCPressed);
    state.update(Action::FrameReceived(delta(session_id, run_id, 0, "tok")));
    state.update(Action::ElapsedReported { millis: 200 });
    let effects = state.update(Action::CtrlCPressed);
    assert_eq!(effects, vec![Effect::Interrupt { session_id, run_id }]);
}

#[test]
fn a_run_that_ended_between_two_presses_arms_the_exit_instead_of_quitting() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    state.update(Action::CtrlCPressed);
    state.update(Action::FrameReceived(status_frame(
        session_id,
        run_id,
        RunStatusDto::Completed,
    )));
    let effects = state.update(Action::CtrlCPressed);
    assert!(
        effects.is_empty(),
        "the interrupt arm did not outlive its run"
    );
    assert!(
        !state.should_quit(),
        "an interrupt arm never becomes the exit arm"
    );
    assert_eq!(state.notice(), Some("press Ctrl+C again to exit"));
}

#[test]
fn ctrl_q_stays_the_immediate_exit_while_the_ctrl_c_exit_is_armed() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    state.update(Action::CtrlCPressed);
    assert!(
        !state.should_quit(),
        "the Ctrl+C arm waits for its second press"
    );
    let effects = state.update(Action::Quit);
    assert!(effects.is_empty());
    assert!(state.should_quit(), "Ctrl+Q needs no second press");
}

#[test]
fn a_live_run_is_cancelled_by_one_esc_press_with_no_arming() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    let effects = state.update(Action::EscapePressed);
    assert_eq!(
        effects,
        vec![Effect::Interrupt { session_id, run_id }],
        "one press cancels the live run"
    );
    assert!(!state.should_quit(), "a cancel never exits");
}

#[test]
fn one_esc_press_arms_the_clear_and_the_second_clears_the_line() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    type_text(&mut state, "typed");
    let effects = state.update(Action::EscapePressed);
    assert!(effects.is_empty(), "one press only arms the clear");
    assert_eq!(state.notice(), Some("press Esc again to clear the input"));
    assert_eq!(state.input(), "typed", "a single Esc never clears");
    assert!(!state.should_quit(), "a typed line is not an exit");
    let effects = state.update(Action::EscapePressed);
    assert!(effects.is_empty(), "the clear asks for no client work");
    assert_eq!(state.input(), "");
    assert_eq!(state.cursor(), 0);
    assert_eq!(state.notice(), None, "the clear shows no notice");
    state.update(Action::NavigateInputHistory(InputHistoryMove::Previous));
    assert_eq!(state.input(), "typed", "the cleared line stays recallable");
}

#[test]
fn esc_exits_an_idle_front_end_with_an_empty_line() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    let effects = state.update(Action::EscapePressed);
    assert!(effects.is_empty());
    assert!(
        state.should_quit(),
        "an empty line leaves on a single press"
    );
}

#[test]
fn any_other_user_action_disarms_the_esc_clear() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    type_text(&mut state, "typed");
    state.update(Action::EscapePressed);
    state.update(Action::ScrollTranscript(TranscriptScroll::Older));
    let effects = state.update(Action::EscapePressed);
    assert!(
        effects.is_empty(),
        "the scroll disarmed the clear and re-armed it"
    );
    assert_eq!(
        state.input(),
        "typed",
        "the clear still needs its second press"
    );
    assert_eq!(state.notice(), Some("press Esc again to clear the input"));
}

#[test]
fn the_two_layered_keys_do_not_share_an_arm() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    type_text(&mut state, "typed");
    state.update(Action::EscapePressed);
    // Ctrl+C is another user action: it disarms the Esc clear and clears the
    // line itself, exactly as one press of it always does.
    state.update(Action::CtrlCPressed);
    assert_eq!(state.input(), "");
    type_text(&mut state, "typed");
    let effects = state.update(Action::EscapePressed);
    assert!(
        effects.is_empty(),
        "the Esc clear did not survive the Ctrl+C press"
    );
    assert_eq!(state.notice(), Some("press Esc again to clear the input"));
}

#[test]
fn the_esc_cancel_disarms_the_ctrl_c_arm() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    state.update(Action::CtrlCPressed);
    let effects = state.update(Action::EscapePressed);
    assert_eq!(effects, vec![Effect::Interrupt { session_id, run_id }]);
    let effects = state.update(Action::CtrlCPressed);
    assert!(
        effects.is_empty(),
        "the interrupt arm did not survive the Esc press"
    );
    let expected = format!(
        "press Ctrl+C again to interrupt run {}",
        super::short_identifier(run_id)
    );
    assert_eq!(state.notice(), Some(expected.as_str()));
}

#[test]
fn a_terminal_status_frame_reports_the_final_status() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    state.update(Action::FrameReceived(status_frame(
        session_id,
        run_id,
        RunStatusDto::Completed,
    )));
    assert_eq!(state.run_status(), Some(RunStatusDto::Completed));
    assert_eq!(state.notice(), Some("run completed"));
    let effects = state.update(Action::InterruptRequested);
    assert!(effects.is_empty(), "a completed run cannot be interrupted");
}

#[test]
fn the_session_mode_comes_from_the_snapshot_projection() {
    let session_id = SessionId::new();
    let mut state = AppState::new(None);
    assert_eq!(
        state.session_mode(),
        None,
        "no snapshot has named a mode yet"
    );
    state.update(Action::SessionSnapshotLoaded(snapshot_in_mode(
        session_id,
        RunModeDto::Plan,
        Vec::new(),
    )));
    assert_eq!(
        state.session_mode(),
        Some(RunModeDto::Plan),
        "the mode badge reads the session's own projection"
    );
}

#[test]
fn an_elapsed_report_is_stored_until_the_next_turn_starts() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    assert_eq!(state.elapsed_millis(), None);
    let effects = state.update(Action::ElapsedReported { millis: 3_200 });
    assert!(effects.is_empty(), "a measurement asks for no client work");
    assert_eq!(state.elapsed_millis(), Some(3_200));
    type_text(&mut state, "next");
    state.update(Action::InputSubmitted);
    assert_eq!(
        state.elapsed_millis(),
        None,
        "a new turn starts a new measurement"
    );
}

#[test]
fn opening_a_session_forgets_the_previous_turns_elapsed_time() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    state.update(Action::ElapsedReported { millis: 13_000 });
    state.update(Action::SessionSnapshotLoaded(snapshot(
        session_id,
        None,
        Vec::new(),
    )));
    assert_eq!(
        state.elapsed_millis(),
        None,
        "the measurement belongs to the turn the previous session ran"
    );
}

#[test]
fn a_closed_stream_drops_the_provisional_tail() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    state.update(Action::FrameReceived(delta(session_id, run_id, 0, "half")));
    state.update(Action::RunStreamEnded);
    assert_eq!(state.stream(), StreamStatus::Ended);
    assert_eq!(
        state.provisional_text(),
        "",
        "transient text does not outlive its stream"
    );
    assert_eq!(state.notice(), Some("live updates ended"));
}

#[test]
fn a_failed_stream_reports_the_failure() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    state.update(Action::RunStreamFailed(failure()));
    assert_eq!(state.stream(), StreamStatus::Failed);
    assert_eq!(state.provisional_text(), "");
    assert!(state.error().is_some());
}

#[test]
fn reconnect_clears_transient_state_and_re_reads_the_session() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    state.update(Action::FrameReceived(delta(session_id, run_id, 0, "half")));
    let effects = state.update(Action::ReconnectRequested);
    assert_eq!(effects, vec![Effect::Connect]);
    assert_eq!(state.connection(), ConnectionStatus::Connecting);
    assert_eq!(state.stream(), StreamStatus::Inactive);
    assert_eq!(
        state.provisional_text(),
        "",
        "a reconnect never replays transient deltas"
    );

    let effects = state.update(Action::Bootstrapped(DaemonHealthDto::ready()));
    assert_eq!(
        effects,
        vec![Effect::ListSessions, Effect::OpenSession(session_id)],
        "a reconnect re-reads the session it was showing"
    );
}

#[test]
fn creating_a_session_opens_it() {
    let session_id = SessionId::new();
    let mut state = AppState::new(None);
    let effects = state.update(Action::NewSessionRequested);
    assert_eq!(effects, vec![Effect::CreateSession]);
    let effects = state.update(Action::SessionCreated(session_id));
    assert_eq!(effects, vec![Effect::OpenSession(session_id)]);
    assert!(state.error().is_none());
}

#[test]
fn the_wheel_step_moves_the_transcript_window_and_stops_at_its_newest_rows() {
    let mut state = AppState::new(None);
    state.update(Action::ScrollTranscript(TranscriptScroll::Older));
    state.update(Action::ScrollTranscript(TranscriptScroll::Older));
    assert_eq!(state.scroll(), 6, "two notches move six display rows");
    state.update(Action::ScrollTranscript(TranscriptScroll::Newer));
    assert_eq!(state.scroll(), 3);
    state.update(Action::ScrollTranscript(TranscriptScroll::Newer));
    state.update(Action::ScrollTranscript(TranscriptScroll::Newer));
    assert_eq!(
        state.scroll(),
        0,
        "the live tail is the floor of the scroll window"
    );
}

#[test]
fn a_snapshot_resets_the_scroll_window() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    state.update(Action::ScrollTranscript(TranscriptScroll::Older));
    assert_eq!(state.scroll(), 3);
    state.update(Action::SessionSnapshotLoaded(snapshot(
        session_id,
        None,
        Vec::new(),
    )));
    assert_eq!(state.scroll(), 0);
}

#[test]
fn quit_stops_the_front_end_without_client_work() {
    let mut state = AppState::new(None);
    let effects = state.update(Action::Quit);
    assert!(effects.is_empty());
    assert!(state.should_quit());
}

#[test]
fn a_reported_selection_is_the_display_row_range_the_view_hit_tested() {
    let mut state = opened_session(SessionId::new(), Vec::new());
    let effects = state.update(Action::SelectTranscriptRows {
        anchor: 7,
        extent: 3,
    });
    assert!(effects.is_empty(), "a selection asks for no client work");
    let selection = state.transcript_selection().expect("the range is kept");
    assert_eq!(
        selection.bounds(),
        (3, 7),
        "the range reads oldest first, whichever end the drag started at"
    );
    for row in 3..=7 {
        assert!(selection.contains(row), "row {row} is inside the selection");
    }
    assert!(!selection.contains(2) && !selection.contains(8));
}

#[test]
fn a_selection_and_an_expansion_clear_when_the_transcript_is_replaced() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = opened_session(
        session_id,
        vec![reasoning_row(session_id, run_id, "the answer", "thought")],
    );
    state.update(Action::SelectTranscriptRows {
        anchor: 1,
        extent: 2,
    });
    assert!(state.transcript_selection().is_some());
    state.update(Action::ExpandReasoning { row: Some(0) });
    assert!(
        state.transcript_selection().is_none(),
        "an expansion inserts rows under the selection, so it is dropped"
    );
    state.update(Action::SelectTranscriptRows {
        anchor: 1,
        extent: 1,
    });
    state.update(Action::SessionSnapshotLoaded(snapshot(
        session_id,
        None,
        vec![assistant_row(session_id, run_id, "another answer")],
    )));
    assert!(
        state.transcript_selection().is_none(),
        "a replacement moves every display row, so the selection is dropped"
    );
    assert_eq!(
        state.reasoning_expansion(0),
        0,
        "a replacement drops every expansion with it"
    );
}

#[test]
fn each_expansion_activation_reveals_one_more_chunk_of_display_rows() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = opened_session(
        session_id,
        vec![reasoning_row(session_id, run_id, "the answer", "thought")],
    );
    let epoch = state.reasoning_epoch();
    state.update(Action::ExpandReasoning { row: Some(0) });
    assert_eq!(
        state.reasoning_expansion(0),
        100,
        "one chunk per activation"
    );
    assert_eq!(
        state.reasoning_epoch(),
        epoch + 1,
        "the layout cache keys on the expansion epoch"
    );
    state.update(Action::ExpandReasoning { row: Some(0) });
    assert_eq!(state.reasoning_expansion(0), 200);
    state.update(Action::ExpandReasoning { row: Some(9) });
    assert_eq!(
        state.reasoning_expansion(0),
        200,
        "a row that carries no reasoning has nothing to expand"
    );
}

#[test]
fn the_key_expansion_targets_the_newest_row_that_carries_reasoning() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = opened_session(
        session_id,
        vec![
            reasoning_row(session_id, run_id, "first answer", "first thought"),
            assistant_row(session_id, run_id, "plain answer"),
            reasoning_row(session_id, run_id, "second answer", "second thought"),
        ],
    );
    state.update(Action::ExpandReasoning { row: None });
    assert_eq!(state.reasoning_expansion(2), 100);
    assert_eq!(
        state.reasoning_expansion(0),
        0,
        "the key expands the newest block, not the oldest"
    );
}

#[test]
fn an_expansion_clears_when_the_transcript_is_front_trimmed() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let rows: Vec<_> = (0..RETAINED_TRANSCRIPT_MESSAGES)
        .map(|index| reasoning_row(session_id, run_id, &format!("row {index}"), "thought"))
        .collect();
    let mut state = opened_session(session_id, rows.clone());
    state.update(Action::SelectTranscriptRows {
        anchor: 2,
        extent: 4,
    });
    state.update(Action::ExpandReasoning { row: Some(1) });
    assert_eq!(state.reasoning_expansion(1), 100);
    state.update(Action::RunStreamOpened(stream_state(
        session_id,
        run_id,
        RunStatusDto::Running,
        rows,
    )));
    state.update(Action::FrameReceived(RunStreamFrameDto::Content(
        assistant_row(session_id, run_id, "the newest row"),
    )));
    assert_eq!(
        state.transcript().len(),
        RETAINED_TRANSCRIPT_MESSAGES,
        "the trim dropped the oldest row"
    );
    assert_eq!(
        state.reasoning_expansion(1),
        0,
        "a front trim moves every row, so every expansion is dropped"
    );
    assert!(state.transcript_selection().is_none());
}

#[test]
fn a_fast_drag_step_moves_five_wheel_notches() {
    let mut state = AppState::new(None);
    state.update(Action::ScrollTranscriptFast(TranscriptScroll::Older));
    assert_eq!(state.scroll(), TRANSCRIPT_DRAG_ROWS);
    assert_eq!(
        state.scroll(),
        15,
        "the fast step is five wheel notches of three rows"
    );
    state.update(Action::ScrollTranscript(TranscriptScroll::Older));
    assert_eq!(state.scroll(), 18, "a plain notch keeps the normal step");
    state.update(Action::ScrollTranscriptFast(TranscriptScroll::Newer));
    assert_eq!(state.scroll(), 3);
    state.update(Action::ScrollTranscriptFast(TranscriptScroll::Newer));
    assert_eq!(state.scroll(), 0);
}
