#[allow(
    dead_code,
    reason = "Shared fixtures serve every integration target in this crate; each target compiles the subset its suite calls."
)]
mod common;

use common::{TEST_REPLY_BOUND, endpoint, message};

use intention_client::{
    FrameUpdate, RETAINED_TRANSCRIPT_MESSAGES, SessionStreamClient, SessionStreamState, StreamStep,
};
use intention_proto::{
    ClientRequestDto, ConfigRevisionId, MessageKindDto, MessageProjectionDto, ProjectId,
    ProtocolDaemonMessageDto, ProtocolResultDto, RunId, RunModeDto, RunProjectionDto, RunStatusDto,
    SessionFrameDto, SessionId, SessionProjectionDto, SessionSnapshotDto,
    SubscribeSessionCommandDto, TextDeltaChannelDto, TextDeltaFrameDto, TurnId, WorkspaceId,
    WorkspaceRootDto, decode_request_line, encode_reply,
};
use intention_transport::{AsyncLocalListener, AsyncMessageReceiver};

/// Receives the one subscription request a fixture peer answers.
async fn receive_session_subscription(
    requests: &mut AsyncMessageReceiver,
) -> (u64, SubscribeSessionCommandDto) {
    let line = requests.receive_line().await.expect("request arrives");
    let request = decode_request_line(&line).expect("request decodes");
    match request.request() {
        ClientRequestDto::SubscribeSession(subscription) => (request.id(), *subscription),
        other => panic!("expected a session subscription request, got {other:?}"),
    }
}

fn run(session_id: SessionId, run_id: RunId, status: RunStatusDto) -> RunProjectionDto {
    let started_at = intention_proto::TimestampDto::from_unix_seconds(1_726_000_000)
        .expect("fixture timestamp is valid");
    let finished_at = intention_proto::run_status_is_terminal(status).then_some(started_at);
    RunProjectionDto::new(
        session_id,
        run_id,
        TurnId::new(),
        status,
        ConfigRevisionId::new(),
        None,
        started_at,
        finished_at,
        0,
    )
    .expect("fixture run projection is valid")
}

/// Returns one fixture session projection with an optional active run.
fn projection(session_id: SessionId, active_run: Option<RunProjectionDto>) -> SessionProjectionDto {
    let root = std::env::temp_dir().join("intention-client-session-fixture");
    SessionProjectionDto::new(
        ProjectId::new(),
        session_id,
        WorkspaceId::new(),
        WorkspaceRootDto::parse(root.to_string_lossy().into_owned())
            .expect("fixture workspace root is absolute"),
        RunModeDto::Build,
        None,
        active_run,
        Vec::new(),
    )
    .expect("fixture session projection is valid")
}

fn snapshot(
    session_id: SessionId,
    active_run: Option<RunProjectionDto>,
    messages: Vec<MessageProjectionDto>,
) -> SessionSnapshotDto {
    SessionSnapshotDto::with_projection(session_id, projection(session_id, active_run), messages)
        .expect("fixture session snapshot is valid")
}

#[test]
fn state_applies_the_subscription_snapshot_then_committed_frames() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = SessionStreamState::new(session_id);
    assert!(state.run().is_none());

    // A session between runs carries no active run: the snapshot is the
    // transcript, and the status frame that follows opens the run another
    // window started.
    let read = SessionStreamState::from_snapshot(snapshot(
        session_id,
        None,
        vec![message(
            session_id,
            Some(run_id),
            MessageKindDto::User,
            "build it",
        )],
    ));
    state
        .reconcile(read)
        .expect("a read of this state's own session merges");
    assert!(state.run().is_none());
    assert_eq!(state.messages().len(), 1);

    state
        .apply_frame(SessionFrameDto::Status(run(
            session_id,
            run_id,
            RunStatusDto::Running,
        )))
        .expect("a status frame opens the run the session started afterwards");
    assert_eq!(state.status(), Some(RunStatusDto::Running));
    assert_eq!(
        state.run().expect("the run projection is current").run_id(),
        run_id
    );

    state
        .apply_frame(SessionFrameDto::Content(message(
            session_id,
            Some(run_id),
            MessageKindDto::Assistant,
            "working",
        )))
        .expect("content frame applies");
    state
        .apply_frame(SessionFrameDto::Status(run(
            session_id,
            run_id,
            RunStatusDto::Completed,
        )))
        .expect("status frame applies");
    assert_eq!(state.status(), Some(RunStatusDto::Completed));
    assert_eq!(state.messages().len(), 2);
    assert_eq!(state.messages()[1].text(), "working");
}

