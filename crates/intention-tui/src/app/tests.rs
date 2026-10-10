#![allow(
    clippy::expect_used,
    reason = "Unit tests build typed fixture DTOs directly and assert them for diagnostics."
)]

use std::sync::atomic::{AtomicI64, Ordering};

use intention_client::{RETAINED_TRANSCRIPT_MESSAGES, RunStreamState};
use intention_proto::{
    ConfigRevisionId, DaemonHealthDto, ErrorDto, MessageId, MessageKindDto, MessageProjectionDto,
    ProjectId, RunId, RunModeDto, RunProjectionDto, RunStatusDto, RunStreamFrameDto,
    RunSubscriptionSnapshotDto, SendUserTurnOutcomeDto, SessionId, SessionProjectionDto,
    SessionSnapshotDto, SessionSummariesDto, SessionSummaryDto, TextDeltaChannelDto,
    TextDeltaFrameDto, ThemeDto, TimestampDto, ToolCallId, TurnId, WorkspaceId, WorkspaceRootDto,
};

use super::{
    Action, AppState, BrowserCursorMove, BrowserTab, COMMANDS, ConnectionStatus, Effect,
    InputCursorMove, InputHistoryMove, MenuMove, RunPhase, Screen, StreamStatus,
    TRANSCRIPT_DRAG_ROWS, Theme, TranscriptScroll,
};

/// The canonical id of the first fixture session.
const FIRST_SESSION: &str = "11111111-1111-4111-8111-111111111111";

/// The canonical id of the second fixture session.
const SECOND_SESSION: &str = "22222222-2222-4222-8222-222222222222";

/// Returns the fixture session id spelled by one canonical UUID literal.
fn fixture_session(value: &str) -> SessionId {
    SessionId::parse(value).expect("the fixture session id is canonical")
}

/// Returns one fresh durable row identity for a committed fixture row.
fn row_id() -> MessageId {
    static NEXT: AtomicI64 = AtomicI64::new(0);
    MessageId::new(NEXT.fetch_add(1, Ordering::Relaxed) + 1)
        .expect("the fixture row identity is positive")
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
    summary_in_root(session_id, updated_at, workspace_root())
}

/// Returns one session summary durably bound to `root`.
fn summary_in_root(
    session_id: SessionId,
    updated_at: i64,
    root: WorkspaceRootDto,
) -> SessionSummaryDto {
    let updated_at = TimestampDto::from_unix_seconds(updated_at).expect("fixture time is valid");
    SessionSummaryDto::new(
        session_id,
        ProjectId::new(),
        WorkspaceId::new(),
        RunModeDto::Build,
        updated_at,
        updated_at,
        root,
        0,
        None,
    )
}

