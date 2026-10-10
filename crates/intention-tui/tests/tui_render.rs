//! Render tests for the terminal front end.
//!
//! They render one fixture [`AppState`] through revue's own test app and assert
//! the panes, the container, and the geometry the front end is contracted to
//! show - the border a value sits in, the row a legend sits on, the column a
//! value is aligned to. The assertions name characters and cells rather than a
//! snapshot, so they survive layout changes, and they stay colour-agnostic:
//! revue's pilot exposes cells and characters, not styles, and the palette's
//! own unit tests guard the colour values.

#![allow(
    clippy::expect_used,
    reason = "Render tests build typed fixture states and assert the rendered text directly."
)]

use intention_client::RunStreamState;
use intention_proto::{
    ConfigRevisionId, DaemonHealthDto, ErrorDto, MessageKindDto, MessageProjectionDto, ProjectId,
    RunId, RunModeDto, RunProjectionDto, RunStatusDto, RunStreamFrameDto,
    RunSubscriptionSnapshotDto, SessionId, SessionProjectionDto, SessionSnapshotDto,
    SessionSummariesDto, SessionSummaryDto, TextDeltaChannelDto, TextDeltaFrameDto, ToolCallId,
    TurnId, WorkspaceId, WorkspaceRootDto,
};
use intention_tui::app::{
    Action, AppState, BrowserCursorMove, InputCursorMove, MenuMove, TranscriptScroll,
    short_identifier,
};
use intention_tui::tui::RevueView;
use revue::runtime::render::Modifier;
use revue::testing::{Pilot, TestApp};

/// The terminal size the transcript window test renders into.
///
/// `80x20` leaves an 8-row transcript window: 20 rows, minus the two canvas
/// margin rows and the panel's two frame rows, minus the input block's five
/// rows and the detail line, minus the two border rows of the transcript pane
/// itself.
const WINDOW_WIDTH: u16 = 80;
const WINDOW_HEIGHT: u16 = 20;

/// The committed rows the transcript window test renders.
const WINDOW_ROWS: usize = 30;

/// The window the browser tests render into: wide enough for the whole tab
/// radio row and tall enough for the panel's cap.
const BROWSER_WIDTH: u16 = 90;
const BROWSER_HEIGHT: u16 = 24;

/// The window the docked-panel geometry test renders into, tall enough to show
/// transcript rows on both sides of the docking.
const DOCK_WIDTH: u16 = 90;
const DOCK_HEIGHT: u16 = 30;

/// The window the browser cursor-window test renders into: half its rows still
/// show ten sessions.
const CURSOR_HEIGHT: u16 = 40;

/// The canonical id of the first fixture session.
const FIRST_SESSION: &str = "11111111-1111-4111-8111-111111111111";

/// The canonical id of the second fixture session.
const SECOND_SESSION: &str = "22222222-2222-4222-8222-222222222222";

/// The wall-clock second every browser render test measures relative times from.
const NOW: i64 = 1_759_000_000;

/// The transcript row the no-sessions-pane test renders: short enough to stay
/// on one display row at the full window width.
const FULL_WIDTH_ROW: &str = "alpha000 alpha001 alpha002 alpha003 alpha004 alpha005";

/// Returns the column a byte offset into one rendered line starts at.
///
/// [`TestApp::find_text`] reports the byte offset of the match, and a border
/// cell is three bytes wide, so its offset is not the column a cell assertion
/// takes. This turns the offset into a column through the line itself.
fn column(line: &str, offset: u16) -> u16 {
    line.get(..usize::from(offset)).map_or(offset, |prefix| {
        u16::try_from(prefix.chars().count()).unwrap_or(offset)
    })
}

/// Returns the column and row of the first `text` on the screen.
fn find_text(app: &TestApp<RevueView<'_>>, text: &str) -> (u16, u16) {
    let (offset, row) = app.find_text(text).expect("the text is rendered");
    (column(&app.get_line(row), offset), row)
}

/// Returns the row of the first `text` on the screen.
fn find_row(app: &TestApp<RevueView<'_>>, text: &str) -> u16 {
    find_text(app, text).1
}

/// Returns whether one row of a framed pane paints only its frame and spaces.
///
/// A gap row inside the transcript still carries the pane's own border cells,
/// so "blank" means no content between them.
fn is_blank_pane_row(line: &str) -> bool {
    line.chars()
        .all(|character| character == ' ' || character == '│')
}

/// Returns the fixture session id spelled by one canonical UUID literal.
fn fixture_session(value: &str) -> SessionId {
    SessionId::parse(value).expect("the fixture session id is canonical")
}

/// Returns one canonical session UUID literal for the browser window fixture.
fn session_literal(index: i64) -> String {
    format!("{index:08}-1111-4111-8111-111111111111")
}

/// Returns the absolute workspace root the fixture session was created in.
fn workspace_root() -> WorkspaceRootDto {
    WorkspaceRootDto::parse(std::env::temp_dir().to_string_lossy().into_owned())
        .expect("the process temporary directory is an absolute workspace root")
}

/// Returns one run projection of the fixture session.
fn run(session_id: SessionId, run_id: RunId, status: RunStatusDto) -> RunProjectionDto {
    RunProjectionDto::new(
        session_id,
        run_id,
        TurnId::new(),
        status,
        ConfigRevisionId::new(),
    )
}

/// Returns one session snapshot with its active run and committed rows.
fn snapshot(
    session_id: SessionId,
    active_run: Option<RunProjectionDto>,
    messages: Vec<MessageProjectionDto>,
) -> SessionSnapshotDto {
    snapshot_in_mode(session_id, RunModeDto::Build, active_run, messages)
}

/// Returns one session snapshot whose projection declares `mode`.
fn snapshot_in_mode(
    session_id: SessionId,
    mode: RunModeDto,
    active_run: Option<RunProjectionDto>,
    messages: Vec<MessageProjectionDto>,
) -> SessionSnapshotDto {
    let projection = SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        workspace_root(),
        mode,
        None,
        active_run,
        Vec::new(),
    )
    .expect("the fixture session projection is coherent");
    SessionSnapshotDto::with_projection(session_id, projection, messages)
        .expect("the fixture session snapshot is coherent")
}

/// Returns one session summary of the fixture session.
fn summary(session_id: SessionId) -> SessionSummaryDto {
    summary_at(session_id, NOW)
}

/// Returns one session summary last updated at `updated_at`.
fn summary_at(session_id: SessionId, updated_at: i64) -> SessionSummaryDto {
    SessionSummaryDto::new(
        session_id,
        ProjectId::new(),
        WorkspaceId::new(),
        RunModeDto::Build,
        updated_at,
        None,
    )
}

/// Returns one committed transcript row of the fixture session.
fn row(
    session_id: SessionId,
    run_id: RunId,
    kind: MessageKindDto,
    text: &str,
) -> MessageProjectionDto {
    MessageProjectionDto::new(session_id, Some(run_id), kind, text, None, None, None)
        .expect("the fixture transcript row is coherent")
}