#[test]
fn state_accepts_another_run_of_its_session_and_rejects_another_session() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let other_run = RunId::new();
    let mut state = SessionStreamState::new(session_id);
    let read = SessionStreamState::from_snapshot(snapshot(
        session_id,
        Some(run(session_id, run_id, RunStatusDto::Running)),
        Vec::new(),
    ));
    state
        .reconcile(read)
        .expect("a read of this state's own session merges");

    // The reported defect: a run another window started belongs to this
    // window's session, so its projection and rows apply here too.
    state
        .apply_frame(SessionFrameDto::Status(run(
            session_id,
            other_run,
            RunStatusDto::Running,
        )))
        .expect("a status frame of the session's next run applies");
    assert_eq!(
        state.run().expect("the newer run is current").run_id(),
        other_run
    );
    state
        .apply_frame(SessionFrameDto::Content(message(
            session_id,
            Some(other_run),
            MessageKindDto::Assistant,
            "from the other window",
        )))
        .expect("a row of the session's next run applies");
    assert_eq!(state.messages().len(), 1);

    // A committed session row that belongs to no run applies as well: a
    // session stream carries the session's rows, not one run's.
    state
        .apply_frame(SessionFrameDto::Content(message(
            session_id,
            None,
            MessageKindDto::Notice,
            "session notice",
        )))
        .expect("a session-level row without a run identity applies");
    assert_eq!(state.messages().len(), 2);

    let mut foreign = SessionStreamState::new(session_id);
    assert_eq!(
        foreign
            .reconcile(SessionStreamState::from_snapshot(snapshot(
                SessionId::new(),
                None,
                Vec::new(),
            )))
            .expect_err("a read from another session is rejected")
            .code(),
        "invalid_session_subscription"
    );
    assert_eq!(
        foreign
            .apply_frame(SessionFrameDto::Content(message(
                SessionId::new(),
                None,
                MessageKindDto::Notice,
                "stray",
            )))
            .expect_err("a content frame from another session is rejected")
            .code(),
        "invalid_session_subscription"
    );
    assert_eq!(
        foreign
            .apply_frame(SessionFrameDto::Status(run(
                SessionId::new(),
                RunId::new(),
                RunStatusDto::Running,
            )))
            .expect_err("a status frame from another session is rejected")
            .code(),
        "invalid_session_subscription"
    );
    assert_eq!(
        foreign
            .apply_frame(SessionFrameDto::TextDelta(
                TextDeltaFrameDto::new(
                    SessionId::new(),
                    RunId::new(),
                    0,
                    TextDeltaChannelDto::Answer,
                    "stray",
                )
                .expect("fixture delta is valid"),
            ))
            .expect_err("a text delta from another session is rejected")
            .code(),
        "invalid_session_subscription"
    );
    assert!(foreign.run().is_none());
    assert!(foreign.messages().is_empty());
}