/// Returns one fixture workspace root distinct from [`workspace_root`].
fn other_workspace_root() -> WorkspaceRootDto {
    WorkspaceRootDto::parse(
        std::env::temp_dir()
            .join("intention-fixture-other-folder")
            .to_string_lossy()
            .into_owned(),
    )
    .expect("the fixture workspace root is an absolute native path")
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
        row_id(),
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
        row_id(),
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
        row_id(),
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

/// Returns one committed tool-call row carrying `arguments` for `call_id`.
fn tool_call_row(
    session_id: SessionId,
    run_id: RunId,
    tool_id: &str,
    arguments: &str,
    call_id: ToolCallId,
) -> MessageProjectionDto {
    MessageProjectionDto::new(
        row_id(),
        session_id,
        Some(run_id),
        MessageKindDto::ToolCall,
        arguments,
        None,
        Some(call_id),
        Some(tool_id.to_owned()),
    )
    .expect("the fixture tool-call row is coherent")
}

/// Returns one committed tool-result row carrying `content` for `call_id`.
fn tool_result_row(
    session_id: SessionId,
    run_id: RunId,
    tool_id: &str,
    content: &str,
    call_id: ToolCallId,
) -> MessageProjectionDto {
    MessageProjectionDto::new(
        row_id(),
        session_id,
        Some(run_id),
        MessageKindDto::ToolResult,
        content,
        None,
        Some(call_id),
        Some(tool_id.to_owned()),
    )
    .expect("the fixture tool-result row is coherent")
}

/// Applies one committed transcript row to `state` as a stream content frame.
fn commit_row(state: &mut AppState, row: MessageProjectionDto) {
    let effects = state.update(Action::FrameReceived(RunStreamFrameDto::Content(row)));
    assert!(
        effects.is_empty(),
        "a committed row asks for no client work"
    );
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

/// Runs one typed slash command the way the keyboard does.
///
/// The first Enter completes the open hint menu into `/<name> `, the second
/// submits the completed line: a line that already spells a complete command
/// needs both presses, and the completing press runs nothing.
fn run_command(state: &mut AppState) -> Vec<Effect> {
    let completing = state.update(Action::InputSubmitted);
    assert!(
        completing.is_empty(),
        "the press that completes the command runs nothing"
    );
    state.update(Action::InputSubmitted)
}

/// Opens the theme picker the way `/theme` does.
fn open_picker(state: &mut AppState) {
    type_text(state, "/theme");
    let effects = run_command(state);
    assert!(
        effects.is_empty(),
        "opening the picker asks for no client work"
    );
    assert_eq!(state.screen(), Screen::Theme);
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

/// Returns a running session whose answer stream started and one tool call just
/// committed at the reported elapsed value `at`, with the fixture identities.
///
/// The commit's elapsed value is the baseline the working threshold measures
/// from: reporting `at` before the row is what pins the baseline a test then
/// advances past or stays under.
fn tool_call_in_flight(at: u64) -> (AppState, SessionId, RunId, ToolCallId) {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let call_id = ToolCallId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    state.update(Action::FrameReceived(delta(
        session_id,
        run_id,
        0,
        "the answer",
    )));
    state.update(Action::ElapsedReported { millis: at });
    commit_row(
        &mut state,
        tool_call_row(
            session_id,
            run_id,
            "read",
            r#"{"path":"src/lib.rs"}"#,
            call_id,
        ),
    );
    (state, session_id, run_id, call_id)
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
        vec![
            Effect::ListSessions,
            Effect::LoadSettings,
            Effect::OpenSession(session_id)
        ],
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
fn the_settings_load_records_the_daemons_theme_on_startup() {
    let mut state = AppState::new(None);
    let effects = state.update(Action::Bootstrapped(DaemonHealthDto::ready()));
    assert_eq!(
        effects,
        vec![Effect::ListSessions, Effect::LoadSettings],
        "a ready connection reads the daemon's terminal settings"
    );
    assert_eq!(
        state.theme(),
        Theme::Light,
        "the terminal's own default holds until the daemon answers"
    );

    let effects = state.update(Action::SettingsReceived(ThemeDto::Dark));
    assert!(
        effects.is_empty(),
        "the settings reply asks for no more work"
    );
    assert_eq!(
        state.theme(),
        Theme::Dark,
        "the daemon's theme becomes the committed one"
    );
    assert_eq!(state.effective_theme(), Theme::Dark);
}

#[test]
fn only_the_accepted_reply_moves_the_committed_theme_and_a_rejection_notices() {
    let mut state = AppState::new(None);
    let effects = state.update(Action::ThemeSelected(Theme::Dark));
    assert_eq!(
        effects,
        vec![Effect::PersistTheme(Theme::Dark)],
        "a selection asks the daemon to persist it"
    );
    assert_eq!(
        state.theme(),
        Theme::Light,
        "a selection never commits optimistically"
    );
    assert_eq!(
        state.effective_theme(),
        Theme::Dark,
        "the candidate previews while the daemon confirms"
    );

    let effects = state.update(Action::SettingsFailed(failure()));
    assert!(effects.is_empty());
    assert_eq!(
        state.theme(),
        Theme::Light,
        "a rejected selection leaves the committed theme"
    );
    assert_eq!(
        state.effective_theme(),
        Theme::Light,
        "a rejected selection drops the preview it showed"
    );
    assert!(
        state
            .error()
            .is_some_and(|error| error.starts_with("fixture_failure:")),
        "the rejection reaches the error line"
    );

    // The accepted reply is the only thing that moves the committed theme.
    let mut state = AppState::new(None);
    state.update(Action::ThemeSelected(Theme::Dark));
    state.update(Action::SettingsReceived(ThemeDto::Dark));
    assert_eq!(state.theme(), Theme::Dark);
    assert_eq!(state.effective_theme(), Theme::Dark);
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
        vec![
            Effect::ListSessions,
            Effect::LoadSettings,
            Effect::OpenSession(selected)
        ],
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
    let effects = run_command(&mut state);
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
        Some("unknown command /watch; known commands: /new /sessions /theme")
    );
}

#[test]
fn a_slash_command_never_enters_the_input_history() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    type_text(&mut state, "/new");
    run_command(&mut state);
    state.update(Action::NavigateInputHistory(InputHistoryMove::Previous));
    assert_eq!(state.input(), "", "a command line is never recallable");
}

#[test]
fn the_hint_menu_opens_on_a_leading_slash_and_stays_closed_otherwise() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    assert!(
        state.command_menu().is_none(),
        "an empty line opens no menu"
    );

    type_text(&mut state, "read /new");
    assert!(
        state.command_menu().is_none(),
        "a slash that is not the first character opens no menu"
    );
    let effects = state.update(Action::InputSubmitted);
    assert_eq!(
        effects,
        vec![Effect::SendTurn {
            session_id,
            content: "read /new".to_owned(),
        }],
        "the mid-text slash reaches the session unchanged"
    );

    // A blank before the slash leaves the line a turn too: the rule is the
    // line's first character, not its first non-blank one, and submission and
    // the menu answer to the same rule.
    let mut state = opened_session(session_id, Vec::new());
    type_text(&mut state, " /new");
    assert!(
        state.command_menu().is_none(),
        "a slash the first character does not start is no command word"
    );
    let effects = state.update(Action::InputSubmitted);
    assert_eq!(
        effects,
        vec![Effect::SendTurn {
            session_id,
            content: "/new".to_owned(),
        }],
        "the blank-led line is a turn whose content is trimmed"
    );

    type_text(&mut state, "/");
    let menu = state
        .command_menu()
        .expect("a leading slash opens the menu");
    assert_eq!(menu.len(), 3, "a bare slash offers every command");
    assert_eq!(menu.highlight(), 0, "the best match starts highlighted");
    assert_eq!(
        menu.rows().map(|row| row.label()).collect::<Vec<_>>(),
        vec![
            "/new".to_owned(),
            "/sessions".to_owned(),
            "/theme".to_owned()
        ],
        "a bare slash lists the registry in registry order"
    );
}

#[test]
fn the_hint_menu_filters_as_the_word_grows() {
    let mut state = AppState::new(None);
    type_text(&mut state, "/s");
    let menu = state
        .command_menu()
        .expect("a prefix match keeps the menu open");
    assert_eq!(
        menu.rows().map(|row| row.label()).collect::<Vec<_>>(),
        vec!["/sessions".to_owned()],
        "s narrows the list to the command that starts with it"
    );

    type_text(&mut state, "e");
    assert_eq!(
        state.command_menu().map(|menu| menu.len()),
        Some(1),
        "the list keeps narrowing as the word grows"
    );

    let mut state = AppState::new(None);
    type_text(&mut state, "/sns");
    assert_eq!(
        state
            .command_menu()
            .expect("an ordered subsequence matches")
            .rows()
            .map(|row| row.label())
            .collect::<Vec<_>>(),
        vec!["/sessions".to_owned()],
        "sns is an ordered subsequence of sessions"
    );

    let mut state = AppState::new(None);
    type_text(&mut state, "/zz");
    assert!(
        state.command_menu().is_none(),
        "a filter no command matches closes the menu"
    );
}

#[test]
fn the_hint_menu_highlight_moves_with_the_arrows_while_the_history_waits() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    type_text(&mut state, "an earlier prompt");
    state.update(Action::InputSubmitted);

    type_text(&mut state, "/");
    assert_eq!(state.command_menu().map(|menu| menu.highlight()), Some(0));
    state.update(Action::MenuMove(MenuMove::Down));
    assert_eq!(
        state.command_menu().map(|menu| menu.highlight()),
        Some(1),
        "Down moves the highlight through the open menu"
    );
    assert_eq!(
        state.input(),
        "/",
        "the menu's arrows never walk the input history"
    );
    state.update(Action::MenuMove(MenuMove::Down));
    assert_eq!(
        state.command_menu().map(|menu| menu.highlight()),
        Some(2),
        "Down keeps walking the three registered commands"
    );
    state.update(Action::MenuMove(MenuMove::Down));
    assert_eq!(
        state.command_menu().map(|menu| menu.highlight()),
        Some(2),
        "the highlight stops at the list's last row"
    );
    state.update(Action::MenuMove(MenuMove::Up));
    assert_eq!(state.command_menu().map(|menu| menu.highlight()), Some(1));
    state.update(Action::MenuMove(MenuMove::Up));
    assert_eq!(state.command_menu().map(|menu| menu.highlight()), Some(0));

    state.update(Action::MenuMove(MenuMove::Up));
    assert_eq!(
        state.command_menu().map(|menu| menu.highlight()),
        Some(0),
        "the highlight stops at the list's first row"
    );

    // With the menu closed the same arrows are the history walk again.
    state.update(Action::EscapePressed);
    assert!(state.command_menu().is_none());
    state.update(Action::NavigateInputHistory(InputHistoryMove::Previous));
    assert_eq!(state.input(), "an earlier prompt");
}