/// Returns one committed tool-call row carrying `arguments` for `tool_id`.
fn tool_call_row(
    session_id: SessionId,
    run_id: RunId,
    tool_id: &str,
    arguments: &str,
    call_id: ToolCallId,
) -> MessageProjectionDto {
    MessageProjectionDto::new(
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

/// Returns the two committed rows of one durable tool exchange.
fn tool_exchange(
    session_id: SessionId,
    run_id: RunId,
    tool_id: &str,
    arguments: &str,
    content: &str,
) -> Vec<MessageProjectionDto> {
    let call_id = ToolCallId::new();
    vec![
        tool_call_row(session_id, run_id, tool_id, arguments, call_id),
        tool_result_row(session_id, run_id, tool_id, content, call_id),
    ]
}

/// Returns one committed daemon notice row carrying `text`.
fn notice_row(session_id: SessionId, run_id: RunId, text: &str) -> MessageProjectionDto {
    MessageProjectionDto::new(
        session_id,
        Some(run_id),
        MessageKindDto::Notice,
        text,
        None,
        None,
        None,
    )
    .expect("the fixture notice row is coherent")
}

/// Returns one committed assistant row carrying `reasoning` above its answer.
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

/// Returns a connected state that listed `sessions` with `omitted` sessions
/// beyond the reported window and opened the sessions browser.
fn browser_state(sessions: Vec<SessionSummaryDto>, omitted: u32) -> AppState {
    let mut state = AppState::new(None);
    state.update(Action::Bootstrapped(DaemonHealthDto::ready()));
    state.update(Action::SessionsListed(
        SessionSummariesDto::new(sessions, omitted).expect("the fixture session list is coherent"),
    ));
    state.update(Action::SessionsBrowserRequested);
    state
}

/// Returns a connected state that listed one session summary with `omitted`
/// sessions beyond the reported window.
fn listed_state(session_id: SessionId, omitted: u32) -> AppState {
    let mut state = AppState::new(Some(session_id));
    state.update(Action::Bootstrapped(DaemonHealthDto::ready()));
    state.update(Action::SessionsListed(
        SessionSummariesDto::new(vec![summary(session_id)], omitted)
            .expect("the fixture session list is coherent"),
    ));
    state
}

/// Returns a listed state that also loaded its session snapshot with `messages`.
fn open_state(session_id: SessionId, messages: Vec<MessageProjectionDto>) -> AppState {
    let mut state = listed_state(session_id, 0);
    state.update(Action::SessionSnapshotLoaded(snapshot(
        session_id, None, messages,
    )));
    state
}

/// Returns a connected state that listed no sessions: the chat's empty state.
fn welcome_state() -> AppState {
    let mut state = AppState::new(None);
    state.update(Action::Bootstrapped(DaemonHealthDto::ready()));
    state.update(Action::SessionsListed(
        SessionSummariesDto::new(Vec::new(), 0).expect("the empty session list is coherent"),
    ));
    state
}

/// Applies the opened-subscription action for one run of `status` to `state`.
fn subscribe(state: &mut AppState, session_id: SessionId, run_id: RunId, status: RunStatusDto) {
    let subscription = RunSubscriptionSnapshotDto::new(run(session_id, run_id, status), Vec::new())
        .expect("the fixture run snapshot is coherent");
    let mut stream = RunStreamState::new(session_id, run_id);
    stream
        .apply_initial(subscription)
        .expect("the fixture run snapshot applies to its own scope");
    state.update(Action::RunStreamOpened(stream));
}

/// Returns a connected session that streams one running run with one committed
/// row, carrying `provisional` as the transient tail of the current step.
fn streaming_state(provisional: Option<&str>) -> AppState {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = open_state(
        session_id,
        vec![row(
            session_id,
            run_id,
            MessageKindDto::Assistant,
            "the committed row",
        )],
    );
    subscribe(&mut state, session_id, run_id, RunStatusDto::Running);
    if let Some(text) = provisional {
        state.update(Action::FrameReceived(RunStreamFrameDto::TextDelta(
            TextDeltaFrameDto::new(session_id, run_id, 0, TextDeltaChannelDto::Answer, text)
                .expect("the fixture delta is valid"),
        )));
    }
    state
}

/// Returns a connected session whose current step streams `reasoning` before
/// the `answer` it informs.
fn streaming_reasoning_state(reasoning: &str, answer: &str) -> AppState {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = open_state(
        session_id,
        vec![row(
            session_id,
            run_id,
            MessageKindDto::Assistant,
            "the committed row",
        )],
    );
    subscribe(&mut state, session_id, run_id, RunStatusDto::Running);
    for (channel, text) in [
        (TextDeltaChannelDto::Reasoning, reasoning),
        (TextDeltaChannelDto::Answer, answer),
    ] {
        state.update(Action::FrameReceived(RunStreamFrameDto::TextDelta(
            TextDeltaFrameDto::new(session_id, run_id, 0, channel, text)
                .expect("the fixture delta is valid"),
        )));
    }
    state
}

/// Returns the browser panel state of two fixture sessions in the browser test
/// window.
fn two_session_state() -> AppState {
    browser_state(
        vec![
            summary_at(fixture_session(FIRST_SESSION), NOW - 23 * 60),
            summary_at(fixture_session(SECOND_SESSION), NOW - 2 * 3_600),
        ],
        0,
    )
}

#[test]
fn the_browser_panel_shows_its_frame_and_embedded_title() {
    let state = browser_state(vec![summary(fixture_session(FIRST_SESSION))], 0);
    let mut app = TestApp::with_size(
        RevueView::with_now(&state, NOW),
        BROWSER_WIDTH,
        BROWSER_HEIGHT,
    );
    let (title_x, title_row) = find_text(&app, "Sessions");
    assert_eq!(
        title_x, 3,
        "the docked panel starts at the window's first column"
    );
    let pilot = Pilot::new(&mut app);
    pilot.assert_cell(0, title_row, '╭');
    pilot.assert_cell(1, title_row, '─');
    pilot.assert_cell(title_x - 1, title_row, ' ');
    pilot.assert_line_contains(title_row + 2, "◦ Current Folder");
}

#[test]
fn the_browser_tabs_show_the_radio_glyphs_and_separators() {
    let mut state = browser_state(vec![summary(fixture_session(FIRST_SESSION))], 0);
    {
        let mut app = TestApp::with_size(
            RevueView::with_now(&state, NOW),
            BROWSER_WIDTH,
            BROWSER_HEIGHT,
        );
        let pilot = Pilot::new(&mut app);
        pilot.assert_contains("◦ Current Folder  |  ⦿ All");
        pilot.assert_contains("|  ◦ Favorites  |  ◦ Archived");
    }
    state.update(Action::BrowserTabNext);
    let mut app = TestApp::with_size(
        RevueView::with_now(&state, NOW),
        BROWSER_WIDTH,
        BROWSER_HEIGHT,
    );
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("⦿ Exec");
    pilot.assert_not_contains("⦿ All");
}

#[test]
fn the_browser_search_bar_shows_its_placeholder() {
    let state = browser_state(vec![summary(fixture_session(FIRST_SESSION))], 0);
    let mut app = TestApp::with_size(
        RevueView::with_now(&state, NOW),
        BROWSER_WIDTH,
        BROWSER_HEIGHT,
    );
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("Type to filter sessions...");
}

#[test]
fn the_browser_shows_the_table_header() {
    let state = browser_state(vec![summary(fixture_session(FIRST_SESSION))], 0);
    let mut app = TestApp::with_size(
        RevueView::with_now(&state, NOW),
        BROWSER_WIDTH,
        BROWSER_HEIGHT,
    );
    let header_row = find_row(&app, "Modified");
    let pilot = Pilot::new(&mut app);
    pilot.assert_line_contains(header_row, "Created");
    pilot.assert_line_contains(header_row, "Size");
    pilot.assert_line_contains(header_row, "Title");
    pilot.assert_line_contains(header_row, "Path");
    pilot.assert_line_contains(header_row + 1, "session 11111111");
}

#[test]
fn the_browser_shows_modified_values_from_the_view_clock() {
    let state = two_session_state();
    let mut app = TestApp::with_size(
        RevueView::with_now(&state, NOW),
        BROWSER_WIDTH,
        BROWSER_HEIGHT,
    );
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("23m ago");
    pilot.assert_contains("2h ago");
}

#[test]
fn the_browser_marks_the_cursor_row_bold() {
    let state = two_session_state();
    let mut app = TestApp::with_size(
        RevueView::with_now(&state, NOW),
        BROWSER_WIDTH,
        BROWSER_HEIGHT,
    );
    let (marker_x, marker_row) = find_text(&app, "> 23m ago");
    let marker = app
        .buffer()
        .get(marker_x, marker_row)
        .expect("the cursor marker cell is rendered");
    assert!(
        marker.modifier.contains(Modifier::BOLD),
        "the cursor row renders bold"
    );
    {
        let pilot = Pilot::new(&mut app);
        pilot.assert_line_contains(marker_row, "session 11111111");
    }
}

#[test]
fn the_browser_moves_the_cursor_marker_to_the_selected_row() {
    let mut state = two_session_state();
    state.update(Action::BrowserCursorMove(BrowserCursorMove::Down));
    let mut app = TestApp::with_size(
        RevueView::with_now(&state, NOW),
        BROWSER_WIDTH,
        BROWSER_HEIGHT,
    );
    let pilot = Pilot::new(&mut app);
    let (_, selected_row) = pilot
        .find_text("session 22222222")
        .expect("the second row is shown");
    pilot.assert_line_contains(selected_row, "> 2h ago");
    pilot.assert_not_contains("> 23m ago");
}

#[test]
fn the_browser_panel_keeps_its_legend_on_its_own_last_row() {
    let state = two_session_state();
    let mut app = TestApp::with_size(RevueView::with_now(&state, NOW), 150, 40);
    let pilot = Pilot::new(&mut app);
    let (_, legend_row) = pilot.find_text("select").expect("the legend is shown");
    assert_eq!(
        legend_row + 1,
        39,
        "the legend is the panel's own last inner row"
    );
    let bottom = pilot.line(39);
    assert!(
        bottom.starts_with('╰') && bottom.trim_end().ends_with('╯'),
        "the panel's bottom edge is the window's last row: {bottom:?}"
    );
}

#[test]
fn the_browser_footer_shows_the_legend_and_the_row_counter() {
    let state = two_session_state();
    let mut app = TestApp::with_size(RevueView::with_now(&state, NOW), 150, BROWSER_HEIGHT);
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("Enter select");
    pilot.assert_contains("Ctrl+F tree");
    pilot.assert_contains("Esc close");
    pilot.assert_contains("1-2 of 2");
    pilot.assert_not_contains("↑↓");
    pilot.assert_not_contains("navigate");
}

#[test]
fn the_browser_footer_styles_a_key_distinctly_from_its_action() {
    let state = two_session_state();
    let app = TestApp::with_size(RevueView::with_now(&state, NOW), 150, BROWSER_HEIGHT);
    let (key_x, key_row) = find_text(&app, "Enter");
    let (action_x, action_row) = find_text(&app, "select");
    let key = app
        .buffer()
        .get(key_x, key_row)
        .expect("the key cell is rendered");
    let action = app
        .buffer()
        .get(action_x, action_row)
        .expect("the action cell is rendered");
    assert!(
        key.modifier.contains(Modifier::BOLD),
        "the hotkey itself is written bold"
    );
    assert!(
        !action.modifier.contains(Modifier::BOLD),
        "the action beside it stays muted"
    );
}

#[test]
fn the_browser_counter_reports_sessions_beyond_the_list_window() {
    let state = browser_state(vec![summary(fixture_session(FIRST_SESSION))], 3);
    let mut app = TestApp::with_size(RevueView::with_now(&state, NOW), 150, BROWSER_HEIGHT);
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("1-1 of 1 +3 omitted (core: paged list @todo)");
}

#[test]
fn a_stub_tab_shows_its_todo_empty_state() {
    let mut state = browser_state(vec![summary(fixture_session(FIRST_SESSION))], 0);
    state.update(Action::BrowserTabNext);
    let mut app = TestApp::with_size(
        RevueView::with_now(&state, NOW),
        BROWSER_WIDTH,
        BROWSER_HEIGHT,
    );
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("⦿ Exec");
    pilot.assert_contains("@todo(core): the session summary carries no execute-call");
    pilot.assert_contains("0 of 0");
}

#[test]
fn the_browser_table_shows_a_right_aligned_trailing_column() {
    let state = two_session_state();
    let mut app = TestApp::with_size(RevueView::with_now(&state, NOW), 130, BROWSER_HEIGHT);
    let (path_x, header_row) = find_text(&app, "Path");
    let pilot = Pilot::new(&mut app);
    pilot.assert_cell(path_x + 4, header_row, ' ');
    pilot.assert_cell(path_x + 6, header_row, '│');
    pilot.assert_cell(path_x + 3, header_row + 1, '—');
}

#[test]
fn the_browser_truncates_a_long_title_with_an_ellipsis() {
    let state = browser_state(
        vec![summary_at(fixture_session(FIRST_SESSION), NOW - 23 * 60)],
        0,
    );
    let mut app = TestApp::with_size(RevueView::with_now(&state, NOW), 52, BROWSER_HEIGHT);
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("session 111111…");
    pilot.assert_not_contains("session 11111111");
}

#[test]
fn the_browser_table_reflows_when_the_window_resizes() {
    let state = browser_state(
        vec![summary_at(fixture_session(FIRST_SESSION), NOW - 23 * 60)],
        0,
    );
    let mut app = TestApp::with_size(RevueView::with_now(&state, NOW), 52, BROWSER_HEIGHT);
    let mut pilot = Pilot::new(&mut app);
    pilot.assert_contains("session 111111…");
    let (narrow_offset, narrow_row) = pilot.find_text("Path").expect("the column is named");
    let narrow_path = column(&pilot.line(narrow_row), narrow_offset);
    pilot.resize(130, BROWSER_HEIGHT);
    let (offset, header_row) = pilot.find_text("Path").expect("the column is named");
    let wide_path = column(&pilot.line(header_row), offset);
    assert!(
        wide_path > narrow_path,
        "the trailing column moves with the window"
    );
    pilot.assert_contains("session 11111111");
    pilot.assert_cell(wide_path + 6, header_row, '│');
}

#[test]
fn the_browser_window_follows_the_cursor() {
    let sessions = (0_i64..30)
        .map(|index| summary_at(fixture_session(&session_literal(index)), NOW - index * 60))
        .collect();
    let mut state = browser_state(sessions, 0);
    {
        let mut app = TestApp::with_size(RevueView::with_now(&state, NOW), 100, CURSOR_HEIGHT);
        let pilot = Pilot::new(&mut app);
        pilot.assert_contains("session 00000000");
        pilot.assert_contains("1-10 of 30");
    }
    for _ in 0..12 {
        state.update(Action::BrowserCursorMove(BrowserCursorMove::Down));
    }
    let mut app = TestApp::with_size(RevueView::with_now(&state, NOW), 100, CURSOR_HEIGHT);
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("session 00000012");
    pilot.assert_contains("session 00000003");
    pilot.assert_not_contains("session 00000002");
    pilot.assert_not_contains("session 00000000");
    pilot.assert_contains("4-13 of 30");
}

#[test]
fn the_browser_panel_docks_below_the_chat_and_shrinks_the_transcript() {
    let session_id = fixture_session(FIRST_SESSION);
    let run_id = RunId::new();
    let rows = (0..WINDOW_ROWS)
        .map(|index| {
            row(
                session_id,
                run_id,
                MessageKindDto::User,
                &format!("row {index}"),
            )
        })
        .collect::<Vec<_>>();
    let closed = open_state(session_id, rows);
    {
        let mut app =
            TestApp::with_size(RevueView::with_now(&closed, NOW), DOCK_WIDTH, DOCK_HEIGHT);
        let pilot = Pilot::new(&mut app);
        pilot.assert_contains("row 25");
        pilot.assert_contains("row 29");
        pilot.assert_not_contains("user>");
        pilot.assert_not_contains("╭─ Sessions");
    }

    let mut opened = closed;
    opened.update(Action::SessionsBrowserRequested);
    let mut app = TestApp::with_size(RevueView::with_now(&opened, NOW), DOCK_WIDTH, DOCK_HEIGHT);
    let (_, panel_row) = find_text(&app, "╭─ Sessions");
    let (_, transcript_row) = find_text(&app, "transcript");
    let (_, detail_row) = find_text(&app, "enter sends");
    let pilot = Pilot::new(&mut app);
    pilot.assert_cell(0, panel_row, '╭');
    assert!(
        panel_row > transcript_row,
        "the panel sits below the chat's transcript"
    );
    assert!(
        panel_row > detail_row,
        "the panel sits below the chat's detail line"
    );
    assert!(
        pilot.line(panel_row - 1).trim().is_empty(),
        "exactly one blank gap row separates the chat container from the panel"
    );
    assert!(
        pilot.line(panel_row - 2).contains('╰'),
        "the chat container's frame ends above the gap: {:?}",
        pilot.line(panel_row - 2)
    );
    let bottom = pilot.line(DOCK_HEIGHT - 1);
    assert!(
        bottom.starts_with('╰') && bottom.trim_end().ends_with('╯'),
        "the panel's bottom edge is the window's last row: {bottom:?}"
    );
    pilot.assert_contains("row 28");
    pilot.assert_contains("row 29");
    pilot.assert_not_contains("row 25");
}

#[test]
fn the_chat_screen_shows_no_sessions_pane() {
    let session_id = fixture_session(FIRST_SESSION);
    let run_id = RunId::new();
    let state = open_state(
        session_id,
        vec![row(
            session_id,
            run_id,
            MessageKindDto::Assistant,
            FULL_WIDTH_ROW,
        )],
    );
    let mut app = TestApp::with_size(
        RevueView::with_now(&state, NOW),
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
    );
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains(FULL_WIDTH_ROW);
    pilot.assert_not_contains("assistant>");
    pilot.assert_not_contains("╭─ Sessions");
}

#[test]
fn the_transcript_shows_a_committed_row() {
    let state = streaming_state(None);
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("transcript");
    pilot.assert_contains("the committed row");
    pilot.assert_not_contains("assistant> the committed row");
    pilot.assert_not_contains("(provisional)");
}

#[test]
fn a_read_tool_exchange_draws_its_path_and_a_bounded_content_preview() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let content = (0..30)
        .map(|index| format!("line {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let state = open_state(
        session_id,
        tool_exchange(
            session_id,
            run_id,
            "read",
            r#"{"path":"src/lib.rs","limit":10}"#,
            &content,
        ),
    );
    // The window is tall enough for the badge, the bounded preview, and its
    // marker: the block's own header stays visible.
    let mut app = TestApp::with_size(RevueView::new(&state), WINDOW_WIDTH, 40);
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("▸ read · src/lib.rs · limit 10");
    pilot.assert_contains("line 0");
    pilot.assert_contains("… 18 more rows");
    pilot.assert_not_contains("line 29");
    pilot.assert_not_contains(r#"{"path":"src/lib.rs","limit":10}"#);
}

#[test]
fn a_glob_or_grep_exchange_draws_its_parsed_matches() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut messages = tool_exchange(
        session_id,
        run_id,
        "glob",
        r#"{"pattern":"**/*.rs"}"#,
        "src/lib.rs\nsrc/main.rs",
    );
    messages.extend(tool_exchange(
        session_id,
        run_id,
        "grep",
        r#"{"pattern":"needle","scope":{"kind":"directory","path":"src"}}"#,
        "src/lib.rs:3:5: needle",
    ));
    let state = open_state(session_id, messages);
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("╭─ ▸ glob · \"**/*.rs\"");
    pilot.assert_contains("│ ‣ src/lib.rs");
    pilot.assert_contains("│ ‣ src/main.rs");
    pilot.assert_contains("╭─ ▸ grep · \"needle\" · directory · src");
    pilot.assert_contains("src/lib.rs:3:5 · needle");
}

#[test]
fn a_mutation_or_execute_exchange_draws_a_badge_and_a_plate_without_its_result() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut messages = Vec::new();
    for (tool_id, arguments) in [
        ("write", r#"{"path":"src/lib.rs","content":"new"}"#),
        ("edit", r#"{"path":"src/lib.rs","old":"a","new":"b"}"#),
        ("execute", r#"{"program":"cargo","args":["test"]}"#),
    ] {
        messages.extend(tool_exchange(
            session_id,
            run_id,
            tool_id,
            arguments,
            "SECRET COMMAND OUTPUT",
        ));
    }
    let state = open_state(session_id, messages);
    // The window is tall enough for all three framed blocks: each one is a
    // badge border, its plate, and a bottom border.
    let mut app = TestApp::with_size(RevueView::new(&state), WINDOW_WIDTH, 40);
    let pilot = Pilot::new(&mut app);
    for tool_id in ["write", "edit"] {
        pilot.assert_contains(&format!("╭─ ▸ {tool_id} · src/lib.rs"));
    }
    pilot.assert_contains("╭─ ▸ execute · cargo test");
    pilot.assert_contains("@todo(core): ");
    pilot.assert_not_contains("SECRET COMMAND OUTPUT");
    pilot.assert_not_contains("\"content\":\"new\"");
}

#[test]
fn a_call_without_a_result_row_draws_its_call_alone() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let state = open_state(
        session_id,
        vec![tool_call_row(
            session_id,
            run_id,
            "read",
            r#"{"path":"src/lib.rs"}"#,
            ToolCallId::new(),
        )],
    );
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("▸ read · src/lib.rs");
    pilot.assert_contains("no tool_result row is committed");
}

#[test]
fn a_notice_row_streamed_from_the_run_renders_as_its_own_block() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = open_state(session_id, Vec::new());
    subscribe(&mut state, session_id, run_id, RunStatusDto::Running);
    // The engine commits its interruption notice as a `notice` row and the
    // daemon streams it as a content frame, exactly like every other row.
    state.update(Action::FrameReceived(RunStreamFrameDto::Content(
        notice_row(
            session_id,
            run_id,
            "[The call was stopped before a final result.]",
        ),
    )));
    assert_eq!(
        state.transcript()[0].kind(),
        MessageKindDto::Notice,
        "the streamed notice row reaches the committed transcript"
    );
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("※ [The call was stopped before a final result.]");
    pilot.assert_not_contains("notice>");
}

#[test]
fn a_notice_row_restored_by_a_session_snapshot_renders_as_a_notice_block_too() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let state = open_state(
        session_id,
        vec![notice_row(
            session_id,
            run_id,
            "the session carried this notice",
        )],
    );
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("※ the session carried this notice");
    pilot.assert_not_contains("notice>");
}

#[test]
fn an_unknown_tool_row_still_degrades_to_its_plate() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let call_id = ToolCallId::new();
    let state = open_state(
        session_id,
        vec![
            tool_call_row(
                session_id,
                run_id,
                "mystery",
                "not one json object",
                call_id,
            ),
            tool_result_row(
                session_id,
                run_id,
                "mystery",
                "the unknown tool's output",
                call_id,
            ),
        ],
    );
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("╭─ ▸ mystery");
    pilot.assert_contains("not one json object");
    pilot.assert_contains("the wire names tools by string");
    pilot.assert_contains("the closed set is");
    pilot.assert_contains("not declared");
    pilot.assert_not_contains("the unknown tool's output");
}