#[test]
fn state_dedupes_the_newest_row_and_bounds_the_transcript() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = SessionStreamState::new(session_id);
    let snapshot_row = message(
        session_id,
        Some(run_id),
        MessageKindDto::Assistant,
        "committed by the snapshot",
    );
    let read = SessionStreamState::from_snapshot(snapshot(
        session_id,
        Some(run(session_id, run_id, RunStatusDto::Running)),
        vec![snapshot_row.clone()],
    ));
    state
        .reconcile(read)
        .expect("a read of this state's own session merges");

    let repeated = MessageProjectionDto::new(
        snapshot_row.id(),
        session_id,
        Some(run_id),
        MessageKindDto::Assistant,
        "the row the snapshot already carried",
        None,
        None,
    )
    .expect("fixture repeated row is valid");
    state
        .apply_frame(SessionFrameDto::Content(repeated))
        .expect("a frame repeating the newest accepted row applies once");
    assert_eq!(
        state.messages().len(),
        1,
        "a content frame carrying the newest row identity is not appended twice"
    );

    // Equal content with a distinct durable identity is a distinct row, so it
    // is a committed row the snapshot never carried.
    let equal_content = message(
        session_id,
        Some(run_id),
        MessageKindDto::Assistant,
        "the row the snapshot already carried",
    );
    state
        .apply_frame(SessionFrameDto::Content(equal_content))
        .expect("a distinct row with equal content applies");
    assert_eq!(
        state.messages().len(),
        2,
        "rows match by durable identity, never by value"
    );

    for index in 0..RETAINED_TRANSCRIPT_MESSAGES + 4 {
        state
            .apply_frame(SessionFrameDto::Content(message(
                session_id,
                Some(run_id),
                MessageKindDto::Notice,
                &format!("row {index}"),
            )))
            .expect("a fresh committed row applies");
    }
    assert_eq!(state.messages().len(), RETAINED_TRANSCRIPT_MESSAGES);
    assert_eq!(
        state.messages().last().expect("rows remain").text(),
        format!("row {}", RETAINED_TRANSCRIPT_MESSAGES + 3),
        "the retained transcript keeps the newest rows"
    );
}

/// Returns one state that has read `messages` of its own session.
fn reading_state(
    session_id: SessionId,
    active_run: Option<RunProjectionDto>,
    messages: Vec<MessageProjectionDto>,
) -> SessionStreamState {
    let mut state = SessionStreamState::new(session_id);
    state
        .reconcile(SessionStreamState::from_snapshot(snapshot(
            session_id, active_run, messages,
        )))
        .expect("a read of the state's own session merges");
    state
}

/// Returns one running fixture run projection of `session_id`.
fn running(session_id: SessionId, run_id: RunId) -> RunProjectionDto {
    run(session_id, run_id, RunStatusDto::Running)
}

/// Returns one answer-channel delta of `step` for a fixture run.
fn answer_delta(session_id: SessionId, run_id: RunId, step: u32, text: &str) -> SessionFrameDto {
    SessionFrameDto::TextDelta(
        TextDeltaFrameDto::new(session_id, run_id, step, TextDeltaChannelDto::Answer, text)
            .expect("the fixture delta is valid"),
    )
}

#[test]
fn state_reports_the_update_of_every_frame_kind() {
    // The client reports what each frame changed, so a consumer never re-derives
    // the acceptance decision for itself.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let current = running(session_id, run_id);
    let mut state = reading_state(session_id, Some(current), Vec::new());
    // A transient delta advances the provisional buffer of its own run and step.
    assert_eq!(
        state
            .apply_frame(answer_delta(session_id, run_id, 3, "hello"))
            .expect("a delta of the projected run applies"),
        FrameUpdate::Delta { run_id, step: 3 }
    );
    // A delta of a run this state does not project changes nothing.
    assert_eq!(
        state
            .apply_frame(answer_delta(session_id, RunId::new(), 0, "stray"))
            .expect("a delta of another run applies as a no-op"),
        FrameUpdate::Unchanged
    );

    // A committed row is reported by the identity it appended, and the same row
    // repeated changes nothing.
    let row = message(session_id, Some(run_id), MessageKindDto::Assistant, "hello");
    assert_eq!(
        state
            .apply_frame(SessionFrameDto::Content(row.clone()))
            .expect("the committed row applies"),
        FrameUpdate::Row(row.id())
    );
    assert_eq!(
        state
            .apply_frame(SessionFrameDto::Content(row))
            .expect("a repeated row applies once"),
        FrameUpdate::Unchanged
    );

    // A status frame replaces the projected run; repeating the same projection
    // changes nothing.
    assert_eq!(
        state
            .apply_frame(SessionFrameDto::Status(current))
            .expect("the projected run's status applies"),
        FrameUpdate::Unchanged,
        "the projected run already carries this projection"
    );
    assert_eq!(
        state
            .apply_frame(SessionFrameDto::Status(run(
                session_id,
                run_id,
                RunStatusDto::Completed,
            )))
            .expect("the terminal status applies"),
        FrameUpdate::Run(run_id)
    );
}