#[test]
fn the_hint_menu_commits_the_highlighted_command_with_a_trailing_space() {
    for (key, highlight) in [(Action::MenuAccept, 0), (Action::MenuAccept, 1)] {
        let mut state = AppState::new(None);
        type_text(&mut state, "/s");
        for _ in 0..highlight {
            state.update(Action::MenuMove(MenuMove::Down));
        }
        state.update(key);
        assert_eq!(
            state.input(),
            "/sessions ",
            "the committed command is spelled out with one trailing space"
        );
        assert_eq!(
            state.cursor(),
            "/sessions ".chars().count(),
            "the caret lands ready for the command's arguments"
        );
        assert!(
            state.command_menu().is_none(),
            "the committed word closes the menu"
        );
    }

    // Enter commits the same way while the menu is open, and the caret is then
    // ready for the second press that submits the completed line.
    let mut state = AppState::new(None);
    type_text(&mut state, "/ne");
    let completing = state.update(Action::InputSubmitted);
    assert!(completing.is_empty(), "the completing press runs nothing");
    assert_eq!(state.input(), "/new ");
    let submitting = state.update(Action::InputSubmitted);
    assert_eq!(
        submitting,
        vec![Effect::CreateSession],
        "the second press runs the completed command"
    );
}

#[test]
fn the_hint_menu_closes_on_every_close_rule() {
    // The slash removed.
    let mut state = AppState::new(None);
    type_text(&mut state, "/n");
    state.update(Action::InputBackspace);
    state.update(Action::InputBackspace);
    assert!(state.command_menu().is_none(), "no slash, no menu");

    // A space after a complete command name.
    let mut state = AppState::new(None);
    type_text(&mut state, "/new");
    state.update(Action::InputChar(' '));
    assert!(
        state.command_menu().is_none(),
        "the caret past the word closes the menu"
    );
    assert_eq!(state.input(), "/new ");

    // The filter leaving no match.
    let mut state = AppState::new(None);
    type_text(&mut state, "/zz");
    assert!(state.command_menu().is_none(), "no match, no menu");

    // Esc closes the menu and means nothing else: over a live run the press
    // cancels nothing, so the run's own cancel still needs its own press.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    type_text(&mut state, "/n");
    assert!(state.command_menu().is_some());
    let closing = state.update(Action::EscapePressed);
    assert!(closing.is_empty(), "closing the menu interrupts nothing");
    assert!(state.command_menu().is_none());
    assert_eq!(
        state.notice(),
        None,
        "closing the menu never arms the clear with a notice"
    );
    assert_eq!(
        state.run_status(),
        Some(RunStatusDto::Running),
        "closing the menu leaves the live run alone"
    );
    let cancelling = state.update(Action::EscapePressed);
    assert!(
        !cancelling.is_empty(),
        "the run's cancel is the next press's own meaning"
    );
    assert!(
        state
            .notice()
            .is_some_and(|notice| notice.starts_with("interrupting run")),
        "the second press cancels the run: {:?}",
        state.notice()
    );
    assert!(!state.should_quit(), "closing the menu never exits");

    // The menu's close is not the clear's first press either: over a
    // non-empty line and no run, the clear arm still needs its own press.
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    type_text(&mut state, "/n");
    state.update(Action::EscapePressed);
    assert!(state.command_menu().is_none());
    assert_eq!(state.notice(), None);
    assert!(!state.should_quit(), "closing the menu never exits");
    state.update(Action::EscapePressed);
    assert_eq!(
        state.notice(),
        Some("press Esc again to clear the input"),
        "the clear arm needs its own first press, so the menu's close was not one"
    );
}