#[test]
fn activating_a_collapsed_reasoning_block_reveals_one_chunk_of_rows_at_a_time() {
    let session_id = fixture_session(FIRST_SESSION);
    let run_id = RunId::new();
    let reasoning = (0..300)
        .map(|index| format!("step {index:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut state = open_state(
        session_id,
        vec![reasoning_row(session_id, run_id, "the answer", &reasoning)],
    );
    {
        let mut app = TestApp::new(RevueView::new(&state));
        let pilot = Pilot::new(&mut app);
        pilot.assert_contains("step 014");
        pilot.assert_not_contains("step 015");
        pilot.assert_contains("… 285 more reasoning lines hidden");
    }
    state.update(Action::ExpandReasoning { row: Some(0) });
    {
        let mut app = TestApp::new(RevueView::new(&state));
        let pilot = Pilot::new(&mut app);
        pilot.assert_contains("step 114");
        pilot.assert_not_contains("step 115");
        pilot.assert_contains("… 185 more reasoning lines hidden");
    }
    state.update(Action::ExpandReasoning { row: Some(0) });
    {
        let mut app = TestApp::new(RevueView::new(&state));
        let pilot = Pilot::new(&mut app);
        pilot.assert_contains("step 214");
        pilot.assert_not_contains("step 215");
        pilot.assert_contains("… 85 more reasoning lines hidden");
    }
    state.update(Action::ExpandReasoning { row: Some(0) });
    {
        let mut app = TestApp::new(RevueView::new(&state));
        let pilot = Pilot::new(&mut app);
        pilot.assert_contains("step 299");
        pilot.assert_not_contains("more reasoning lines hidden");
        pilot.assert_not_contains("step 300");
    }
}

#[test]
fn expanding_a_reasoning_block_above_the_window_keeps_the_visible_rows() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let reasoning = (0..300)
        .map(|index| format!("step {index:03}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut messages = vec![reasoning_row(session_id, run_id, "the answer", &reasoning)];
    for index in 0..20 {
        messages.push(row(
            session_id,
            run_id,
            MessageKindDto::Assistant,
            &format!("row {index:02}"),
        ));
    }
    let mut state = open_state(session_id, messages);
    let before = {
        let app = TestApp::with_size(
            RevueView::with_now(&state, NOW),
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
        );
        (app.screen_text(), state.scroll())
    };
    // The reasoning block sits above the window, so the hundred rows the
    // activation reveals are inserted above its first visible row.
    state.update(Action::ExpandReasoning { row: Some(0) });
    let after = {
        let app = TestApp::with_size(
            RevueView::with_now(&state, NOW),
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
        );
        (app.screen_text(), state.scroll())
    };
    assert_eq!(
        after.1, before.1,
        "the expansion leaves the transcript's scroll offset alone"
    );
    assert!(
        after.0.contains("row 19"),
        "the newest rows stay visible: {}",
        after.0
    );
    assert_eq!(
        after.0, before.0,
        "an insertion above the window keeps the visible rows in place"
    );
}

#[test]
fn the_streaming_tail_renders_as_the_answer_row_it_becomes() {
    let state = streaming_state(Some("partial answer"));
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("∷ partial answer");
    pilot.assert_not_contains("(provisional)");
}

#[test]
fn the_live_reasoning_segment_renders_as_the_committed_block_it_becomes() {
    let state = streaming_reasoning_state("weighing the options", "partial answer");
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("∴ reasoning");
    pilot.assert_contains("weighing the options");
    pilot.assert_contains("∷ partial answer");
    pilot.assert_not_contains("(provisional)");

    // A step that thinks longer than the collapsed block shows carries the
    // committed block's own collapse: the same affordance, the same count, and
    // the answer after it.
    let long = (0..25)
        .map(|index| format!("thought {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let state = streaming_reasoning_state(&long, "partial answer");
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("more reasoning lines hidden — click or Ctrl+E to reveal more");
    pilot.assert_contains("∷ partial answer");
}

#[test]
fn a_user_row_renders_as_a_labelled_card() {
    let session_id = fixture_session(FIRST_SESSION);
    let run_id = RunId::new();
    let state = open_state(
        session_id,
        vec![row(
            session_id,
            run_id,
            MessageKindDto::User,
            "the question",
        )],
    );
    let mut app = TestApp::new(RevueView::new(&state));
    let (_, label_row) = find_text(&app, "❯ you");
    let (_, question_row) = find_text(&app, "the question");
    let pilot = Pilot::new(&mut app);
    pilot.assert_line_contains(label_row, "╭─ ❯ you");
    pilot.assert_line_contains(label_row, "╮");
    pilot.assert_line_contains(question_row, "the question");
    pilot.assert_line_contains(question_row + 1, "╰");
    pilot.assert_not_contains("user>");
}

#[test]
fn markdown_inside_a_user_card_renders() {
    let session_id = fixture_session(FIRST_SESSION);
    let run_id = RunId::new();
    let state = open_state(
        session_id,
        vec![row(
            session_id,
            run_id,
            MessageKindDto::User,
            "use **care** here\n\n```\nlet x = 1;\n```",
        )],
    );
    let mut app = TestApp::new(RevueView::new(&state));
    let (strong_x, strong_row) = find_text(&app, "care");
    let strong = app
        .buffer()
        .get(strong_x, strong_row)
        .expect("the bold cell is rendered");
    assert!(
        strong.modifier.contains(Modifier::BOLD),
        "**care** renders bold inside the user card"
    );
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("╭─ ❯ you");
    pilot.assert_contains("let x = 1;");
    pilot.assert_contains("│");
}

/// Returns the chat state of one committed answer with `typed` in the input.
fn typed_state(typed: &str) -> AppState {
    let session_id = fixture_session(FIRST_SESSION);
    let run_id = RunId::new();
    let mut state = open_state(
        session_id,
        vec![row(
            session_id,
            run_id,
            MessageKindDto::Assistant,
            "the committed answer",
        )],
    );
    for character in typed.chars() {
        state.update(Action::InputChar(character));
    }
    state
}

#[test]
fn the_hint_menu_renders_its_three_columns_above_the_input_block() {
    let without = typed_state("");
    let with = typed_state("/");
    let closed = TestApp::with_size(RevueView::new(&without), WINDOW_WIDTH, WINDOW_HEIGHT);
    let mut opened = TestApp::with_size(RevueView::new(&with), WINDOW_WIDTH, WINDOW_HEIGHT);

    let (band_x, band_row) = find_text(&opened, "╭─ commands");
    let (_, cursor_row) = find_text(&opened, "█");
    assert!(
        band_row < cursor_row,
        "the band sits above the input block, not over it: {band_row} < {cursor_row}"
    );
    assert!(
        band_row > 0,
        "the transcript keeps the rows above the band: {band_row}"
    );

    let pilot = Pilot::new(&mut opened);
    pilot.assert_line_contains(band_row, "commands");
    pilot.assert_line_contains(band_row + 1, "> /new");
    pilot.assert_line_contains(band_row + 1, "create a new session in this workspace");
    pilot.assert_line_contains(band_row + 1, "Session");
    pilot.assert_line_contains(band_row + 2, "/sessions");
    pilot.assert_line_contains(band_row + 2, "browse, filter, and switch sessions");
    pilot.assert_line_contains(band_row + 2, "Navigation");
    pilot.assert_cell(band_x, band_row + 3, '╰');
    pilot.assert_cell(band_x, band_row + 4, '╭');

    // The band takes its rows out of the transcript: the committed answer is
    // still shown, and the input block and the detail line keep their rows.
    pilot.assert_contains("the committed answer");
    let (_, closed_cursor) = find_text(&closed, "█");
    let (_, closed_detail) = find_text(&closed, "enter sends");
    let (_, opened_detail) = find_text(&opened, "enter sends");
    assert_eq!(
        (cursor_row, opened_detail),
        (closed_cursor, closed_detail),
        "the menu never moves the input block or the detail line"
    );
}

#[test]
fn the_hint_menu_narrows_and_moves_its_highlight() {
    let state = typed_state("/s");
    let mut app = TestApp::with_size(RevueView::new(&state), WINDOW_WIDTH, WINDOW_HEIGHT);
    let (_, band_row) = find_text(&app, "╭─ commands");
    let pilot = Pilot::new(&mut app);
    pilot.assert_line_contains(band_row + 1, "> /sessions");
    assert!(
        !pilot.line(band_row + 1).contains("/new"),
        "the filter hides every command it does not select: {}",
        pilot.line(band_row + 1)
    );
    pilot.assert_line_contains(band_row + 2, "╰");
    let (_, badge_row) = find_text(&app, "build");
    assert_eq!(
        badge_row,
        band_row + 4,
        "one command row and a frame, and the input block follows immediately"
    );

    let mut state = typed_state("/");
    let first_row = {
        let app = TestApp::with_size(RevueView::new(&state), WINDOW_WIDTH, WINDOW_HEIGHT);
        find_text(&app, "/new").1
    };
    state.update(Action::MenuMove(MenuMove::Down));
    let second_row = {
        let mut app = TestApp::with_size(RevueView::new(&state), WINDOW_WIDTH, WINDOW_HEIGHT);
        let pilot = Pilot::new(&mut app);
        pilot.assert_contains("> /sessions");
        pilot.assert_not_contains("> /new");
        find_text(&app, "/new").1
    };
    assert_eq!(
        first_row, second_row,
        "the rows do not move when the highlight does"
    );

    state.update(Action::MenuMove(MenuMove::Up));
    let mut app = TestApp::with_size(RevueView::new(&state), WINDOW_WIDTH, WINDOW_HEIGHT);
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("> /new");
    pilot.assert_not_contains("> /sessions");
}

#[test]
fn a_slash_that_is_not_the_first_character_opens_no_menu() {
    let state = typed_state("read /new");
    let mut app = TestApp::with_size(RevueView::new(&state), WINDOW_WIDTH, WINDOW_HEIGHT);
    let pilot = Pilot::new(&mut app);
    pilot.assert_not_contains("╭─ commands");
    pilot.assert_not_contains("create a new session in this workspace");
    pilot.assert_contains("read /new");
}

#[test]
fn a_window_too_short_for_the_band_keeps_the_input_and_shows_no_band() {
    let state = typed_state("/");
    let mut app = TestApp::with_size(RevueView::new(&state), WINDOW_WIDTH, 12);
    let pilot = Pilot::new(&mut app);
    pilot.assert_not_contains("╭─ commands");
    pilot.assert_contains("> /█");
    pilot.assert_contains("enter sends | /new session | /sessions switch");
}

#[test]
fn a_blank_row_separates_the_user_card_from_the_reasoning_and_the_answer() {
    let session_id = fixture_session(FIRST_SESSION);
    let run_id = RunId::new();
    let state = open_state(
        session_id,
        vec![
            row(session_id, run_id, MessageKindDto::User, "the question"),
            reasoning_row(session_id, run_id, "the answer", "the thinking"),
        ],
    );
    let mut app = TestApp::new(RevueView::new(&state));
    let (_, header_row) = find_text(&app, "∴ reasoning");
    let (_, reasoning_row) = find_text(&app, "the thinking");
    let (_, answer_row) = find_text(&app, "the answer");
    let pilot = Pilot::new(&mut app);
    assert!(
        header_row < reasoning_row && reasoning_row < answer_row,
        "the reasoning block sits between the user card and the answer"
    );
    pilot.assert_line_contains(header_row - 2, "╰");
    assert!(
        is_blank_pane_row(&pilot.line(header_row - 1)),
        "one blank row separates the user card from the reasoning"
    );
    pilot.assert_line_contains(answer_row - 2, "────────");
    assert!(
        is_blank_pane_row(&pilot.line(answer_row - 1)),
        "one blank row separates the reasoning from the answer"
    );
    pilot.assert_line_contains(answer_row, "∷ the answer");
}

#[test]
fn the_session_notice_is_gone_and_the_status_row_names_the_session() {
    let session_id = fixture_session(FIRST_SESSION);
    let state = open_state(session_id, Vec::new());
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_not_contains(" open");
    pilot.assert_not_contains("11111111 open");
    pilot.assert_contains("Ready · session 11111111");
}

#[test]
fn a_long_committed_row_wraps_and_shows_its_tail() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let state = open_state(
        session_id,
        vec![row(
            session_id,
            run_id,
            MessageKindDto::Assistant,
            "alpha beta gamma delta epsilon zeta eta theta iota kappa",
        )],
    );
    // A 56-column terminal leaves a 48-column transcript, so the answer wraps
    // into two display rows at the last word that fits.
    let mut app = TestApp::with_size(RevueView::new(&state), 56, WINDOW_HEIGHT);
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("alpha beta gamma delta epsilon zeta eta theta");
    pilot.assert_contains("  iota kappa");
}

#[test]
fn a_wrapped_row_consumes_its_display_rows_in_the_window() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    // Each logical line wraps at the pane's width into two display rows: the
    // five `common00` tokens, then the indented `wordNNxx` tail. The tail only
    // appears in the window when the wrapped row is counted as a row.
    let rows = (0..8)
        .map(|index| {
            row(
                session_id,
                run_id,
                MessageKindDto::Assistant,
                &format!("common00 common00 common00 common00 common00 word{index:02}xx"),
            )
        })
        .collect();
    let mut state = open_state(session_id, rows);
    state.update(Action::ScrollTranscript(TranscriptScroll::Older));
    {
        let mut app = TestApp::with_size(RevueView::new(&state), 52, WINDOW_HEIGHT);
        let pilot = Pilot::new(&mut app);
        pilot.assert_not_contains("word07xx");
    }
    state.update(Action::ScrollTranscript(TranscriptScroll::Newer));
    {
        let mut app = TestApp::with_size(RevueView::new(&state), 52, WINDOW_HEIGHT);
        let pilot = Pilot::new(&mut app);
        pilot.assert_contains("word07xx");
    }
}

#[test]
fn an_assistant_heading_reads_as_a_heading() {
    let session_id = fixture_session(FIRST_SESSION);
    let run_id = RunId::new();
    let state = open_state(
        session_id,
        vec![row(
            session_id,
            run_id,
            MessageKindDto::Assistant,
            "# Answer\n\nbody text",
        )],
    );
    let mut app = TestApp::new(RevueView::new(&state));
    let (heading_x, heading_row) = find_text(&app, "Answer");
    let pilot = Pilot::new(&mut app);
    pilot.assert_line_contains(heading_row, "# Answer");
    pilot.assert_contains("body text");
    let heading = app
        .buffer()
        .get(heading_x, heading_row)
        .expect("the heading cell is rendered");
    assert!(
        heading.modifier.contains(Modifier::BOLD),
        "a heading is written bold, not only coloured"
    );
}

#[test]
fn a_bold_phrase_in_an_answer_is_bold() {
    let session_id = fixture_session(FIRST_SESSION);
    let run_id = RunId::new();
    let state = open_state(
        session_id,
        vec![row(
            session_id,
            run_id,
            MessageKindDto::Assistant,
            "plain **strong** tail",
        )],
    );
    let app = TestApp::new(RevueView::new(&state));
    let (strong_x, strong_row) = find_text(&app, "strong");
    let (plain_x, plain_row) = find_text(&app, "plain");
    let strong = app
        .buffer()
        .get(strong_x, strong_row)
        .expect("the strong cell is rendered");
    let plain = app
        .buffer()
        .get(plain_x, plain_row)
        .expect("the plain cell is rendered");
    assert!(
        strong.modifier.contains(Modifier::BOLD),
        "**strong** renders bold"
    );
    assert!(
        !plain.modifier.contains(Modifier::BOLD),
        "the text around it stays plain"
    );
}

#[test]
fn a_markdown_table_renders_as_an_aligned_grid() {
    let session_id = fixture_session(FIRST_SESSION);
    let run_id = RunId::new();
    let state = open_state(
        session_id,
        vec![row(
            session_id,
            run_id,
            MessageKindDto::Assistant,
            "| cell | count |\n|---|---|\n| alpha | 347 |\n| beta | 829 |",
        )],
    );
    let mut app = TestApp::new(RevueView::new(&state));
    let (alpha_x, _) = find_text(&app, "alpha");
    let (beta_x, _) = find_text(&app, "beta");
    let (first_x, _) = find_text(&app, "347");
    let (second_x, _) = find_text(&app, "829");
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("┌");
    pilot.assert_contains("└");
    assert_eq!(alpha_x, beta_x, "the first table column aligns");
    assert_eq!(first_x, second_x, "the second table column aligns");
}

#[test]
fn a_committed_reasoning_block_renders_above_the_answer_with_a_separator() {
    let session_id = fixture_session(FIRST_SESSION);
    let run_id = RunId::new();
    let state = open_state(
        session_id,
        vec![reasoning_row(
            session_id,
            run_id,
            "the answer",
            "short reasoning",
        )],
    );
    let mut app = TestApp::new(RevueView::new(&state));
    let (_, header_row) = find_text(&app, "∴ reasoning");
    let (_, reasoning_row) = find_text(&app, "short reasoning");
    let (_, answer_row) = find_text(&app, "the answer");
    let pilot = Pilot::new(&mut app);
    assert!(
        header_row < reasoning_row && reasoning_row < answer_row,
        "the reasoning block sits above the answer"
    );
    pilot.assert_line_contains(answer_row - 2, "────────");
    assert!(
        is_blank_pane_row(&pilot.line(answer_row - 1)),
        "one blank row separates the reasoning block from the answer"
    );
    pilot.assert_line_contains(answer_row, "∷ the answer");
}

#[test]
fn a_long_reasoning_block_collapses_with_a_hidden_line_marker() {
    let session_id = fixture_session(FIRST_SESSION);
    let run_id = RunId::new();
    let reasoning = (0..40)
        .map(|index| format!("step {index}"))
        .collect::<Vec<_>>()
        .join("\n");
    let state = open_state(
        session_id,
        vec![reasoning_row(session_id, run_id, "the answer", &reasoning)],
    );
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("… 25 more reasoning lines hidden");
    pilot.assert_contains("step 14");
    pilot.assert_not_contains("step 15");
    pilot.assert_not_contains("step 39");
    pilot.assert_contains("the answer");
}

#[test]
fn the_input_line_marks_the_cursor_position() {
    let session_id = SessionId::new();
    let mut state = open_state(session_id, Vec::new());
    for character in "ac".chars() {
        state.update(Action::InputChar(character));
    }
    state.update(Action::MoveInputCursor(InputCursorMove::Left));
    state.update(Action::InputChar('b'));
    let mut app = TestApp::new(RevueView::new(&state));
    let (cursor_x, cursor_row) = find_text(&app, "█");
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("> ab█c");
    pilot.assert_cell(cursor_x, cursor_row, '█');
}

#[test]
fn a_long_input_line_windows_around_the_cursor() {
    let session_id = SessionId::new();
    let mut state = open_state(session_id, Vec::new());
    let line = format!("HEAD{}TAIL", "x".repeat(96));
    for character in line.chars() {
        state.update(Action::InputChar(character));
    }
    let mut app = TestApp::with_size(
        RevueView::with_now(&state, NOW),
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
    );
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("TAIL█");
    pilot.assert_not_contains("HEAD");
    pilot.assert_not_contains(&format!("> {line}"));
}

#[test]
fn the_input_badges_show_the_real_mode_and_the_model_placeholder() {
    let session_id = fixture_session(FIRST_SESSION);
    let mut state = listed_state(session_id, 0);
    state.update(Action::SessionSnapshotLoaded(snapshot_in_mode(
        session_id,
        RunModeDto::Plan,
        None,
        Vec::new(),
    )));
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains(" plan ");
    pilot.assert_contains("model @todo(core)");
}

#[test]
fn the_input_badges_say_so_before_a_session_names_a_mode() {
    let mut state = AppState::new(None);
    state.update(Action::Bootstrapped(DaemonHealthDto::ready()));
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("mode @todo(core)");
    pilot.assert_contains("model @todo(core)");
}

#[test]
fn the_status_row_reads_ready_session_and_mode() {
    let session_id = fixture_session(FIRST_SESSION);
    let state = open_state(session_id, Vec::new());
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("Ready · session 11111111 · build");
    pilot.assert_contains("ctx @todo(core)");
}

#[test]
fn the_chat_hint_no_longer_advertises_the_history_keys() {
    let session_id = fixture_session(FIRST_SESSION);
    let state = open_state(session_id, Vec::new());
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("enter sends | /new session | /sessions switch");
    pilot.assert_not_contains("up/down");
    pilot.assert_not_contains("history");
}

#[test]
fn the_armed_interrupt_notice_names_the_run_a_second_press_stops() {
    let mut state = streaming_state(None);
    let run_id = state
        .active_run()
        .expect("the streaming state carries its live run")
        .run_id();
    state.update(Action::CtrlCPressed);
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    let expected = format!(
        "press Ctrl+C again to interrupt run {}",
        short_identifier(run_id)
    );
    pilot.assert_contains(&expected);
}

#[test]
fn the_armed_exit_notice_asks_for_a_second_ctrl_c() {
    let session_id = SessionId::new();
    let mut state = open_state(session_id, Vec::new());
    state.update(Action::CtrlCPressed);
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("press Ctrl+C again to exit");
}

#[test]
fn a_ctrl_c_press_clears_the_typed_input_line() {
    let session_id = SessionId::new();
    let mut state = open_state(session_id, Vec::new());
    for character in "abandoned".chars() {
        state.update(Action::InputChar(character));
    }
    state.update(Action::CtrlCPressed);
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("> █");
    pilot.assert_not_contains("abandoned");
}

#[test]
fn the_armed_esc_clear_asks_for_a_second_press() {
    let session_id = SessionId::new();
    let mut state = open_state(session_id, Vec::new());
    for character in "typed".chars() {
        state.update(Action::InputChar(character));
    }
    state.update(Action::EscapePressed);
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("press Esc again to clear the input");
    pilot.assert_contains("> typed");
}

#[test]
fn a_two_line_buffer_grows_the_input_block_and_shrinks_the_transcript() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = open_state(
        session_id,
        vec![row(
            session_id,
            run_id,
            MessageKindDto::Assistant,
            "the committed row",
        )],
    );
    let (single_badge, single_hint) = {
        let single = TestApp::with_size(
            RevueView::with_now(&state, NOW),
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
        );
        let (_, badge) = find_text(&single, "model @todo(core)");
        let (_, hint) = find_text(&single, "enter sends | /new session | /sessions switch");
        (badge, hint)
    };

    for character in "line one".chars() {
        state.update(Action::InputChar(character));
    }
    state.update(Action::InputChar('\\'));
    state.update(Action::InputSubmitted);
    for character in "line two".chars() {
        state.update(Action::InputChar(character));
    }
    assert_eq!(state.input(), "line one\nline two");

    let double = TestApp::with_size(
        RevueView::with_now(&state, NOW),
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
    );
    let (_, first_line) = find_text(&double, "> line one");
    let (_, second_line) = find_text(&double, "line two█");
    assert_eq!(
        second_line,
        first_line + 1,
        "one display row per buffer line, the cursor riding its own line"
    );
    let (_, double_badge) = find_text(&double, "model @todo(core)");
    let (_, double_hint) = find_text(&double, "enter sends | /new session | /sessions switch");
    assert_eq!(
        double_badge + 1,
        single_badge,
        "the block grew by exactly the line the buffer added"
    );
    assert_eq!(
        double_hint, single_hint,
        "the detail line keeps its row: the transcript gave up the block's new row"
    );
    find_text(&double, "the committed row");
}

#[test]
fn a_running_turn_shows_the_measured_elapsed_time() {
    let mut state = streaming_state(None);
    state.update(Action::ElapsedReported { millis: 3_200 });
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("Thinking… 3.2s");
}

#[test]
fn a_finished_run_shows_its_measured_duration() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = open_state(session_id, Vec::new());
    subscribe(&mut state, session_id, run_id, RunStatusDto::Completed);
    state.update(Action::ElapsedReported { millis: 13_000 });
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("Answered in 13s");
}