#[test]
fn state_names_the_step_its_committed_assistant_row_superseded() {
    // The identity a sink needs to suppress streamed text is the client's: the
    // committed assistant row of a step carries the step it became.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = reading_state(session_id, Some(running(session_id, run_id)), Vec::new());
    state
        .apply_frame(answer_delta(session_id, run_id, 2, "half"))
        .expect("the delta applies");
    let row = message(session_id, Some(run_id), MessageKindDto::Assistant, "half");
    state
        .apply_frame(SessionFrameDto::Content(row.clone()))
        .expect("the committed row applies");
    assert_eq!(
        state.superseded_step(row.id()),
        Some(2),
        "the row carries the step whose streamed text it became"
    );

    // A row that superseded no streamed text names no step.
    let notice = message(
        session_id,
        Some(run_id),
        MessageKindDto::Notice,
        "still running",
    );
    state
        .apply_frame(SessionFrameDto::Content(notice.clone()))
        .expect("the notice applies");
    assert_eq!(state.superseded_step(notice.id()), None);

    // The identity lives exactly as long as its row: a front drain drops it.
    for index in 0..RETAINED_TRANSCRIPT_MESSAGES {
        state
            .apply_frame(SessionFrameDto::Content(message(
                session_id,
                Some(run_id),
                MessageKindDto::Notice,
                &format!("row {index}"),
            )))
            .expect("a fresh committed row applies");
    }
    assert!(
        state
            .messages()
            .iter()
            .all(|committed| committed.id() != row.id()),
        "the bound drained the superseded row"
    );
    assert_eq!(
        state.superseded_step(row.id()),
        None,
        "a drained row keeps no identity to report"
    );
}

#[test]
fn state_reconciles_a_newer_read_by_row_identity() {
    // The union of two reads of one durable transcript, matched by durable row
    // identity: equal content with distinct identities is never conflated.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let first = message(session_id, Some(run_id), MessageKindDto::User, "build it");
    let second = message(
        session_id,
        Some(run_id),
        MessageKindDto::Assistant,
        "working",
    );
    let third = message(session_id, Some(run_id), MessageKindDto::Notice, "stopped");
    let mut state = reading_state(
        session_id,
        Some(running(session_id, run_id)),
        vec![first.clone(), third.clone()],
    );
    assert_eq!(state.messages().len(), 2);
    assert_eq!(state.messages()[0], first);
    assert_eq!(state.messages()[1], third);

    // A newer read carrying the row between them joins it in durable order.
    state
        .reconcile(SessionStreamState::from_snapshot(snapshot(
            session_id,
            Some(running(session_id, run_id)),
            vec![second.clone()],
        )))
        .expect("a second read of the same session merges");
    assert_eq!(state.messages().len(), 3);
    assert_eq!(state.messages()[0], first);
    assert_eq!(state.messages()[1], second);
    assert_eq!(state.messages()[2], third);

    // Equal content with a distinct durable identity is a distinct row.
    let equal_content = message(
        session_id,
        Some(run_id),
        MessageKindDto::Assistant,
        "working",
    );
    state
        .reconcile(SessionStreamState::from_snapshot(snapshot(
            session_id,
            Some(running(session_id, run_id)),
            vec![equal_content.clone()],
        )))
        .expect("a read carrying equal content merges by identity");
    assert_eq!(
        state.messages().len(),
        4,
        "rows match by identity, never by value"
    );
    assert_eq!(
        state.messages().last().expect("rows remain"),
        &equal_content
    );

    // A read that already carries every row adds nothing.
    state
        .reconcile(SessionStreamState::from_snapshot(snapshot(
            session_id,
            Some(running(session_id, run_id)),
            state.messages().to_vec(),
        )))
        .expect("a read of its own rows merges");
    assert_eq!(state.messages().len(), 4);
}

