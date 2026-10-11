#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "Session-stream client fixtures use direct assertions for diagnostics."
)]

#[allow(
    dead_code,
    reason = "Shared fixtures serve every integration target in this crate; each target compiles the subset its suite calls."
)]
mod common;

use common::{TEST_REPLY_BOUND, endpoint, message};

use intention_client::{RETAINED_TRANSCRIPT_MESSAGES, SessionStreamClient, SessionStreamState};
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
    RunProjectionDto::new(
        session_id,
        run_id,
        TurnId::new(),
        status,
        ConfigRevisionId::new(),
    )
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
    state
        .apply_initial(snapshot(
            session_id,
            None,
            vec![message(
                session_id,
                Some(run_id),
                MessageKindDto::User,
                "build it",
            )],
        ))
        .expect("current-state snapshot applies");
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
    state
        .apply_initial(snapshot(
            session_id,
            Some(run(session_id, run_id, RunStatusDto::Running)),
            Vec::new(),
        ))
        .expect("fixture snapshot applies");

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
            .apply_initial(snapshot(SessionId::new(), None, Vec::new()))
            .expect_err("a snapshot from another session is rejected")
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
    state
        .apply_initial(snapshot(
            session_id,
            Some(run(session_id, run_id, RunStatusDto::Running)),
            vec![snapshot_row.clone()],
        ))
        .expect("fixture snapshot applies");

    let repeated = MessageProjectionDto::new(
        snapshot_row.id(),
        session_id,
        Some(run_id),
        MessageKindDto::Assistant,
        "the row the snapshot already carried",
        None,
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

#[test]
fn state_merges_another_read_of_the_same_transcript_by_row_identity() {
    let session_id = SessionId::new();
    let run_id = RunId::new();
    let mut state = SessionStreamState::new(session_id);
    let first = message(session_id, Some(run_id), MessageKindDto::User, "build it");
    let second = message(
        session_id,
        Some(run_id),
        MessageKindDto::Assistant,
        "working",
    );
    let third = message(session_id, Some(run_id), MessageKindDto::Notice, "stopped");
    state
        .apply_initial(snapshot(
            session_id,
            Some(run(session_id, run_id, RunStatusDto::Running)),
            vec![first.clone(), second.clone(), third.clone()],
        ))
        .expect("fixture snapshot applies");

    // A session read carrying the outer rows leaves exactly the row it never
    // carried.
    assert_eq!(
        state.missing_rows(&[first.clone(), third.clone()]),
        vec![second.clone()]
    );

    // Equal content with a distinct durable identity is still missing.
    let equal_content = message(
        session_id,
        Some(run_id),
        MessageKindDto::Assistant,
        "working",
    );
    assert_eq!(
        state.missing_rows(&[first, equal_content]),
        vec![second, third]
    );

    // A read that already carries every row leaves nothing to merge.
    assert!(state.missing_rows(state.messages()).is_empty());
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

    let content = subscription
        .receive()
        .await
        .expect("content frame reads")
        .expect("content frame is not a stream close");
    assert!(matches!(
        content,
        SessionFrameDto::Content(ref row) if row.text() == "working"
    ));
    assert_eq!(subscription.state().messages().len(), 2);

    let status = subscription
        .receive()
        .await
        .expect("status frame reads")
        .expect("status frame is not a stream close");
    // The frame carries the committed projection itself; the fixture builds
    // each projection with fresh turn and config identities, so the assertion
    // checks the run scope and terminal status instead of object identity.
    let SessionFrameDto::Status(projection) = status else {
        unreachable!("a status frame carries the committed run projection")
    };
    assert_eq!(projection.session_id(), session_id);
    assert_eq!(projection.run_id(), run_id);
    assert_eq!(projection.status(), RunStatusDto::Completed);
    assert_eq!(subscription.state().status(), Some(RunStatusDto::Completed));

    assert!(
        subscription
            .receive()
            .await
            .expect("a closed stream reads as a clean end")
            .is_none(),
        "the closed stream reports no further frames"
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