#[test]
fn the_band_lists_a_declared_arguments_values_and_completes_one() {
    let mut state = AppState::new(None);
    type_text(&mut state, "/theme ");
    let menu = state
        .command_menu()
        .expect("a declared argument opens the band");
    assert_eq!(
        menu.rows().map(|row| row.label()).collect::<Vec<_>>(),
        vec!["light".to_owned(), "dark".to_owned()],
        "an empty filter lists every value in declaration order"
    );
    assert_eq!(menu.highlight(), 0, "the best match starts highlighted");

    // Tab completes the empty word: the band has nothing to replace, so the
    // value it highlights becomes the word and one trailing space follows. The
    // band has nothing to complete for Enter either - but Enter runs the
    // command instead, which is what opens the picker.
    state.update(Action::MenuAccept);
    assert_eq!(state.input(), "/theme light ");
    assert_eq!(state.cursor(), "/theme light ".chars().count());
    assert!(
        state.command_menu().is_none(),
        "a complete value closes the band"
    );
    assert_eq!(
        state.update(Action::InputSubmitted),
        vec![Effect::PersistTheme(Theme::Light)],
        "the completed line runs on the next Enter"
    );

    // A typed prefix narrows the band, and Enter completes it the same way.
    let mut state = AppState::new(None);
    type_text(&mut state, "/theme d");
    let menu = state.command_menu().expect("d selects dark");
    assert_eq!(
        menu.rows().map(|row| row.label()).collect::<Vec<_>>(),
        vec!["dark".to_owned()],
        "the value filter narrows the band"
    );
    assert_eq!(
        menu.rows().next().expect("the row is shown").trailing(),
        "theme",
        "the third cell names the argument the value fills"
    );

    // Enter completes the value and appends one trailing space; the completed
    // line is then what the next Enter runs.
    let completing = state.update(Action::InputSubmitted);
    assert!(completing.is_empty(), "the completing press runs nothing");
    assert_eq!(state.input(), "/theme dark ");
    assert_eq!(state.cursor(), "/theme dark ".chars().count());
    assert!(
        state.command_menu().is_none(),
        "a space after a complete value closes the band"
    );

    let submitting = state.update(Action::InputSubmitted);
    assert_eq!(submitting, vec![Effect::PersistTheme(Theme::Dark)]);
}