#[test]
fn state_reconciliation_keeps_the_newest_retained_rows() {
    // A read larger than the client's window is bounded by the client itself:
    // the newest rows stay, in durable order.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let rows: Vec<_> = (0..RETAINED_TRANSCRIPT_MESSAGES + 4)
        .map(|index| {
            message(
                session_id,
                Some(run_id),
                MessageKindDto::Notice,
                &format!("row {index}"),
            )
        })
        .collect();
    let state = reading_state(session_id, Some(running(session_id, run_id)), rows.clone());
    assert_eq!(state.messages().len(), RETAINED_TRANSCRIPT_MESSAGES);
    assert_eq!(
        state.messages(),
        &rows[4..],
        "the window keeps the newest rows in durable order"
    );
}

#[test]
fn state_walks_the_rows_after_a_reported_identity_past_the_retention_bound() {
    // A reporter that walks by durable identity sees every committed row once,
    // even past the client's retention bound, and a reported row the bound has
    // since dropped never blocks the next report.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = reading_state(session_id, Some(running(session_id, run_id)), Vec::new());
    let mut reported = None;
    let mut reported_ids = Vec::new();
    for index in 0..RETAINED_TRANSCRIPT_MESSAGES + 4 {
        state
            .apply_frame(SessionFrameDto::Content(message(
                session_id,
                Some(run_id),
                MessageKindDto::Notice,
                &format!("row {index}"),
            )))
            .expect("a committed row applies");
        for pending in state.rows_after(reported) {
            reported = Some(pending.id());
            reported_ids.push(pending.id());
        }
    }
    assert_eq!(
        reported_ids.len(),
        RETAINED_TRANSCRIPT_MESSAGES + 4,
        "every committed row is reported exactly once"
    );
    assert_eq!(
        state.rows_after(reported),
        &[] as &[MessageProjectionDto],
        "a reporter that reported the newest row has nothing pending"
    );

    // The row committed past the bound is the next reported row.
    let newest = message(
        session_id,
        Some(run_id),
        MessageKindDto::Notice,
        "the newest row",
    );
    state
        .apply_frame(SessionFrameDto::Content(newest.clone()))
        .expect("a committed row applies");
    assert_eq!(
        state.rows_after(reported),
        std::slice::from_ref(&newest),
        "the row committed past the bound is reported"
    );

    // A reported row the bound dropped never blocks the next report.
    let drained = reported_ids[0];
    assert!(
        state.messages().iter().all(|row| row.id() != drained),
        "the bound dropped the first reported row"
    );
    assert!(
        !state.rows_after(Some(drained)).is_empty(),
        "a drained reported identity still lets the retained rows through"
    );
}