#[test]
fn the_status_row_never_shows_the_debug_vocabulary() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = open_state(session_id, Vec::new());
    subscribe(&mut state, session_id, run_id, RunStatusDto::Running);
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    for vocabulary in ["stream idle", "run none", "connected", "stream closed"] {
        pilot.assert_not_contains(vocabulary);
    }
}

#[test]
fn a_failed_run_reports_its_status_and_the_error_line() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = open_state(session_id, Vec::new());
    subscribe(&mut state, session_id, run_id, RunStatusDto::Failed);
    state.update(Action::RunStreamFailed(ErrorDto::unavailable(
        "run_stream_failed",
        "the run stream failed",
    )));
    let mut app = TestApp::new(RevueView::new(&state));
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("Failed");
    pilot.assert_contains("run_stream_failed: the run stream failed");
}

#[test]
fn scrolling_the_transcript_reveals_older_rows_and_cuts_the_newest() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let rows = (0..WINDOW_ROWS)
        .map(|index| {
            row(
                session_id,
                run_id,
                MessageKindDto::Assistant,
                &format!("row {index}"),
            )
        })
        .collect();
    let mut state = open_state(session_id, rows);
    // `Older` is the action one wheel-up notch produces: the window moves three
    // display rows (gaps included) towards the older answers. An answer's
    // marker rides its own first row, so one answer block is one display row
    // and one notch moves the window one answer and a half back.
    state.update(Action::ScrollTranscript(TranscriptScroll::Older));
    {
        let mut app = TestApp::with_size(
            RevueView::with_now(&state, NOW),
            WINDOW_WIDTH,
            WINDOW_HEIGHT,
        );
        let pilot = Pilot::new(&mut app);
        pilot.assert_contains("row 24");
        pilot.assert_not_contains("row 20");
        pilot.assert_not_contains("row 28");
        pilot.assert_not_contains("row 29");
    }
    state.update(Action::ScrollTranscript(TranscriptScroll::Newer));
    let mut app = TestApp::with_size(
        RevueView::with_now(&state, NOW),
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
    );
    let pilot = Pilot::new(&mut app);
    pilot.assert_contains("row 29");
    pilot.assert_not_contains("row 24");
}