#[test]
fn the_band_close_rule_follows_the_commands_declared_arguments() {
    // A space after a command that declares no argument closes the band.
    let mut state = AppState::new(None);
    type_text(&mut state, "/new ");
    assert!(
        state.command_menu().is_none(),
        "/new declares no argument, so its blank closes the band"
    );

    // A space after a command that declares one reopens it on the argument
    // word: the caret is in a word the command has, even before any character
    // of it is typed.
    let mut state = AppState::new(None);
    type_text(&mut state, "/theme ");
    assert!(
        state.command_menu().is_some(),
        "/theme declares an argument, so its blank opens that word"
    );

    // A complete argument value closes the band once the blank after it is
    // typed, and a word past the declared argument closes it too.
    type_text(&mut state, "dark ");
    assert!(
        state.command_menu().is_none(),
        "the blank after a complete value closes the band"
    );
    type_text(&mut state, "now");
    assert!(
        state.command_menu().is_none(),
        "a word past the declared argument fills none"
    );

    // Removing the slash still closes it, exactly as it did before the command
    // grew a word model.
    let mut state = AppState::new(None);
    type_text(&mut state, "/theme ");
    for _ in 0.."/theme ".chars().count() {
        state.update(Action::InputBackspace);
    }
    assert!(state.input().is_empty());
    assert!(state.command_menu().is_none(), "no slash, no band");
}

#[test]
fn the_registry_answers_for_both_submission_and_the_menu() {
    let mut state = AppState::new(None);
    type_text(&mut state, "/");
    assert_eq!(
        state
            .command_menu()
            .expect("a bare slash offers the registry")
            .rows()
            .map(|row| row.label())
            .collect::<Vec<_>>(),
        COMMANDS
            .iter()
            .map(|command| command.typed_name())
            .collect::<Vec<_>>(),
        "the menu lists exactly the registry, in registry order"
    );

    for command in COMMANDS {
        let mut state = AppState::new(None);
        type_text(&mut state, &command.typed_name());
        assert!(
            state.command_menu().is_some(),
            "/{} is offered by the menu",
            command.name
        );
        let effects = run_command(&mut state);
        assert!(
            !effects.is_empty() || state.screen() != Screen::Chat,
            "/{} is dispatched by the submission path",
            command.name
        );
        assert!(
            state
                .notice()
                .is_none_or(|notice| !notice.starts_with("unknown command")),
            "/{} is known to the submission path",
            command.name
        );
    }

    let mut state = AppState::new(None);
    type_text(&mut state, "/nothing");
    state.update(Action::InputSubmitted);
    assert_eq!(
        state.notice(),
        Some("unknown command /nothing; known commands: /new /sessions /theme"),
        "the unknown-command notice answers from the registry"
    );
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
    let effects = run_command(&mut state);
    assert_eq!(
        effects,
        vec![Effect::ListSessions],
        "the browser refreshes the list it shows"
    );
    assert_eq!(state.screen(), Screen::Sessions);
    assert_eq!(
        state.browser_tab(),
        BrowserTab::All,
        "the browser opens on the whole list"
    );
    assert_eq!(state.browser_filter(), "");
    assert_eq!(state.input(), "", "a command never stays in the input line");
}

#[test]
fn the_theme_command_opens_the_picker_without_an_argument() {
    let mut state = AppState::new(None);
    type_text(&mut state, "/theme");
    let completing = state.update(Action::InputSubmitted);
    assert!(completing.is_empty(), "the completing press runs nothing");
    assert_eq!(state.input(), "/theme ");
    assert!(
        state.command_menu().is_some(),
        "the blank after a declared argument opens its word"
    );

    // The empty argument word has nothing to complete, so Enter runs the
    // command itself instead of writing the value its highlight points at.
    let effects = state.update(Action::InputSubmitted);
    assert!(
        effects.is_empty(),
        "the picker needs no client work to open"
    );
    assert_eq!(state.screen(), Screen::Theme);
    assert_eq!(state.input(), "", "a command never stays in the input line");
    assert_eq!(
        state.effective_theme(),
        Theme::Light,
        "the picker opens on the committed theme"
    );
}