#[test]
fn state_reports_appends_only_while_the_rows_at_the_epoch_stay_a_prefix() {
    // The epoch machinery a layout cache keys on: appends answer a count, and a
    // reconciliation, a read, or a front drain ends the append-only run.
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = reading_state(
        session_id,
        Some(running(session_id, run_id)),
        vec![message(
            session_id,
            Some(run_id),
            MessageKindDto::User,
            "hello",
        )],
    );
    let epoch = state.transcript_epoch();
    assert_eq!(
        state.transcript_appended_since(epoch),
        Some(0),
        "a transcript that changed nothing appended nothing"
    );
    state
        .apply_frame(SessionFrameDto::Content(message(
            session_id,
            Some(run_id),
            MessageKindDto::Assistant,
            "one",
        )))
        .expect("a committed row applies");
    assert_eq!(state.transcript_appended_since(epoch), Some(1));
    state
        .apply_frame(SessionFrameDto::Content(message(
            session_id,
            Some(run_id),
            MessageKindDto::Assistant,
            "two",
        )))
        .expect("a committed row applies");
    assert_eq!(state.transcript_appended_since(epoch), Some(2));
    assert_eq!(
        state.transcript_appended_since(state.transcript_epoch()),
        Some(0)
    );
    assert_eq!(
        state.transcript_appended_since(state.transcript_epoch() + 1),
        None,
        "a future epoch is never answered append-only"
    );

    // A reconciliation is a replacement: the rows current at the epoch are no
    // longer a prefix of the transcript.
    let epoch = state.transcript_epoch();
    state
        .reconcile(SessionStreamState::from_snapshot(snapshot(
            session_id,
            Some(running(session_id, run_id)),
            vec![message(
                session_id,
                Some(run_id),
                MessageKindDto::Notice,
                "three",
            )],
        )))
        .expect("a read of the same session merges");
    assert_eq!(
        state.transcript_appended_since(epoch),
        None,
        "a reconciliation ends the append-only run"
    );

    // A front drain is a replacement too.
    let rows: Vec<_> = (0..RETAINED_TRANSCRIPT_MESSAGES)
        .map(|index| {
            message(
                session_id,
                Some(run_id),
                MessageKindDto::Notice,
                &format!("row {index}"),
            )
        })
        .collect();
    let mut state = reading_state(session_id, Some(running(session_id, run_id)), rows);
    let epoch = state.transcript_epoch();
    state
        .apply_frame(SessionFrameDto::Content(message(
            session_id,
            Some(run_id),
            MessageKindDto::Notice,
            "the newest row",
        )))
        .expect("a committed row applies");
    assert_eq!(
        state.transcript_appended_since(epoch),
        None,
        "a front drain ends the append-only run"
    );
}

#[tokio::test]
async fn subscribe_delivers_current_state_then_live_content_and_status_frames() {
    let endpoint = endpoint();
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("peer accepts");
        let (mut requests, mut messages) = connection.split();
        let (request_id, request) = receive_session_subscription(&mut requests).await;
        assert_eq!(request.session_id(), session_id);
        messages
            .send_message(&encode_reply(
                request_id,
                ProtocolResultDto::SessionSubscribed(snapshot(
                    session_id,
                    Some(run(session_id, run_id, RunStatusDto::Running)),
                    vec![message(
                        session_id,
                        Some(run_id),
                        MessageKindDto::User,
                        "build it",
                    )],
                )),
            ))
            .await
            .expect("snapshot reply sends");
        messages
            .send_message(&ProtocolDaemonMessageDto::frame(SessionFrameDto::Content(
                message(
                    session_id,
                    Some(run_id),
                    MessageKindDto::Assistant,
                    "working",
                ),
            )))
            .await
            .expect("content frame sends");
        messages
            .send_message(&ProtocolDaemonMessageDto::frame(SessionFrameDto::Status(
                run(session_id, run_id, RunStatusDto::Completed),
            )))
            .await
            .expect("status frame sends");
    });

    let client = SessionStreamClient::new(endpoint);
    let mut subscription = client
        .subscribe(SubscribeSessionCommandDto::new(session_id))
        .await
        .expect("current-state snapshot reply arrives");
    assert_eq!(subscription.state().session_id(), session_id);
    assert_eq!(subscription.state().status(), Some(RunStatusDto::Running));
    assert_eq!(subscription.state().messages().len(), 1);
    assert_eq!(
        subscription.state().messages()[0].kind(),
        MessageKindDto::User
    );

    let content = subscription.receive().await.expect("content frame reads");
    assert!(matches!(
        content,
        StreamStep::Frame(SessionFrameDto::Content(ref row)) if row.text() == "working"
    ));
    assert_eq!(subscription.state().messages().len(), 2);

    let status = subscription.receive().await.expect("status frame reads");
    // The frame carries the committed projection itself; the fixture builds
    // each projection with fresh turn and config identities, so the assertion
    // checks the run scope and terminal status instead of object identity.
    let StreamStep::Frame(SessionFrameDto::Status(projection)) = status else {
        unreachable!("a status frame carries the committed run projection")
    };
    assert_eq!(projection.session_id(), session_id);
    assert_eq!(projection.run_id(), run_id);
    assert_eq!(projection.status(), RunStatusDto::Completed);
    assert_eq!(subscription.state().status(), Some(RunStatusDto::Completed));

    assert!(
        matches!(
            subscription
                .receive()
                .await
                .expect("a closed stream reads as the client's classification"),
            StreamStep::Closed(_)
        ),
        "the closed stream carries the client's own classification"
    );
    server.await.expect("scripted peer completes");
}