#[test]
fn the_welcome_state_shows_the_lockup_and_the_overview_while_no_session_is_open() {
    let state = welcome_state();
    let app = TestApp::with_size(
        RevueView::with_now(&state, NOW),
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
    );
    find_text(&app, "INTENTION");
    find_text(&app, "R E L A Y");
    find_text(&app, &format!("Version {}", env!("CARGO_PKG_VERSION")));
    find_text(&app, "AGENTS.md @todo(core)");
    find_text(&app, "MCPs @todo(core)");
    find_text(&app, "Skills @todo(core)");
    assert!(
        app.find_text("transcript").is_none(),
        "the welcome replaces the transcript pane"
    );

    // The lockup is centred in the 74-column content: its 20 columns leave 27
    // columns on either side, and the card's content starts at column 3.
    let (product_x, product_row) = find_text(&app, "INTENTION");
    let (trailing_x, trailing_row) = find_text(&app, "R E L A Y");
    assert_eq!(product_x, 30, "the lockup is centred in the chat");
    assert_eq!(trailing_row, product_row, "the lockup is one line");
    assert_eq!(
        trailing_x,
        product_x + 9 + 2,
        "the trailing word follows INTENTION and the documented two-column gap"
    );
    let product = app
        .buffer()
        .get(product_x, product_row)
        .expect("the product word paints a cell");
    assert!(
        product.modifier.contains(Modifier::BOLD),
        "the product word is the bold half of the lockup"
    );
    let trailing = app
        .buffer()
        .get(trailing_x, trailing_row)
        .expect("the trailing word paints a cell");
    assert!(
        !trailing.modifier.contains(Modifier::BOLD),
        "the trailing word is the muted, tracked half"
    );

    let (_, overview_row) = find_text(&app, "AGENTS.md @todo(core)");
    let (_, input_row) = find_text(&app, "model @todo(core)");
    assert!(
        product_row < overview_row,
        "the overview sits below the lockup"
    );
    assert!(
        overview_row < input_row,
        "the overview sits above the input block"
    );
    find_text(&app, "> █");
    find_text(&app, "enter sends | /new session | /sessions switch");
}