#[test]
fn the_theme_command_applies_a_theme_name_however_it_is_spelled() {
    for (typed, theme) in [
        ("dark", Theme::Dark),
        ("DARK", Theme::Dark),
        ("Light", Theme::Light),
    ] {
        let mut state = AppState::new(None);
        type_text(&mut state, &format!("/theme {typed}"));
        let effects = run_command(&mut state);
        assert_eq!(
            effects,
            vec![Effect::PersistTheme(theme)],
            "/theme {typed} asks the daemon to persist {theme:?}"
        );
        assert_eq!(
            state.screen(),
            Screen::Chat,
            "a spelled-out theme never opens the picker"
        );
        assert_eq!(
            state.theme(),
            Theme::Light,
            "the committed theme waits for the accepted reply"
        );
        assert_eq!(
            state.effective_theme(),
            theme,
            "the selection previews while the daemon confirms"
        );
    }
}

#[test]
fn the_theme_command_notices_an_unknown_value_and_an_extra_word() {
    let mut state = AppState::new(None);
    type_text(&mut state, "/theme purple");
    let effects = state.update(Action::InputSubmitted);
    assert!(effects.is_empty(), "a bad value asks for no client work");
    assert_eq!(
        state.notice(),
        Some("unknown theme \"purple\" - expected light or dark")
    );
    assert_eq!(state.screen(), Screen::Chat, "a bad value opens no picker");

    let mut state = AppState::new(None);
    type_text(&mut state, "/theme dark now");
    let effects = state.update(Action::InputSubmitted);
    assert!(effects.is_empty());
    assert_eq!(state.notice(), Some("unexpected argument \"now\""));
    assert_eq!(
        state.theme(),
        Theme::Light,
        "a malformed line selects nothing"
    );
}

#[test]
fn the_picker_moves_the_highlight_and_previews_the_candidate_without_persisting() {
    let mut state = AppState::new(None);
    open_picker(&mut state);

    let effects = state.update(Action::ThemePreviewed(Theme::Dark));
    assert!(
        effects.is_empty(),
        "a preview asks the daemon for nothing: it is never persisted"
    );
    assert_eq!(
        state.effective_theme(),
        Theme::Dark,
        "the whole window repaints through the candidate"
    );
    assert_eq!(
        state.theme(),
        Theme::Light,
        "the committed theme stays what the daemon carries"
    );

    // The picker walks its two rows from the effective theme, so the last row
    // is where Down stops and the first is where Up stops.
    assert_eq!(
        state.effective_theme().next(),
        Theme::Dark,
        "Down from light reaches dark"
    );
    state.update(Action::ThemePreviewed(Theme::Light));
    assert_eq!(
        state.effective_theme(),
        Theme::Light,
        "Up walks back to the committed row"
    );
}

#[test]
fn the_picker_commits_the_highlighted_theme_through_the_daemon_and_closes() {
    let mut state = AppState::new(None);
    open_picker(&mut state);
    state.update(Action::ThemePreviewed(Theme::Dark));

    let effects = state.update(Action::ThemeSelected(Theme::Dark));
    assert_eq!(
        effects,
        vec![Effect::PersistTheme(Theme::Dark)],
        "Enter asks the daemon to persist the highlighted theme"
    );
    assert_eq!(
        state.screen(),
        Screen::Chat,
        "the picker closes on its commit"
    );
    assert_eq!(
        state.theme(),
        Theme::Light,
        "the committed theme moves only on the accepted reply"
    );
    assert_eq!(
        state.effective_theme(),
        Theme::Dark,
        "the candidate stays on screen while the daemon confirms"
    );

    let effects = state.update(Action::SettingsReceived(ThemeDto::Dark));
    assert!(effects.is_empty());
    assert_eq!(state.theme(), Theme::Dark);
    assert_eq!(state.effective_theme(), Theme::Dark);
}

#[test]
fn escaping_the_picker_restores_the_committed_theme() {
    let mut state = AppState::new(None);
    open_picker(&mut state);
    state.update(Action::ThemePreviewed(Theme::Dark));
    assert_eq!(state.effective_theme(), Theme::Dark);

    let effects = state.update(Action::ThemePreviewCleared);
    assert!(effects.is_empty(), "the revert asks for no client work");
    assert_eq!(
        state.effective_theme(),
        Theme::Light,
        "Esc restores the committed theme"
    );
    assert_eq!(state.theme(), Theme::Light);
    assert_eq!(state.screen(), Screen::Chat, "Esc closes the picker");

    // The picker's Esc is its own layer: it never cancels a run, arms a clear,
    // or exits, because the screen owns the key while it is open. A reopened
    // picker starts from the committed theme again.
    let mut state = AppState::new(None);
    open_picker(&mut state);
    state.update(Action::ThemePreviewed(Theme::Dark));
    state.update(Action::ThemePreviewCleared);
    open_picker(&mut state);
    assert_eq!(state.effective_theme(), Theme::Light);
    assert!(!state.should_quit());
    assert_eq!(state.notice(), None);
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
        summary(second, 40),
        summary(first, 30),
    ])));
    assert_eq!(
        browser_row_ids(&state),
        vec![second, first],
        "a fresh list is selected again, in the daemon's recency order"
    );
}