#[tokio::test]
async fn a_closed_stream_carries_the_clients_own_classification() {
    let endpoint = endpoint();
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("peer accepts");
        let (mut requests, mut messages) = connection.split();
        let (request_id, _request) = receive_session_subscription(&mut requests).await;
        messages
            .send_message(&encode_reply(
                request_id,
                ProtocolResultDto::SessionSubscribed(snapshot(
                    session_id,
                    Some(run(session_id, run_id, RunStatusDto::Running)),
                    Vec::new(),
                )),
            ))
            .await
            .expect("snapshot reply sends");
        // Dropping the connection closes the stream while the run is live.
    });

    let client = SessionStreamClient::new(endpoint);
    let mut subscription = client
        .subscribe(SubscribeSessionCommandDto::new(session_id))
        .await
        .expect("current-state snapshot reply arrives");
    let step = subscription
        .receive()
        .await
        .expect("a closed connection is not a protocol error");
    let StreamStep::Closed(error) = step else {
        unreachable!("the peer closed the connection without sending a frame")
    };
    assert_eq!(error.code(), "local_daemon_connection_unavailable");
    assert_eq!(
        error,
        SessionStreamClient::closed_stream_failure(),
        "the closed stream reports the client's own classification, not a composed one"
    );
    server.await.expect("scripted peer completes");
}

#[tokio::test]
async fn receive_rejects_a_live_frame_from_another_session() {
    let endpoint = endpoint();
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("peer accepts");
        let (mut requests, mut messages) = connection.split();
        let (request_id, _) = receive_session_subscription(&mut requests).await;
        messages
            .send_message(&encode_reply(
                request_id,
                ProtocolResultDto::SessionSubscribed(snapshot(
                    session_id,
                    Some(run(session_id, run_id, RunStatusDto::Running)),
                    Vec::new(),
                )),
            ))
            .await
            .expect("snapshot reply sends");
        messages
            .send_message(&ProtocolDaemonMessageDto::frame(SessionFrameDto::Status(
                run(SessionId::new(), RunId::new(), RunStatusDto::Completed),
            )))
            .await
            .expect("foreign status frame sends");
    });

    let client = SessionStreamClient::new(endpoint);
    let mut subscription = client
        .subscribe(SubscribeSessionCommandDto::new(session_id))
        .await
        .expect("current-state snapshot reply arrives");
    let error = subscription
        .receive()
        .await
        .expect_err("a frame from another session is rejected");
    assert_eq!(error.code(), "invalid_session_subscription");
    assert_eq!(subscription.state().status(), Some(RunStatusDto::Running));
    assert!(subscription.state().messages().is_empty());
    server.await.expect("scripted peer completes");
}

#[tokio::test]
async fn subscribe_returns_a_typed_error_when_the_channel_closes_before_the_reply() {
    let endpoint = endpoint();
    let session_id = SessionId::new();
    let listener = AsyncLocalListener::bind(endpoint.clone()).expect("listener binds");
    let server = tokio::spawn(async move {
        let connection = listener.accept().await.expect("peer accepts");
        let (mut requests, _messages) = connection.split();
        let _ = receive_session_subscription(&mut requests).await;
    });

    let client = SessionStreamClient::new(endpoint);
    let error = tokio::time::timeout(
        TEST_REPLY_BOUND,
        client.subscribe(SubscribeSessionCommandDto::new(session_id)),
    )
    .await
    .expect("a closed channel must not hang the subscriber")
    .err()
    .expect("a closed channel before the reply is a typed error");
    assert_eq!(error.code(), "local_daemon_connection_unavailable");
    server.await.expect("scripted peer completes");
}