#[test]
fn the_welcome_state_is_gone_once_a_session_is_open() {
    let state = open_state(fixture_session(FIRST_SESSION), Vec::new());
    let app = TestApp::with_size(
        RevueView::with_now(&state, NOW),
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
    );
    assert!(
        app.find_text("INTENTION").is_none(),
        "an open session keeps the transcript pane"
    );
    assert!(app.find_text("Skills @todo(core)").is_none());
    let (_, transcript_row) = find_text(&app, "transcript");
    let (_, input_row) = find_text(&app, "model @todo(core)");
    assert!(transcript_row < input_row, "the pane keeps its region");
}

#[test]
fn the_welcome_state_never_covers_a_row_the_core_reports() {
    // A run subscription can append committed rows before any session snapshot
    // opens, so the transcript with rows wins even with no open session.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = AppState::new(None);
    state.update(Action::Bootstrapped(DaemonHealthDto::ready()));
    let subscription = RunSubscriptionSnapshotDto::new(
        run(session_id, run_id, RunStatusDto::Running),
        vec![row(
            session_id,
            run_id,
            MessageKindDto::Assistant,
            "the committed row",
        )],
    )
    .expect("the fixture run snapshot is coherent");
    let mut stream = RunStreamState::new(session_id, run_id);
    stream
        .apply_initial(subscription)
        .expect("the fixture run snapshot applies to its own scope");
    state.update(Action::RunStreamOpened(stream));
    assert!(state.session_id().is_none(), "no session snapshot arrived");
    assert_eq!(state.transcript().len(), 1, "one committed row is present");

    let app = TestApp::with_size(
        RevueView::with_now(&state, NOW),
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
    );
    find_text(&app, "the committed row");
    assert!(
        app.find_text("INTENTION").is_none(),
        "a reported row keeps the transcript pane"
    );
    assert!(app.find_text("Skills @todo(core)").is_none());
}

#[test]
fn a_launch_that_lists_sessions_shows_the_welcome_until_a_session_is_asked_for() {
    let listed = fixture_session(FIRST_SESSION);
    let mut state = AppState::new(None);
    state.update(Action::Bootstrapped(DaemonHealthDto::ready()));
    state.update(Action::SessionsListed(
        SessionSummariesDto::new(vec![summary(listed)], 0)
            .expect("the fixture session list is coherent"),
    ));
    assert_eq!(state.session_id(), None, "the launch opened no session");
    assert!(state.transcript().is_empty());
    assert_eq!(
        state.sessions().len(),
        1,
        "the session exists and is listed"
    );

    let app = TestApp::with_size(
        RevueView::with_now(&state, NOW),
        WINDOW_WIDTH,
        WINDOW_HEIGHT,
    );
    find_text(&app, "INTENTION");
    find_text(&app, &format!("Version {}", env!("CARGO_PKG_VERSION")));
    find_text(&app, "Skills @todo(core)");
    find_text(&app, "> █");
    assert!(
        app.find_text("transcript").is_none(),
        "a listed session the launch did not ask for draws no transcript"
    );
}