#[test]
fn the_current_folder_tab_selects_the_sessions_of_the_front_end_root() {
    let here = fixture_session(FIRST_SESSION);
    let elsewhere = fixture_session(SECOND_SESSION);
    let mut state = opened_session(here, Vec::new()).with_workspace_root(workspace_root());
    state.update(Action::SessionsListed(summaries(vec![
        summary_in_root(here, 30, workspace_root()),
        summary_in_root(elsewhere, 20, other_workspace_root()),
    ])));
    state.update(Action::SessionsBrowserRequested);
    assert_eq!(
        browser_row_ids(&state),
        vec![here, elsewhere],
        "`All` selects every listed session"
    );
    state.update(Action::BrowserTabPrevious);
    assert_eq!(state.browser_tab(), BrowserTab::CurrentFolder);
    assert_eq!(
        browser_row_ids(&state),
        vec![here],
        "`Current Folder` selects only the sessions bound to the front end's root"
    );
    state.update(Action::BrowserTabNext);
    assert_eq!(state.browser_tab(), BrowserTab::All);
    assert_eq!(
        browser_row_ids(&state),
        vec![here, elsewhere],
        "the whole list is one press away"
    );
}

#[test]
fn the_current_folder_tab_without_a_declared_root_selects_nothing() {
    let first = fixture_session(FIRST_SESSION);
    let mut state = opened_browser(first, vec![summary(first, 20)]);
    state.update(Action::BrowserTabPrevious);
    assert_eq!(state.browser_tab(), BrowserTab::CurrentFolder);
    assert!(
        state.browser_rows().is_empty(),
        "a state with no declared root matches no session's folder"
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
fn a_dispatched_turn_starts_the_waiting_phase() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    assert_eq!(
        state.run_phase(),
        None,
        "no turn request has left the state yet"
    );
    type_text(&mut state, "hello");
    let effects = state.update(Action::InputSubmitted);
    assert!(
        matches!(effects.as_slice(), [Effect::SendTurn { .. }]),
        "the submission dispatches one turn request"
    );
    assert_eq!(
        state.run_phase(),
        Some(RunPhase::Waiting),
        "the run the request starts waits for its first streamed token"
    );

    // The created session's own first turn leaves the same way: the snapshot
    // that opens the created session returns the pending prompt's `SendTurn`.
    let mut state = AppState::new(None);
    type_text(&mut state, "hello");
    state.update(Action::InputSubmitted);
    let created = SessionId::new();
    state.update(Action::SessionCreated(created));
    state.update(Action::SessionSnapshotLoaded(snapshot(
        created,
        None,
        Vec::new(),
    )));
    assert_eq!(
        state.run_phase(),
        Some(RunPhase::Waiting),
        "the prompt the snapshot sends starts its run waiting too"
    );
}

#[test]
fn a_turn_request_that_never_reached_a_run_leaves_no_phase() {
    let session_id = SessionId::new();
    let mut state = opened_session(session_id, Vec::new());
    type_text(&mut state, "hello");
    state.update(Action::InputSubmitted);
    assert_eq!(state.run_phase(), Some(RunPhase::Waiting));
    state.update(Action::TurnFailed(failure()));
    assert_eq!(
        state.run_phase(),
        None,
        "the waiting phase belonged to the run the failed request never started"
    );
    assert!(
        state.error().is_some(),
        "the failure reaches the error line"
    );
}

#[test]
fn the_reasoning_stream_starts_the_thinking_phase_and_the_answer_stream_the_answering_one() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    assert_eq!(
        state.run_phase(),
        Some(RunPhase::Waiting),
        "a fresh subscription waits for the token it will see first"
    );
    state.update(Action::FrameReceived(reasoning_delta(
        session_id, run_id, 0, "weighing",
    )));
    assert_eq!(state.run_phase(), Some(RunPhase::Thinking));
    state.update(Action::FrameReceived(delta(
        session_id,
        run_id,
        0,
        "the answer",
    )));
    assert_eq!(state.run_phase(), Some(RunPhase::Answering));
}

#[test]
fn a_terminal_status_ends_the_live_phase() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    state.update(Action::FrameReceived(delta(
        session_id,
        run_id,
        0,
        "the answer",
    )));
    assert_eq!(state.run_phase(), Some(RunPhase::Answering));
    state.update(Action::FrameReceived(status_frame(
        session_id,
        run_id,
        RunStatusDto::Completed,
    )));
    assert_eq!(
        state.run_phase(),
        None,
        "a terminal run's word comes from its own status, not a live phase"
    );
    assert_eq!(state.run_status(), Some(RunStatusDto::Completed));
}

#[test]
fn a_tool_call_in_flight_under_half_a_second_keeps_the_phase_it_found() {
    let (mut state, ..) = tool_call_in_flight(2_000);
    state.update(Action::ElapsedReported { millis: 2_400 });
    assert_eq!(
        state.run_phase(),
        Some(RunPhase::Answering),
        "a tool call under half a second in flight leaves the answering phase"
    );
    state.update(Action::ElapsedReported { millis: 2_500 });
    assert_eq!(
        state.run_phase(),
        Some(RunPhase::Answering),
        "half a second exactly is not yet past the threshold"
    );
}

#[test]
fn a_tool_call_in_flight_past_half_a_second_names_the_working_phase() {
    let (mut state, session_id, run_id, call_id) = tool_call_in_flight(2_000);
    state.update(Action::ElapsedReported { millis: 2_501 });
    assert_eq!(
        state.run_phase(),
        Some(RunPhase::Working),
        "one millisecond past half a second the row names the tool"
    );
    commit_row(
        &mut state,
        tool_result_row(session_id, run_id, "read", "the file", call_id),
    );
    assert_eq!(
        state.run_phase(),
        Some(RunPhase::Answering),
        "the answering result ends the watch and the phase it covered"
    );
}

#[test]
fn one_result_leaves_the_watch_while_another_call_is_in_flight() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let first = ToolCallId::new();
    let second = ToolCallId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    state.update(Action::FrameReceived(delta(
        session_id,
        run_id,
        0,
        "the answer",
    )));
    state.update(Action::ElapsedReported { millis: 1_000 });
    commit_row(
        &mut state,
        tool_call_row(session_id, run_id, "read", "{}", first),
    );
    commit_row(
        &mut state,
        tool_call_row(session_id, run_id, "glob", "{}", second),
    );
    state.update(Action::ElapsedReported { millis: 1_700 });
    assert_eq!(state.run_phase(), Some(RunPhase::Working));
    commit_row(
        &mut state,
        tool_result_row(session_id, run_id, "read", "the file", first),
    );
    assert_eq!(
        state.run_phase(),
        Some(RunPhase::Working),
        "the other call is still in flight"
    );
    commit_row(
        &mut state,
        tool_result_row(session_id, run_id, "glob", "the files", second),
    );
    assert_eq!(state.run_phase(), Some(RunPhase::Answering));
}

#[test]
fn a_tool_call_before_any_elapsed_report_never_names_working() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(session_id, run_id, RunStatusDto::Running);
    state.update(Action::FrameReceived(delta(
        session_id,
        run_id,
        0,
        "the answer",
    )));
    commit_row(
        &mut state,
        tool_call_row(session_id, run_id, "read", "{}", ToolCallId::new()),
    );
    state.update(Action::ElapsedReported { millis: 10_000 });
    assert_eq!(
        state.run_phase(),
        Some(RunPhase::Answering),
        "a call that committed before any report has no baseline to measure"
    );
}

#[test]
fn a_new_turn_forgets_the_previous_runs_phase_and_tool_watch() {
    let (mut state, ..) = tool_call_in_flight(2_000);
    state.update(Action::ElapsedReported { millis: 2_600 });
    assert_eq!(state.run_phase(), Some(RunPhase::Working));
    type_text(&mut state, "next");
    let effects = state.update(Action::InputSubmitted);
    assert!(
        matches!(effects.as_slice(), [Effect::SendTurn { .. }]),
        "the next turn leaves for the network"
    );
    assert_eq!(state.run_phase(), Some(RunPhase::Waiting));
    state.update(Action::ElapsedReported { millis: 2_600 });
    assert_eq!(
        state.run_phase(),
        Some(RunPhase::Waiting),
        "the previous run's tool baseline never measures the new turn"
    );
}

#[test]
fn opening_a_session_rebases_the_phase_on_its_own_run() {
    let first = SessionId::new();
    let run_id = RunId::new();
    let mut state = running_session(first, run_id, RunStatusDto::Running);
    state.update(Action::FrameReceived(delta(first, run_id, 0, "the answer")));
    assert_eq!(state.run_phase(), Some(RunPhase::Answering));

    let second = SessionId::new();
    state.update(Action::SessionSnapshotLoaded(snapshot(
        second,
        None,
        Vec::new(),
    )));
    assert_eq!(
        state.run_phase(),
        None,
        "an idle session carries no phase from the one it replaced"
    );

    let third = SessionId::new();
    let live = RunId::new();
    let effects = state.update(Action::SessionSnapshotLoaded(snapshot(
        third,
        Some(run(third, live, RunStatusDto::Running)),
        Vec::new(),
    )));
    assert_eq!(
        effects,
        vec![Effect::Subscribe {
            session_id: third,
            run_id: live,
        }]
    );
    assert_eq!(
        state.run_phase(),
        Some(RunPhase::Waiting),
        "the snapshot's own live run waits for the token this state will see first"
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
        vec![
            Effect::ListSessions,
            Effect::LoadSettings,
            Effect::OpenSession(session_id)
        ],
        "a reconnect re-reads the session it was showing and the daemon's settings"
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
